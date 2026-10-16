use crate::repo::LocalRepoManager;
use anyhow::{Context, Result};
use std::fs;
use std::io::{BufRead, BufReader};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct PackageBuilder {
    build_dir: PathBuf,
}

impl PackageBuilder {
    pub fn new() -> Result<Self> {
        let home = std::env::var("HOME").context("HOME environment variable not set")?;
        let build_dir = PathBuf::from(home).join(".cache").join("arc").join("builds");
        fs::create_dir_all(&build_dir)?;
        Ok(Self { build_dir })
    }

    pub fn fetch_source(&self, pkg_name: &str, clean: bool, cancel: &Arc<AtomicBool>) -> Result<PathBuf> {
        let target_dir = self.build_dir.join(pkg_name);

        if cancel.load(Ordering::Relaxed) {
            anyhow::bail!("Build aborted by user");
        }

        if clean && target_dir.exists() {
            let _ = fs::remove_dir_all(&target_dir);
        }

        if target_dir.exists() {
            let status = Command::new("git")
                .arg("pull")
                .current_dir(&target_dir)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .context("Failed to run git pull")?;

            if !status.success() {
                let _ = fs::remove_dir_all(&target_dir);
                self.clone_repo(pkg_name, &target_dir)?;
            }
        } else {
            self.clone_repo(pkg_name, &target_dir)?;
        }

        Ok(target_dir)
    }

    fn clone_repo(&self, pkg_name: &str, target_dir: &Path) -> Result<()> {
        let url = format!("https://aur.archlinux.org/{}.git", pkg_name);
        let status = Command::new("git")
            .args(["clone", "--depth", "1", &url])
            .arg(target_dir)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .context("Failed to clone git repository")?;

        if !status.success() {
            anyhow::bail!("git clone failed for {}", pkg_name);
        }
        Ok(())
    }

    pub fn build<F>(
        &self,
        target_dir: &Path,
        child_pgid: &Arc<AtomicI32>,
        cancel: &Arc<AtomicBool>,
        mut on_line: F,
    ) -> Result<PathBuf>
    where
        F: FnMut(&str),
    {
        let mut cmd = Command::new("makepkg");
        cmd.args(["-sc", "--noconfirm"])
            .current_dir(target_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .process_group(0);

        let mut child = cmd.spawn().context("Failed to spawn makepkg")?;
        let pgid = child.id() as i32;
        child_pgid.store(pgid, Ordering::SeqCst);

        let stdout = child.stdout.take().context("Failed to capture stdout")?;
        let reader = BufReader::new(stdout);

        for line in reader.lines().map_while(Result::ok) {
            if cancel.load(Ordering::Relaxed) {
                unsafe { libc::kill(-pgid, libc::SIGTERM); }
                anyhow::bail!("Build aborted by user");
            }
            on_line(&line);
        }

        let status = child.wait().context("makepkg process failed")?;
        child_pgid.store(0, Ordering::SeqCst);

        if cancel.load(Ordering::Relaxed) {
            anyhow::bail!("Build aborted by user");
        }

        if !status.success() {
            anyhow::bail!("makepkg exited with error status {:?}", status.code());
        }

        self.find_built_artifact(target_dir)
    }

    fn find_built_artifact(&self, dir: &Path) -> Result<PathBuf> {
        for entry in fs::read_dir(dir)?.flatten() {
            let path = entry.path();
            if let Some(fname) = path.file_name().and_then(|n| n.to_str()) {
                if (fname.ends_with(".pkg.tar.zst") || fname.ends_with(".pkg.tar.xz"))
                    && !fname.contains("-debug-")
                {
                    return Ok(path);
                }
            }
        }
        anyhow::bail!("Build succeeded, but no package archive was found in {:?}", dir);
    }

    fn parse_makepkg_stage(line: &str) -> Option<(&'static str, u16)> {
        if line.contains("Retrieving sources") || line.contains("Cloning") {
            Some(("Downloading sources...", 20))
        } else if line.contains("Validating source") || line.contains("Verifying source") {
            Some(("Verifying source checksums & PGP keys...", 35))
        } else if line.contains("Extracting sources") {
            Some(("Extracting source archives...", 45))
        } else if line.contains("Starting prepare") {
            Some(("Preparing build & applying patches...", 50))
        } else if line.contains("Starting build") {
            Some(("Compiling C/C++ source code...", 60))
        } else if line.contains("Entering fakeroot") {
            Some(("Entering fakeroot environment...", 80))
        } else if line.contains("Starting package") {
            Some(("Packaging files...", 88))
        } else if line.contains("Compressing package") {
            Some(("Compressing package archive...", 94))
        } else if line.contains("Finished making") {
            Some(("Build complete!", 98))
        } else {
            None
        }
    }

    pub fn build_and_integrate<F>(
        &self,
        pkg_name: &str,
        repo: &LocalRepoManager,
        clean: bool,
        child_pgid: &Arc<AtomicI32>,
        cancel: &Arc<AtomicBool>,
        mut on_status: F,
    ) -> Result<PathBuf>
    where
        F: FnMut(&str, u16),
    {
        on_status("Fetching AUR git repository...", 10);
        let target_dir = self.fetch_source(pkg_name, clean, cancel)?;

        on_status("Starting makepkg...", 15);
        let mut current_percent = 15u16;

        let artifact = self.build(&target_dir, child_pgid, cancel, |line| {
            if let Some((stage_name, pct)) = Self::parse_makepkg_stage(line) {
                current_percent = pct;
                on_status(stage_name, pct);
            } else if line.starts_with("  -> ") {
                let sub = line.trim_start_matches("  -> ");
                on_status(sub, current_percent);
            }
        })?;

        if cancel.load(Ordering::Relaxed) {
            anyhow::bail!("Build aborted by user");
        }

        on_status("Moving artifact to local repo...", 92);
        let dest_file = repo.path.join(artifact.file_name().unwrap());
        fs::copy(&artifact, &dest_file).context("Failed to copy built package to repository")?;

        on_status("Registering package in repo database (repo-add)...", 96);
        repo.add_package(&dest_file)?;

        on_status("Complete!", 100);
        Ok(dest_file)
    }
}
