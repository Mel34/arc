use crate::aur::{AurClient, AurPackage};
use crate::builder::PackageBuilder;
use crate::repo::{LocalRepoManager, RepoPackage};
use crate::ui::dialogs::{ConfirmAction, ConfirmationDialog, ProgressState};
use crate::worker::WorkerEvent;
use alpm::Alpm;
use anyhow::Result;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::collections::HashSet;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

#[derive(PartialEq, Clone, Copy)]
pub enum ActivePane {
    Left,
    Right,
}

#[allow(dead_code)]
pub enum Modal {
    Confirm(ConfirmationDialog),
    Help,
    Message(String),
}

pub struct App {
    pub alpm: Alpm,
    pub active_pane: ActivePane,
    pub repo_mgr: LocalRepoManager,
    pub aur_client: AurClient,
    pub builder: PackageBuilder,

    // Left Pane
    pub local_pkgs: Vec<RepoPackage>,
    pub local_cursor: usize,
    pub local_filter: String,
    pub local_tagged: HashSet<String>,
    pub locked_pkgs: HashSet<String>,

    // Right Pane
    pub aur_results: Vec<AurPackage>,
    pub aur_cursor: usize,
    pub aur_search_query: String,
    pub aur_tagged: HashSet<String>,
    pub search_debounce: Option<Instant>,

    // Quick View
    pub quick_view: bool,
    pub quick_view_title: String,
    pub quick_view_content: Vec<String>,
    pub quick_view_scroll: usize,

    // Modals & Progress
    pub modal: Option<Modal>,
    pub progress: Option<ProgressState>,
    pub should_quit: bool,
}

impl App {
    pub fn new() -> Result<Self> {
        let pacconf = pacmanconf::Config::new().unwrap_or_default();
        let alpm = Alpm::new(pacconf.root_dir.as_str(), pacconf.db_path.as_str())?;
        let repo_mgr = LocalRepoManager::new()?;
        let aur_client = AurClient::new();
        let builder = PackageBuilder::new()?;
        let locked_pkgs = Self::load_locked_packages();

        let mut app = Self {
            alpm,
            active_pane: ActivePane::Left,
            repo_mgr,
            aur_client,
            builder,
            local_pkgs: Vec::new(),
            local_cursor: 0,
            local_filter: String::new(),
            local_tagged: HashSet::new(),
            locked_pkgs,
            aur_results: Vec::new(),
            aur_cursor: 0,
            aur_search_query: String::new(),
            aur_tagged: HashSet::new(),
            search_debounce: None,
            quick_view: false,
            quick_view_title: String::new(),
            quick_view_content: Vec::new(),
            quick_view_scroll: 0,
            modal: None,
            progress: None,
            should_quit: false,
        };

        app.refresh_local()?;
        Ok(app)
    }

