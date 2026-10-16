use alpm::{Alpm, SigLevel, TransFlag};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::io::{self, Write};

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type")]
pub enum WorkerEvent {
    Progress {
        title: String,
        target: String,
        step: String,
        percent: u16,
    },
    Done,
    Error { message: String },
}

fn emit(event: WorkerEvent) {
    if let Ok(json) = serde_json::to_string(&event) {
        println!("{}", json);
        let _ = io::stdout().flush();
    }
}

pub fn run_worker(pkg_paths: Vec<String>) -> Result<()> {
    if unsafe { libc::geteuid() != 0 } {
        emit(WorkerEvent::Error {
            message: "Worker must be executed with root privileges.".into(),
        });
        std::process::exit(1);
    }

    let pacconf = pacmanconf::Config::new().unwrap_or_default();
    let mut alpm = match Alpm::new(pacconf.root_dir.as_str(), pacconf.db_path.as_str()) {
        Ok(a) => a,
        Err(e) => {
            emit(WorkerEvent::Error {
                message: format!("ALPM Init Failed: {e}"),
            });
            std::process::exit(1);
        }
    };

    alpm.set_progress_cb((), move |_progress, pkgname, percent, howmany, current, _| {
        let overall = if howmany > 0 {
            ((current.saturating_sub(1) * 100 + percent.max(0) as usize) / howmany).min(100) as u16
        } else {
            percent.max(0).min(100) as u16
        };

        emit(WorkerEvent::Progress {
            title: "Installing to System".to_string(),
            target: format!("{pkgname} ({current}/{howmany})"),
            step: "Extracting package files...".to_string(),
            percent: overall,
        });
    });

    if let Err(e) = alpm.trans_init(TransFlag::empty()) {
        emit(WorkerEvent::Error {
            message: format!("Transaction Init Failed: {e}"),
        });
        std::process::exit(1);
    }

    for path in &pkg_paths {
        let pkg = match alpm.pkg_load(path.as_str(), true, SigLevel::NONE) {
            Ok(p) => p,
            Err(err) => {
                emit(WorkerEvent::Error {
                    message: format!("Failed to load {path}: {err:?}"),
                });
                std::process::exit(1);
            }
        };

        if let Err(err) = alpm.trans_add_pkg(pkg) {
            emit(WorkerEvent::Error {
                message: format!("Failed to add {path}: {err:?}"),
            });
            std::process::exit(1);
        }
    }

    emit(WorkerEvent::Progress {
        title: "Installing to System".to_string(),
        target: "Dependencies".to_string(),
        step: "Resolving dependencies & checking conflicts...".to_string(),
        percent: 25,
    });

    let prep_res = alpm.trans_prepare().map_err(|e| format!("{e:?}"));
    if let Err(err_msg) = prep_res {
        let _ = alpm.trans_release();
        emit(WorkerEvent::Error {
            message: format!("Transaction prepare failed: {err_msg}"),
        });
        std::process::exit(1);
    }

    emit(WorkerEvent::Progress {
        title: "Installing to System".to_string(),
        target: "Disk".to_string(),
        step: "Writing package files to root...".to_string(),
        percent: 50,
    });

    let commit_res = alpm.trans_commit().map_err(|e| format!("{e:?}"));
    if let Err(err_msg) = commit_res {
        let _ = alpm.trans_release();
        emit(WorkerEvent::Error {
            message: format!("Transaction commit failed: {err_msg}"),
        });
        std::process::exit(1);
    }

    let _ = alpm.trans_release();
    emit(WorkerEvent::Done);
    Ok(())
}

/// Elevated worker for package removal with recursive unneeded cascade (-Rns)
pub fn run_worker_remove(pkg_names: Vec<String>) -> Result<()> {
    if unsafe { libc::geteuid() != 0 } {
        emit(WorkerEvent::Error {
            message: "Worker must be executed with root privileges.".into(),
        });
        std::process::exit(1);
    }

    let pacconf = pacmanconf::Config::new().unwrap_or_default();
    let mut alpm = match Alpm::new(pacconf.root_dir.as_str(), pacconf.db_path.as_str()) {
        Ok(a) => a,
        Err(e) => {
            emit(WorkerEvent::Error {
                message: format!("ALPM Init Failed: {e}"),
            });
            std::process::exit(1);
        }
    };

    alpm.set_progress_cb((), move |_progress, pkgname, percent, howmany, current, _| {
        let overall = if howmany > 0 {
            ((current.saturating_sub(1) * 100 + percent.max(0) as usize) / howmany).min(100) as u16
        } else {
            percent.max(0).min(100) as u16
        };

        emit(WorkerEvent::Progress {
            title: "Uninstalling Package".to_string(),
            target: format!("{pkgname} ({current}/{howmany})"),
            step: "Removing package files (-Rns)...".to_string(),
            percent: overall,
        });
    });

    if let Err(e) = alpm.trans_init(TransFlag::RECURSE | TransFlag::UNNEEDED) {
        emit(WorkerEvent::Error {
            message: format!("Transaction Init Failed: {e}"),
        });
        std::process::exit(1);
    }

    for name in &pkg_names {
        if let Ok(pkg) = alpm.localdb().pkg(name.as_str()) {
            if let Err(e) = alpm.trans_remove_pkg(pkg) {
                let _ = alpm.trans_release();
                emit(WorkerEvent::Error {
                    message: format!("Failed to remove {name}: {e:?}"),
                });
                std::process::exit(1);
            }
        }
    }

    emit(WorkerEvent::Progress {
        title: "Uninstalling Package".to_string(),
        target: "Dependencies".to_string(),
        step: "Resolving cascade dependencies (-Rns)...".to_string(),
        percent: 30,
    });

    let prep_res = alpm.trans_prepare().map_err(|e| format!("{e:?}"));
    if let Err(err_msg) = prep_res {
        let _ = alpm.trans_release();
        emit(WorkerEvent::Error {
            message: format!("Transaction prepare failed: {err_msg}"),
        });
        std::process::exit(1);
    }

    emit(WorkerEvent::Progress {
        title: "Uninstalling Package".to_string(),
        target: "Disk".to_string(),
        step: "Removing files and running hooks...".to_string(),
        percent: 60,
    });

    let commit_res = alpm.trans_commit().map_err(|e| format!("{e:?}"));
    if let Err(err_msg) = commit_res {
        let _ = alpm.trans_release();
        emit(WorkerEvent::Error {
            message: format!("Transaction commit failed: {err_msg}"),
        });
        std::process::exit(1);
    }

    let _ = alpm.trans_release();
    emit(WorkerEvent::Done);
    Ok(())
}
