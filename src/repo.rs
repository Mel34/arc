use anyhow::{Context, Result};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Clone, Debug)]
pub struct RepoPackage {
    pub name: String,
    pub version: String,
    pub file_path: PathBuf,
    pub size: i64,
    pub is_installed: bool,
    pub aur_update: Option<String>,
    pub depends: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct LocalRepoManager {
    pub name: String,
    pub path: PathBuf,
    pub db_path: PathBuf,
}

impl LocalRepoManager {
    pub fn new() -> Result<Self> {
        let hostname = fs::read_to_string("/etc/hostname")
            .unwrap_or_else(|_| "custom".to_string())
            .trim()
            .to_string();

        let path = PathBuf::from(format!("/var/cache/pacman/{}", hostname));
        let mut db_path = path.join(format!("{}.db.tar.zst", hostname));
        if !db_path.exists() {
            let gz_path = path.join(format!("{}.db.tar.gz", hostname));
            if gz_path.exists() {
                db_path = gz_path;
            }
        }

        Ok(Self {
            name: hostname,
            path,
            db_path,
        })
    }

    pub fn list_packages(&self, alpm: &alpm::Alpm) -> Result<Vec<RepoPackage>> {
        let mut latest_map: HashMap<String, RepoPackage> = HashMap::new();
        if !self.path.exists() {
            return Ok(Vec::new());
        }

        let local_db = alpm.localdb();

        for entry in fs::read_dir(&self.path)?.flatten() {
            let path = entry.path();
            let fname = match path.file_name().and_then(|n| n.to_str()) {
                Some(f) => f,
                None => continue,
            };

            if fname.ends_with(".pkg.tar.zst") || fname.ends_with(".pkg.tar.xz") {
                if let Ok(pkg) = alpm.pkg_load(path.to_str().unwrap(), false, alpm::SigLevel::NONE) {
                    let name = pkg.name().to_string();
                    let ver = pkg.version().as_str().to_string();
                    let is_installed = local_db.pkg(name.as_str()).is_ok();
                    let depends: Vec<String> = pkg.depends().into_iter().map(|d| d.name().to_string()).collect();

                    let candidate = RepoPackage {
                        name: name.clone(),
                        version: ver.clone(),
                        file_path: path.clone(),
                        size: pkg.isize(),
                        is_installed,
                        aur_update: None,
                        depends,
                    };

                    match latest_map.entry(name) {
                        std::collections::hash_map::Entry::Vacant(e) => {
                            e.insert(candidate);
                        }
                        std::collections::hash_map::Entry::Occupied(mut e) => {
                            if alpm::vercmp(ver.as_str(), e.get().version.as_str()).is_gt() {
                                e.insert(candidate);
                            }
                        }
                    }
                }
            }
        }

        let mut pkgs: Vec<RepoPackage> = latest_map.into_values().collect();
        pkgs.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(pkgs)
    }

    pub fn scan_old_cache(&self, alpm: &alpm::Alpm) -> Result<(usize, u64, Vec<PathBuf>)> {
        let mut grouped: HashMap<String, Vec<(String, PathBuf, u64)>> = HashMap::new();
        if !self.path.exists() {
            return Ok((0, 0, Vec::new()));
        }

        for entry in fs::read_dir(&self.path)?.flatten() {
            let path = entry.path();
            let fname = match path.file_name().and_then(|n| n.to_str()) {
                Some(f) => f,
                None => continue,
            };

            if fname.ends_with(".pkg.tar.zst") || fname.ends_with(".pkg.tar.xz") {
                if let Ok(pkg) = alpm.pkg_load(path.to_str().unwrap(), false, alpm::SigLevel::NONE) {
                    let name = pkg.name().to_string();
                    let ver = pkg.version().as_str().to_string();
                    let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                    grouped.entry(name).or_default().push((ver, path, size));
                }
            }
        }

        let mut old_files = Vec::new();
        let mut total_bytes = 0u64;

        for (_name, mut versions) in grouped {
            if versions.len() <= 1 {
                continue;
            }
            versions.sort_by(|a, b| alpm::vercmp(b.0.as_str(), a.0.as_str()));
            for (_old_ver, old_path, size) in &versions[1..] {
                old_files.push(old_path.clone());
                total_bytes += size;
            }
        }

        Ok((old_files.len(), total_bytes, old_files))
    }

    pub fn purge_cache_files(&self, files: &[PathBuf]) -> (usize, u64) {
        let mut count = 0;
        let mut bytes = 0;
        for path in files {
            if let Ok(meta) = fs::metadata(path) {
                let size = meta.len();
                if fs::remove_file(path).is_ok() {
                    count += 1;
                    bytes += size;
                }
            }
        }
        (count, bytes)
    }

    pub fn add_package(&self, pkg_path: &Path) -> Result<()> {
        let status = Command::new("repo-add")
            .arg("-n")
            .arg("-R")
            .arg(&self.db_path)
            .arg(pkg_path)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .context("Failed to run repo-add")?;

        if !status.success() {
            anyhow::bail!("repo-add exited with status {:?}", status.code());
        }
        Ok(())
    }

    pub fn remove_package(&self, pkg_name: &str) -> Result<()> {
        let _ = Command::new("repo-remove")
            .arg(&self.db_path)
            .arg(pkg_name)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();

        let prefix = format!("{}-", pkg_name);
        if let Ok(entries) = fs::read_dir(&self.path) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with(&prefix) && name.contains(".pkg.tar.") {
                    let _ = fs::remove_file(entry.path());
                }
            }
        }
        Ok(())
    }
}
