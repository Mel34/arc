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

    /// Resolve source:
    /// Builds ALWAYS happen in ~/.cache/arc/builds/<pkgname> sandbox.
    /// ~/Projects/ is strictly read-only (only copying the PKGBUILD).
    pub fn get_or_fetch_source(
        &self,
        pkg_name: &str,
        is_locked: bool,
        clean: bool,
        cancel: &Arc<AtomicBool>,
    ) -> Result<PathBuf> {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        let proj_dir = PathBuf::from(&home).join("Projects").join(pkg_name);
        let sandbox_dir = self.build_dir.join(pkg_name);

        if cancel.load(Ordering::Relaxed) {
            anyhow::bail!("Build aborted by user");
        }

        // --- LOCKED / LOCAL PACKAGES ---
        if is_locked {
            // Ensure sandbox directory exists
            fs::create_dir_all(&sandbox_dir)?;

            // If a local project PKGBUILD exists, copy ONLY the PKGBUILD into the sandbox
            let proj_pkgbuild = proj_dir.join("PKGBUILD");
            if proj_pkgbuild.exists() {
                fs::copy(&proj_pkgbuild, sandbox_dir.join("PKGBUILD"))
                    .context("Failed to copy PKGBUILD to build sandbox")?;
            } else if !sandbox_dir.join("PKGBUILD").exists() {
                anyhow::bail!(
                    "Cannot rebuild locked package '{}':\n\nNo PKGBUILD found in ~/Projects/{}/PKGBUILD or ~/.cache/arc/builds/{}/PKGBUILD.\n\nPlease ensure a PKGBUILD exists.",
                    pkg_name, pkg_name, pkg_name
                );
            }

            // Safe sandbox cleaning: only purge inside ~/.cache/arc/builds/<pkgname>/
            if clean && sandbox_dir.exists() {
                let _ = fs::remove_dir_all(sandbox_dir.join("src"));
                let _ = fs::remove_dir_all(sandbox_dir.join("pkg"));
                if let Ok(entries) = fs::read_dir(&sandbox_dir) {
                    for entry in entries.flatten() {
                        let fname = entry.file_name().to_string_lossy().to_string();
                        if fname.contains(".pkg.tar.") {
                            let _ = fs::remove_file(entry.path());
                        }
                    }
                }
            }

            return Ok(sandbox_dir);
        }

        // --- STANDARD AUR PACKAGES ---
        let target_dir = sandbox_dir;
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
        cmd.args(["-scf", "--noconfirm"])
            .current_dir(target_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0);

        let mut child = cmd.spawn().context("Failed to spawn makepkg")?;
        let pgid = child.id() as i32;
        child_pgid.store(pgid, Ordering::SeqCst);

        let stdout = child.stdout.take().context("Failed to capture stdout")?;
        let stderr = child.stderr.take().context("Failed to capture stderr")?;

        let mut error_lines: Vec<String> = Vec::new();

        let stderr_handle = std::thread::spawn(move || {
            let mut errs = Vec::new();
            let reader = BufReader::new(stderr);
            for line in reader.lines().map_while(Result::ok) {
                if errs.len() > 15 {
                    errs.remove(0);
                }
                errs.push(line);
            }
            errs
        });

        let reader = BufReader::new(stdout);
        for line in reader.lines().map_while(Result::ok) {
            if cancel.load(Ordering::Relaxed) {
                unsafe { libc::kill(-pgid, libc::SIGTERM); }
                anyhow::bail!("Build aborted by user (ESC)");
            }
            on_line(&line);
        }

        let status = child.wait().context("makepkg process failed")?;
        child_pgid.store(0, Ordering::SeqCst);

        if let Ok(errs) = stderr_handle.join() {
            error_lines = errs;
        }

        if cancel.load(Ordering::Relaxed) {
            anyhow::bail!("Build aborted by user (ESC)");
        }

        if !status.success() {
            let detail = if !error_lines.is_empty() {
                format!("\n\nError output:\n{}", error_lines.join("\n"))
            } else {
                String::new()
            };
            anyhow::bail!("makepkg failed (exit code {:?}){}", status.code(), detail);
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
        is_locked: bool,
        clean: bool,
        child_pgid: &Arc<AtomicI32>,
        cancel: &Arc<AtomicBool>,
        mut on_status: F,
    ) -> Result<PathBuf>
    where
        F: FnMut(&str, u16),
    {
        on_status("Preparing build sandbox...", 10);
        let target_dir = self.get_or_fetch_source(pkg_name, is_locked, clean, cancel)?;

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
            anyhow::bail!("Build aborted by user (ESC)");
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
