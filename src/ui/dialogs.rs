use crate::theme::*;
use ratatui::{
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap},
    Frame,
};

pub struct ProgressState {
    pub title: String,
    pub target: String,
    pub step: String,
    pub percent: u16,
}

#[derive(Clone)]
pub enum ConfirmAction {
    DeleteRepo(Vec<String>, bool),
    UninstallSystem(Vec<String>),
    CleanCache(Vec<std::path::PathBuf>),
    UpdateAll(Vec<String>),
}

#[derive(Clone)]
pub struct ConfirmationDialog {
    pub title: String,
    pub prompt: String,
    pub targets: Vec<String>,
    pub cascade: Vec<String>,
    pub total_size: i64,
    pub selected_button: usize,
    pub button_labels: Vec<String>,
    pub action: ConfirmAction,
}

pub fn draw_dialog_box(f: &mut Frame, width: u16, height: u16) -> Rect {
    let screen = f.area();
    let w = width.min(screen.width.saturating_sub(4));
    let h = height.min(screen.height.saturating_sub(2));

    let x = (screen.width.saturating_sub(w)) / 2;
    let y = (screen.height.saturating_sub(h)) / 2;

    let shadow_rect = Rect {
        x: (x + 2).min(screen.width - 1),
        y: (y + 1).min(screen.height - 1),
        width: w.min(screen.width.saturating_sub(x + 2)),
        height: h.min(screen.height.saturating_sub(y + 1)),
    };
    f.render_widget(Block::default().style(Style::default().bg(NC_BLACK)), shadow_rect);

    let dialog_rect = Rect { x, y, width: w, height: h };
    f.render_widget(Clear, dialog_rect);
    dialog_rect
}

pub fn draw_confirmation_dialog(f: &mut Frame, diag: &ConfirmationDialog) {
    let height = if !diag.cascade.is_empty() { 18 } else { 14 };
    let dialog_rect = draw_dialog_box(f, 74, height);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(NC_WHITE))
        .style(Style::default().bg(NC_GRAY).fg(NC_BLACK))
        .title(Span::styled(
            format!(" {} ", diag.title),
            Style::default().fg(NC_WHITE).bg(NC_DARK_GRAY).add_modifier(Modifier::BOLD),
        ));

    let targets_preview = if diag.targets.len() > 3 {
        format!("{} (+{} more)", diag.targets[..3].join(", "), diag.targets.len() - 3)
    } else {
        diag.targets.join(", ")
    };

    let mut lines = vec![
        Line::from(""),
        Line::from(Span::styled(&diag.prompt, Style::default().fg(NC_BLACK).add_modifier(Modifier::BOLD))),
        Line::from(Span::styled(format!("  • {}", targets_preview), Style::default().fg(NC_BLUE).add_modifier(Modifier::BOLD))),
    ];

    if !diag.cascade.is_empty() {
        let cascade_preview = if diag.cascade.len() > 4 {
            format!("{} (+{} more)", diag.cascade[..4].join(", "), diag.cascade.len() - 4)
        } else {
            diag.cascade.join(", ")
        };
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Dependent support packages to be removed (-Rns):",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(Span::styled(format!("  • {}", cascade_preview), Style::default().fg(NC_BLACK))));
    }

    if diag.total_size > 0 {
        lines.push(Line::from(""));
        let size_mib = diag.total_size as f64 / (1024.0 * 1024.0);
        lines.push(Line::from(Span::styled(
            format!("Total space to free: {:.2} MiB", size_mib),
            Style::default().fg(NC_DARK_GRAY).add_modifier(Modifier::BOLD),
        )));
    }

    lines.push(Line::from(""));
    let mut btn_spans = Vec::new();
    for (idx, label) in diag.button_labels.iter().enumerate() {
        let style = if idx == diag.selected_button {
            Style::default().bg(NC_CYAN).fg(NC_BLACK).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(NC_BLACK)
        };
        btn_spans.push(Span::styled(format!("  {}  ", label), style));
        if idx + 1 < diag.button_labels.len() {
            btn_spans.push(Span::raw("    "));
        }
    }
    lines.push(Line::from(btn_spans));

    let p = Paragraph::new(lines).block(block).alignment(Alignment::Center);
    f.render_widget(p, dialog_rect);
}