    fn lock_file_path() -> PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        PathBuf::from(home).join(".config").join("arc").join("ignore")
    }

    fn load_locked_packages() -> HashSet<String> {
        let mut locked = HashSet::new();
        let path = Self::lock_file_path();
        if let Ok(content) = fs::read_to_string(&path) {
            for line in content.lines() {
                let trimmed = line.trim();
                if !trimmed.is_empty() && !trimmed.starts_with('#') {
                    for pkg in trimmed.split_whitespace() {
                        locked.insert(pkg.to_string());
                    }
                }
            }
        }
        if let Ok(content) = fs::read_to_string("/etc/pacman.d/IgnorePkg") {
            for line in content.lines() {
                let trimmed = line.trim();
                if let Some((_, val)) = trimmed.split_once('=') {
                    for pkg in val.split_whitespace() {
                        locked.insert(pkg.to_string());
                    }
                }
            }
        }
        locked
    }

    pub fn save_locked_packages(&self) -> Result<()> {
        let path = Self::lock_file_path();
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let mut list: Vec<&str> = self.locked_pkgs.iter().map(|s| s.as_str()).collect();
        list.sort();
        let content = format!("# Managed by ARC\n{}\n", list.join("\n"));
        fs::write(path, content)?;
        Ok(())
    }

    pub fn toggle_lock_current(&mut self) -> Result<()> {
        let targets: Vec<String> = if !self.local_tagged.is_empty() {
            self.local_tagged.iter().cloned().collect()
        } else if let Some(pkg) = self.filtered_local_pkgs().get(self.local_cursor) {
            vec![pkg.name.clone()]
        } else {
            return Ok(());
        };

        let any_unlocked = targets.iter().any(|t| !self.locked_pkgs.contains(t));
        for t in &targets {
            if any_unlocked {
                self.locked_pkgs.insert(t.clone());
                if let Some(p) = self.local_pkgs.iter_mut().find(|p| &p.name == t) {
                    p.aur_update = None;
                }
            } else {
                self.locked_pkgs.remove(t);
            }
        }

        self.save_locked_packages()?;
        self.local_tagged.clear();
        Ok(())
    }

    pub fn refresh_local(&mut self) -> Result<()> {
        let pacconf = pacmanconf::Config::new().unwrap_or_default();
        if let Ok(fresh_alpm) = Alpm::new(pacconf.root_dir.as_str(), pacconf.db_path.as_str()) {
            self.alpm = fresh_alpm;
        }

        self.local_pkgs = self.repo_mgr.list_packages(&self.alpm)?;
        self.local_tagged.clear();
        if self.local_cursor >= self.local_pkgs.len() && !self.local_pkgs.is_empty() {
            self.local_cursor = self.local_pkgs.len() - 1;
        }
        Ok(())
    }

    pub fn filtered_local_pkgs(&self) -> Vec<&RepoPackage> {
        if self.local_filter.is_empty() {
            self.local_pkgs.iter().collect()
        } else {
            let f = self.local_filter.to_lowercase();
            self.local_pkgs
                .iter()
                .filter(|p| p.name.to_lowercase().contains(&f))
                .collect()
        }
    }

    pub fn trigger_aur_search(&mut self) {
        if self.aur_search_query.trim().is_empty() {
            self.aur_results.clear();
            self.aur_cursor = 0;
            return;
        }
        match self.aur_client.search(&self.aur_search_query) {
            Ok(results) => {
                self.aur_results = results;
                self.aur_cursor = 0;
                if self.quick_view {
                    self.update_quick_view();
                }
            }
            Err(e) => {
                self.modal = Some(Modal::Message(format!("AUR Search Error: {e}")));
            }
        }
    }

    pub fn check_aur_updates(&mut self) {
        let local_tuples: Vec<(&str, &str)> = self
            .local_pkgs
            .iter()
            .filter(|p| !self.locked_pkgs.contains(&p.name))
            .map(|p| (p.name.as_str(), p.version.as_str()))
            .collect();

        match self.aur_client.check_updates(&local_tuples) {
            Ok(updates) => {
                for (name, new_ver) in updates {
                    if let Some(pkg) = self.local_pkgs.iter_mut().find(|p| p.name == name) {
                        pkg.aur_update = Some(new_ver);
                    }
                }
            }
            Err(e) => {
                self.modal = Some(Modal::Message(format!("Update check failed: {e}")));
            }
        }
    }

    pub fn prompt_update_all(&mut self) {
        if !self.local_pkgs.iter().any(|p| p.aur_update.is_some()) {
            self.check_aur_updates();
        }

        let to_update: Vec<String> = self
            .local_pkgs
            .iter()
            .filter(|p| p.aur_update.is_some() && !self.locked_pkgs.contains(&p.name))
            .map(|p| p.name.clone())
            .collect();

        if to_update.is_empty() {
            self.modal = Some(Modal::Message("All packages are up to date! No updates available.".to_string()));
            return;
        }

        self.modal = Some(Modal::Confirm(ConfirmationDialog {
            title: "Update All Packages".to_string(),
            prompt: format!("Build and update {} package(s) from AUR?", to_update.len()),
            targets: to_update.clone(),
            cascade: Vec::new(),
            total_size: 0,
            selected_button: 0,
            button_labels: vec!["Update All".to_string(), "Cancel".to_string()],
            action: ConfirmAction::UpdateAll(to_update),
        }));
    }

    pub fn get_unneeded_cascade(&self, targets: &[String]) -> (Vec<String>, i64) {
        let localdb = self.alpm.localdb();
        let mut removal_set: HashSet<String> = targets.iter().cloned().collect();
        let mut changed = true;

        while changed {
            changed = false;
            for pkg in localdb.pkgs() {
                let name = pkg.name().to_string();
                if removal_set.contains(&name) {
                    continue;
                }

                let is_needed = localdb.pkgs().into_iter().any(|other| {
                    let other_name = other.name();
                    if removal_set.contains(other_name) || other_name == name {
                        return false;
                    }
                    other.depends().into_iter().any(|d| d.name() == name)
                });

                if !is_needed {
                    let is_dep_of_removal = removal_set.iter().any(|target| {
                        if let Ok(t_pkg) = localdb.pkg(target.as_str()) {
                            t_pkg.depends().into_iter().any(|d| d.name() == name)
                        } else {
                            false
                        }
                    });

                    if is_dep_of_removal {
                        removal_set.insert(name);
                        changed = true;
                    }
                }
            }
        }

        let mut cascade = Vec::new();
        let mut total_size = 0i64;

        for name in &removal_set {
            if let Ok(pkg) = localdb.pkg(name.as_str()) {
                total_size += pkg.isize();
            }
            if !targets.contains(name) {
                cascade.push(name.clone());
            }
        }
        cascade.sort();
        (cascade, total_size)
    }

    pub fn get_repo_cascade(&self, targets: &[String]) -> (Vec<String>, i64) {
        let mut to_delete: HashSet<String> = targets.iter().cloned().collect();
        let mut changed = true;

        while changed {
            changed = false;
            for p in &self.local_pkgs {
                if to_delete.contains(&p.name) {
                    continue;
                }

                let is_dep_of_delete = self.local_pkgs.iter().any(|other| {
                    to_delete.contains(&other.name) && other.depends.contains(&p.name)
                });

                if is_dep_of_delete {
                    let needed_by_remaining = self.local_pkgs.iter().any(|other| {
                        !to_delete.contains(&other.name) && other.depends.contains(&p.name)
                    });

                    if !needed_by_remaining {
                        to_delete.insert(p.name.clone());
                        changed = true;
                    }
                }
            }
        }

        let mut cascade = Vec::new();
        let mut total_size = 0i64;

        for name in &to_delete {
            if let Some(pkg) = self.local_pkgs.iter().find(|p| &p.name == name) {
                total_size += pkg.size;
            }
            if !targets.contains(name) {
                cascade.push(name.clone());
            }
        }
        cascade.sort();
        (cascade, total_size)
    }

    pub fn prompt_clean_cache(&mut self) {
        match self.repo_mgr.scan_old_cache(&self.alpm) {
            Ok((count, bytes, files)) => {
                if count == 0 {
                    self.modal = Some(Modal::Message("Cache is already clean! No older package versions found.".to_string()));
                } else {
                    let mib = bytes as f64 / (1024.0 * 1024.0);
                    self.modal = Some(Modal::Confirm(ConfirmationDialog {
                        title: "Clean Package Cache".to_string(),
                        prompt: format!("Found {} older archive(s) taking {:.2} MiB.\nPurge them from {} cache?", count, mib, self.repo_mgr.name),
                        targets: files.iter().filter_map(|p| p.file_name().and_then(|n| n.to_str()).map(|s| s.to_string())).collect(),
                        cascade: Vec::new(),
                        total_size: bytes as i64,
                        selected_button: 0,
                        button_labels: vec!["Purge".to_string(), "Cancel".to_string()],
                        action: ConfirmAction::CleanCache(files),
                    }));
                }
            }
            Err(e) => {
                self.modal = Some(Modal::Message(format!("Scan Error: {e}")));
            }
        }
    }

    pub fn update_quick_view(&mut self) {
        let name = match self.active_pane {
            ActivePane::Left => self.filtered_local_pkgs().get(self.local_cursor).map(|p| p.name.clone()),
            ActivePane::Right => self.aur_results.get(self.aur_cursor).map(|p| p.name.clone()),
        };

        if let Some(n) = name {
            self.quick_view_title = n.clone();
            self.quick_view_scroll = 0;
            match self.aur_client.fetch_pkgbuild(&n) {
                Ok(content) => {
                    self.quick_view_content = content.lines().map(|s| s.to_string()).collect();
                }
                Err(e) => {
                    self.quick_view_content = vec![format!("Failed to load PKGBUILD: {e}")];
                }
            }
        }
    }

    pub fn toggle_quick_view(&mut self) {
        self.quick_view = !self.quick_view;
        if self.quick_view {
            self.update_quick_view();
        }
    }

    pub fn build_and_integrate_package(
        &mut self,
        pkg_name: &str,
        clean: bool,
        terminal: &mut Terminal<CrosstermBackend<std::fs::File>>,
    ) -> Result<PathBuf> {
        use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
        use std::sync::mpsc;
        use std::sync::Arc;
        use std::time::Duration;
        use crossterm::event::{self, Event, KeyCode, KeyEventKind};

        enum BuildMsg {
            Status(String, u16),
            Done(Result<PathBuf, String>),
        }

        let name = pkg_name.to_string();
        let builder = self.builder.clone();
        let repo_mgr = self.repo_mgr.clone();

        let child_pgid = Arc::new(AtomicI32::new(0));
        let cancel = Arc::new(AtomicBool::new(false));

        let child_pgid_clone = child_pgid.clone();
        let cancel_clone = cancel.clone();
        let name_clone = name.clone();

        let (tx, rx) = mpsc::channel::<BuildMsg>();

        std::thread::spawn(move || {
            let res = builder.build_and_integrate(
                &name_clone,
                &repo_mgr,
                clean,
                &child_pgid_clone,
                &cancel_clone,
                |status, pct| {
                    let _ = tx.send(BuildMsg::Status(status.to_string(), pct));
                },
            );

            match res {
                Ok(artifact) => { let _ = tx.send(BuildMsg::Done(Ok(artifact))); }
                Err(e) => { let _ = tx.send(BuildMsg::Done(Err(e.to_string()))); }
            }
        });

        self.progress = Some(ProgressState {
            title: "Building Package".to_string(),
            target: name.clone(),
            step: "Initializing build...".to_string(),
            percent: 5,
        });
        terminal.draw(|f| crate::ui::draw_ui(f, self))?;

        loop {
            if event::poll(Duration::from_millis(40))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind == KeyEventKind::Press && key.code == KeyCode::Esc {
                        cancel.store(true, Ordering::SeqCst);
                        let pgid = child_pgid.load(Ordering::SeqCst);
                        if pgid > 1 {
                            unsafe { libc::kill(-pgid, libc::SIGTERM); }
                        }
                        self.progress = None;
                        anyhow::bail!("Build aborted by user (ESC)");
                    }
                }
            }

            while let Ok(msg) = rx.try_recv() {
                match msg {
                    BuildMsg::Status(status, pct) => {
                        self.progress = Some(ProgressState {
                            title: "Building Package".to_string(),
                            target: name.clone(),
                            step: status,
                            percent: pct,
                        });
                        terminal.draw(|f| crate::ui::draw_ui(f, self))?;
                    }
                    BuildMsg::Done(result) => {
                        self.progress = None;
                        match result {
                            Ok(artifact) => {
                                self.refresh_local()?;
                                return Ok(artifact);
                            }
                            Err(e) => {
                                anyhow::bail!("{e}");
                            }
                        }
                    }
                }
            }
        }
    }

    pub fn install_archive_via_worker(
        &mut self,
        pkg_path: &Path,
        terminal: &mut Terminal<CrosstermBackend<std::fs::File>>,
    ) -> Result<()> {
        let current_exe = std::env::current_exe()?;
        let mut child = Command::new("sudo")
            .arg(current_exe)
            .arg("--worker-commit")
            .arg(pkg_path)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;

        let stdout = child.stdout.take().unwrap();
        let reader = BufReader::new(stdout);

        for line in reader.lines().map_while(Result::ok) {
            if let Ok(event) = serde_json::from_str::<WorkerEvent>(&line) {
                match event {
                    WorkerEvent::Progress {
                        title,
                        target,
                        step,
                        percent,
                    } => {
                        self.progress = Some(ProgressState {
                            title,
                            target,
                            step,
                            percent,
                        });
                        let _ = terminal.draw(|f| crate::ui::draw_ui(f, self));
                    }
                    WorkerEvent::Done => break,
                    WorkerEvent::Error { message } => {
                        self.progress = None;
                        anyhow::bail!("{message}");
                    }
                }
            }
        }

        let _ = child.wait()?;
        self.progress = None;
        self.refresh_local()?;
        Ok(())
    }

    pub fn remove_packages_via_worker(
        &mut self,
        targets: &[String],
        terminal: &mut Terminal<CrosstermBackend<std::fs::File>>,
    ) -> Result<()> {
        let current_exe = std::env::current_exe()?;
        let mut cmd = Command::new("sudo");
        cmd.arg(current_exe).arg("--worker-remove");
        for t in targets {
            cmd.arg(t);
        }
        cmd.stdout(Stdio::piped()).stderr(Stdio::null());

        let mut child = cmd.spawn()?;
        let stdout = child.stdout.take().unwrap();
        let reader = BufReader::new(stdout);

        for line in reader.lines().map_while(Result::ok) {
            if let Ok(event) = serde_json::from_str::<WorkerEvent>(&line) {
                match event {
                    WorkerEvent::Progress {
                        title,
                        target,
                        step,
                        percent,
                    } => {
                        self.progress = Some(ProgressState {
                            title,
                            target,
                            step,
                            percent,
                        });
                        let _ = terminal.draw(|f| crate::ui::draw_ui(f, self));
                    }
                    WorkerEvent::Done => break,
                    WorkerEvent::Error { message } => {
                        self.progress = None;
                        anyhow::bail!("{message}");
                    }
                }
            }
        }

        let _ = child.wait()?;
        self.progress = None;
        self.refresh_local()?;
        Ok(())
    }
}
