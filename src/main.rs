mod app;
mod aur;
mod builder;
mod repo;
mod theme;
mod ui;
mod worker;

use anyhow::Result;
use app::{ActivePane, App, Modal};
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::env;
use std::time::{Duration, Instant};
use ui::dialogs::{ConfirmAction, ConfirmationDialog};

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();

    if args.len() >= 3 && args[1] == "--worker-commit" {
        let targets = args[2..].to_vec();
        return worker::run_worker(targets);
    }

    if args.len() >= 3 && args[1] == "--worker-remove" {
        let targets = args[2..].to_vec();
        return worker::run_worker_remove(targets);
    }

    let mut app = match App::new() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("Failed to initialize ARC: {e}");
            return Ok(());
        }
    };

    let tty = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")?;

    enable_raw_mode()?;
    let mut tty_clone = tty.try_clone()?;
    execute!(tty_clone, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(tty);
    let mut terminal = Terminal::new(backend)?;

    while !app.should_quit {
        terminal.draw(|f| ui::draw_ui(f, &app))?;

        // Adaptive tiered debounce based on query length:
        // 1 char = 1100ms | 2 chars = 800ms | 3 chars = 500ms | 4+ chars = 280ms
        if let Some(debounce_start) = app.search_debounce {
            let delay = match app.aur_search_query.trim().chars().count() {
                0 => Duration::from_millis(0),
                1 => Duration::from_millis(1100),
                2 => Duration::from_millis(800),
                3 => Duration::from_millis(500),
                _ => Duration::from_millis(280),
            };

            if debounce_start.elapsed() >= delay {
                app.search_debounce = None;
                app.trigger_aur_search();
            }
        }

        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }

                if let Some(mut modal) = app.modal.take() {
                    match modal {
                        Modal::Confirm(ref mut diag) => match key.code {
                            KeyCode::Left | KeyCode::BackTab => {
                                let len = diag.button_labels.len();
                                diag.selected_button = (diag.selected_button + len - 1) % len;
                                app.modal = Some(modal);
                            }
                            KeyCode::Right | KeyCode::Tab => {
                                let len = diag.button_labels.len();
                                diag.selected_button = (diag.selected_button + 1) % len;
                                app.modal = Some(modal);
                            }
                            KeyCode::Enter => {
                                match diag.action {
                                    ConfirmAction::DeleteRepo(ref targets, is_installed) => {
                                        if is_installed {
                                            if diag.selected_button == 0 {
                                                let _ = app.remove_packages_via_worker(targets, &mut terminal);
                                                for t in targets {
                                                    let _ = app.repo_mgr.remove_package(t);
                                                }
                                                let _ = app.refresh_local();
                                            } else if diag.selected_button == 1 {
                                                for t in targets {
                                                    let _ = app.repo_mgr.remove_package(t);
                                                }
                                                let _ = app.refresh_local();
                                            }
                                        } else {
                                            if diag.selected_button == 0 {
                                                for t in targets {
                                                    let _ = app.repo_mgr.remove_package(t);
                                                }
                                                let _ = app.refresh_local();
                                            }
                                        }
                                    }
                                    ConfirmAction::UninstallSystem(ref targets) => {
                                        if diag.selected_button == 0 {
                                            if let Err(e) = app.remove_packages_via_worker(targets, &mut terminal) {
                                                app.modal = Some(Modal::Message(format!("Uninstall Error: {e}")));
                                            }
                                        }
                                    }
                                    ConfirmAction::CleanCache(ref files) => {
                                        if diag.selected_button == 0 {
                                            let (count, bytes) = app.repo_mgr.purge_cache_files(files);
                                            let _ = app.refresh_local();
                                            let mib = bytes as f64 / (1024.0 * 1024.0);
                                            app.modal = Some(Modal::Message(format!(
                                                "Cache Clean Complete!\n\nPurged: {} old archive(s)\nFreed:  {:.2} MiB",
                                                count, mib
                                            )));
                                        }
                                    }
                                    ConfirmAction::UpdateAll(ref targets) => {
                                        if diag.selected_button == 0 {
                                            let mut updated_count = 0;
                                            for name in targets {
                                                let was_installed = app.local_pkgs.iter().find(|p| &p.name == name).map(|p| p.is_installed).unwrap_or(false);
                                                if let Ok(pkg_file) = app.build_and_integrate_package(name, false, &mut terminal) {
                                                    updated_count += 1;
                                                    if was_installed {
                                                        let _ = app.install_archive_via_worker(&pkg_file, &mut terminal);
                                                    }
                                                }
                                            }
                                            let _ = app.refresh_local();
                                            app.modal = Some(Modal::Message(format!("Update All Complete!\n\nSuccessfully updated {} package(s).", updated_count)));
                                        }
                                    }
                                }
                            }
                            KeyCode::Esc => {}
                            _ => {
                                app.modal = Some(modal);
                            }
                        },
                        Modal::Help | Modal::Message(_) => {
                            if key.code != KeyCode::Esc && key.code != KeyCode::Enter {
                                app.modal = Some(modal);
                            }
                        }
                    }
                    continue;
                }

                match key.code {
                    KeyCode::Tab => {
                        app.active_pane = match app.active_pane {
                            ActivePane::Left => ActivePane::Right,
                            ActivePane::Right => ActivePane::Left,
                        };
                        if app.quick_view {
                            app.update_quick_view();
                        }
                    }
                    KeyCode::Home => {
                        if app.active_pane == ActivePane::Left {
                            app.local_cursor = 0;
                        } else {
                            app.aur_cursor = 0;
                        }
                        if app.quick_view {
                            app.update_quick_view();
                        }
                    }
                    KeyCode::End => {
                        if app.active_pane == ActivePane::Left {
                            let count = app.filtered_local_pkgs().len();
                            if count > 0 {
                                app.local_cursor = count - 1;
                            }
                        } else {
                            let count = app.aur_results.len();
                            if count > 0 {
                                app.aur_cursor = count - 1;
                            }
                        }
                        if app.quick_view {
                            app.update_quick_view();
                        }
                    }
                    KeyCode::PageUp => {
                        if app.quick_view {
                            app.quick_view_scroll = app.quick_view_scroll.saturating_sub(15);
                        } else if app.active_pane == ActivePane::Left {
                            app.local_cursor = app.local_cursor.saturating_sub(15);
                        } else {
                            app.aur_cursor = app.aur_cursor.saturating_sub(15);
                        }
                        if app.quick_view {
                            app.update_quick_view();
                        }
                    }
                    KeyCode::PageDown => {
                        if app.quick_view {
                            app.quick_view_scroll = (app.quick_view_scroll + 15).min(app.quick_view_content.len().saturating_sub(5));
                        } else if app.active_pane == ActivePane::Left {
                            let count = app.filtered_local_pkgs().len();
                            if count > 0 {
                                app.local_cursor = (app.local_cursor + 15).min(count - 1);
                            }
                        } else {
                            let count = app.aur_results.len();
                            if count > 0 {
                                app.aur_cursor = (app.aur_cursor + 15).min(count - 1);
                            }
                        }
                        if app.quick_view {
                            app.update_quick_view();
                        }
                    }
                    KeyCode::Up => {
                        if app.active_pane == ActivePane::Left {
                            if app.local_cursor > 0 {
                                app.local_cursor -= 1;
                            }
                        } else if app.aur_cursor > 0 {
                            app.aur_cursor -= 1;
                        }
                        if app.quick_view {
                            app.update_quick_view();
                        }
                    }
                    KeyCode::Down => {
                        if app.active_pane == ActivePane::Left {
                            let count = app.filtered_local_pkgs().len();
                            if count > 0 && app.local_cursor < count - 1 {
                                app.local_cursor += 1;
                            }
                        } else {
                            let count = app.aur_results.len();
                            if count > 0 && app.aur_cursor < count - 1 {
                                app.aur_cursor += 1;
                            }
                        }
                        if app.quick_view {
                            app.update_quick_view();
                        }
                    }
                    KeyCode::Char(' ') | KeyCode::Insert => {
                        if app.active_pane == ActivePane::Left {
                            if let Some(pkg) = app.filtered_local_pkgs().get(app.local_cursor) {
                                let name = pkg.name.clone();
                                if app.local_tagged.contains(&name) {
                                    app.local_tagged.remove(&name);
                                } else {
                                    app.local_tagged.insert(name);
                                }
                            }
                        } else if let Some(pkg) = app.aur_results.get(app.aur_cursor) {
                            let name = pkg.name.clone();
                            if app.aur_tagged.contains(&name) {
                                app.aur_tagged.remove(&name);
                            } else {
                                app.aur_tagged.insert(name);
                            }
                        }
                    }
                    KeyCode::Backspace => {
                        if app.active_pane == ActivePane::Left {
                            app.local_filter.pop();
                            app.local_cursor = 0;
                        } else {
                            app.aur_search_query.pop();
                            app.search_debounce = Some(Instant::now());
                        }
                    }
                    KeyCode::Esc => {
                        if app.quick_view {
                            app.quick_view = false;
                        } else if app.active_pane == ActivePane::Left {
                            app.local_filter.clear();
                            app.local_cursor = 0;
                        } else {
                            app.aur_search_query.clear();
                            app.aur_results.clear();
                            app.aur_cursor = 0;
                            app.search_debounce = None;
                        }
                    }
                    KeyCode::Enter => {
                        if app.active_pane == ActivePane::Right {
                            // Enter on AUR Explorer: Build & integrate into local repository
                            if let Some(pkg) = app.aur_results.get(app.aur_cursor) {
                                let name = pkg.name.clone();
                                if let Err(e) = app.build_and_integrate_package(&name, false, &mut terminal) {
                                    app.modal = Some(Modal::Message(format!("Build Failed: {e}")));
                                }
                            }
                        } else if app.active_pane == ActivePane::Left {
                            let targets: Vec<String> = if !app.local_tagged.is_empty() {
                                app.local_tagged.iter().cloned().collect()
                            } else if let Some(pkg) = app.filtered_local_pkgs().get(app.local_cursor) {
                                vec![pkg.name.clone()]
                            } else {
                                vec![]
                            };

                            if !targets.is_empty() {
                                let is_installed = app.local_pkgs.iter().any(|p| targets.contains(&p.name) && p.is_installed);

                                if is_installed {
                                    let (cascade, total_size) = app.get_unneeded_cascade(&targets);
                                    app.modal = Some(Modal::Confirm(ConfirmationDialog {
                                        title: "Uninstall Package".to_string(),
                                        prompt: "Uninstall selected package(s) from host system?".to_string(),
                                        targets: targets.clone(),
                                        cascade,
                                        total_size,
                                        selected_button: 0,
                                        button_labels: vec!["Uninstall".to_string(), "Cancel".to_string()],
                                        action: ConfirmAction::UninstallSystem(targets),
                                    }));
                                } else {
                                    if let Some(pkg) = app.filtered_local_pkgs().get(app.local_cursor) {
                                        let path = pkg.file_path.clone();
                                        if let Err(e) = app.install_archive_via_worker(&path, &mut terminal) {
                                            app.modal = Some(Modal::Message(format!("Install Error: {e}")));
                                        }
                                    }
                                }
                            }
                        }
                    }
                    KeyCode::F(1) => {
                        app.modal = Some(Modal::Help);
                    }
                    KeyCode::F(2) => {
                        if app.active_pane == ActivePane::Left {
                            if let Some(pkg) = app.filtered_local_pkgs().get(app.local_cursor) {
                                let name = pkg.name.clone();
                                if let Err(e) = app.build_and_integrate_package(&name, true, &mut terminal) {
                                    app.modal = Some(Modal::Message(format!("Rebuild Failed: {e}")));
                                }
                            }
                        }
                    }
                    KeyCode::F(3) => {
                        app.toggle_quick_view();
                    }
                    KeyCode::F(4) => {
                        if app.active_pane == ActivePane::Left {
                            if let Err(e) = app.toggle_lock_current() {
                                app.modal = Some(Modal::Message(format!("Lock Error: {e}")));
                            }
                        }
                    }
                    KeyCode::F(5) => {
                        if app.active_pane == ActivePane::Left {
                            app.prompt_update_all();
                        } else if app.active_pane == ActivePane::Right {
                            if let Some(pkg) = app.aur_results.get(app.aur_cursor) {
                                let name = pkg.name.clone();
                                if let Err(e) = app.build_and_integrate_package(&name, false, &mut terminal) {
                                    app.modal = Some(Modal::Message(format!("Build Failed: {e}")));
                                }
                            }
                        }
                    }
                    KeyCode::F(6) => {
                        app.check_aur_updates();
                    }
                    KeyCode::F(7) => {
                        if app.active_pane == ActivePane::Left {
                            app.prompt_clean_cache();
                        }
                    }
                    KeyCode::F(8) => {
                        if app.active_pane == ActivePane::Left {
                            let targets: Vec<String> = if !app.local_tagged.is_empty() {
                                app.local_tagged.iter().cloned().collect()
                            } else if let Some(pkg) = app.filtered_local_pkgs().get(app.local_cursor) {
                                vec![pkg.name.clone()]
                            } else {
                                vec![]
                            };

                            if !targets.is_empty() {
                                let (cascade, total_size) = app.get_repo_cascade(&targets);
                                let mut all_delete = targets.clone();
                                all_delete.extend(cascade.clone());

                                let any_installed = app.local_pkgs.iter().any(|p| all_delete.contains(&p.name) && p.is_installed);

                                if any_installed {
                                    app.modal = Some(Modal::Confirm(ConfirmationDialog {
                                        title: "Delete / Uninstall Package".to_string(),
                                        prompt: "Package (or support packages) installed on host! How would you like to proceed?".to_string(),
                                        targets: targets.clone(),
                                        cascade,
                                        total_size,
                                        selected_button: 0,
                                        button_labels: vec![
                                            "Delete & Uninstall".to_string(),
                                            "Delete Repo Only".to_string(),
                                            "Cancel".to_string(),
                                        ],
                                        action: ConfirmAction::DeleteRepo(all_delete, true),
                                    }));
                                } else {
                                    app.modal = Some(Modal::Confirm(ConfirmationDialog {
                                        title: "Delete Package from Repo".to_string(),
                                        prompt: "Remove package(s) and unused support packages from bababool repository?".to_string(),
                                        targets: targets.clone(),
                                        cascade,
                                        total_size,
                                        selected_button: 0,
                                        button_labels: vec!["Delete".to_string(), "Cancel".to_string()],
                                        action: ConfirmAction::DeleteRepo(all_delete, false),
                                    }));
                                }
                            }
                        }
                    }
                    KeyCode::F(10) => {
                        app.should_quit = true;
                    }
                    KeyCode::Char(c) if c.is_alphanumeric() || matches!(c, '-' | '_' | '+' | '.' | '@') => {
                        if app.active_pane == ActivePane::Left {
                            app.local_filter.push(c);
                            app.local_cursor = 0;
                        } else {
                            app.aur_search_query.push(c);
                            app.search_debounce = Some(Instant::now());
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    Ok(())
}