pub fn draw_progress_dialog(f: &mut Frame, prog: &ProgressState) {
    let dialog_rect = draw_dialog_box(f, 66, 12);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(NC_CYAN))
        .style(Style::default().bg(NC_BLUE).fg(NC_WHITE))
        .title(Span::styled(
            format!(" [ ARC: {} ] ", prog.title),
            Style::default().fg(NC_YELLOW).add_modifier(Modifier::BOLD),
        ));

    let gauge_width = 42;
    let filled = ((prog.percent as usize * gauge_width) / 100).min(gauge_width);
    let empty = gauge_width - filled;
    let bar_str = format!("[{}{}] {}%", "█".repeat(filled), "░".repeat(empty), prog.percent);

    let is_building = prog.title.contains("Build");
    let escape_hint = if is_building {
        Span::styled("[ Press ESC to Abort ]", Style::default().fg(NC_YELLOW).add_modifier(Modifier::BOLD))
    } else {
        Span::raw("")
    };

    let text = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("Target : ", Style::default().fg(NC_CYAN)),
            Span::styled(&prog.target, Style::default().fg(NC_WHITE).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled("Status : ", Style::default().fg(NC_CYAN)),
            Span::styled(&prog.step, Style::default().fg(NC_YELLOW)),
        ]),
        Line::from(""),
        Line::from(Span::styled(bar_str, Style::default().fg(NC_CYAN).add_modifier(Modifier::BOLD))),
        Line::from(""),
        Line::from(escape_hint),
    ];

    let p = Paragraph::new(text).block(block).alignment(Alignment::Center);
    f.render_widget(p, dialog_rect);
}

pub fn draw_help_dialog(f: &mut Frame) {
    let dialog_rect = draw_dialog_box(f, 74, 23);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(NC_WHITE))
        .style(Style::default().bg(NC_BLUE).fg(NC_WHITE))
        .title(Span::styled(
            " [ Arch Repo Commander: Help ] ",
            Style::default().fg(NC_YELLOW).add_modifier(Modifier::BOLD),
        ));

    let help_entries = [
        ("Tab", "Switch active pane (Local Repo ⇄ AUR)"),
        ("Enter", "Install/Uninstall (Left) / Build&Repo (Right)"),
        ("Home / End", "Jump to top / bottom of list"),
        ("PgUp / PgDn", "Scroll list / scroll PKGBUILD preview"),
        ("Space / Ins", "Tag / multi-select packages ([✔])"),
        ("F1", "Show this Help dialog"),
        ("F2", "Rebuild selected package (cleanbuild)"),
        ("F3", "Quick View PKGBUILD in opposite pane"),
        ("F4", "Lock / Unlock package version (ignore updates)"),
        ("F5", "Update All (Left) / Build&Repo (Right)"),
        ("F6", "Check upstream AUR updates"),
        ("F7", "Clean old package versions from cache"),
        ("F8", "Delete from repository & disk (aurdel)"),
        ("F10", "Quit ARC"),
    ];

    let mut lines = vec![Line::from("")];
    for (key, desc) in help_entries {
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(format!("{:<13}", key), Style::default().fg(NC_YELLOW).add_modifier(Modifier::BOLD)),
            Span::styled(": ", Style::default().fg(NC_CYAN)),
            Span::styled(desc, Style::default().fg(NC_WHITE)),
        ]));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled("Status Icons : ", Style::default().fg(NC_CYAN).add_modifier(Modifier::BOLD)),
        Span::styled("✔ ", Style::default().fg(NC_WHITE).add_modifier(Modifier::BOLD)),
        Span::styled("Installed   ", Style::default().fg(NC_GRAY)),
        Span::styled("\u{f487} ", Style::default().fg(NC_GRAY).add_modifier(Modifier::BOLD)),
        Span::styled("Standalone   ", Style::default().fg(NC_GRAY)),
        Span::styled("\u{f44c} ", Style::default().fg(NC_GRAY).add_modifier(Modifier::BOLD)),
        Span::styled("Dependency   ", Style::default().fg(NC_GRAY)),
        Span::styled("▲ ", Style::default().fg(NC_YELLOW).add_modifier(Modifier::BOLD)),
        Span::styled("Update", Style::default().fg(NC_GRAY)),
    ]));
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::raw("                        "),
        Span::styled("[ Press ESC to close ]", Style::default().fg(NC_CYAN).add_modifier(Modifier::BOLD)),
    ]));

    let p = Paragraph::new(lines).block(block);
    f.render_widget(p, dialog_rect);
}

pub fn draw_message_dialog(f: &mut Frame, msg: &str) {
    let screen = f.area();
    let width = 64.min(screen.width.saturating_sub(4));
    let height = 12.min(screen.height.saturating_sub(2));

    let dialog_rect = draw_dialog_box(f, width, height);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(Color::Red))
        .style(Style::default().bg(NC_BLUE).fg(NC_WHITE))
        .title(Span::styled(" [ Notice ] ", Style::default().fg(NC_YELLOW)));

    let text = vec![
        Line::from(""),
        Line::from(Span::styled(msg, Style::default().fg(NC_WHITE))),
        Line::from(""),
        Line::from(Span::styled("[ Press ESC/Enter to close ]", Style::default().bg(NC_CYAN).fg(NC_BLACK))),
    ];

    let p = Paragraph::new(text).block(block).alignment(Alignment::Center).wrap(Wrap { trim: false });
    f.render_widget(p, dialog_rect);
}
