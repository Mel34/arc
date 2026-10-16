use crate::app::{ActivePane, App};
use crate::theme::*;
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

pub fn draw_bottom_bar(f: &mut Frame, area: Rect, app: &App) {
    let f_keys = match app.active_pane {
        ActivePane::Left => {
            let is_installed = app
                .filtered_local_pkgs()
                .get(app.local_cursor)
                .map(|p| p.is_installed)
                .unwrap_or(false);
            let is_locked = app
                .filtered_local_pkgs()
                .get(app.local_cursor)
                .map(|p| app.locked_pkgs.contains(&p.name))
                .unwrap_or(false);

            let enter_label = if is_installed { "Uninstall" } else { "Install" };
            let lock_label = if is_locked { "Unlock" } else { "Lock" };

            vec![
                ("F1", "Help"),
                ("Enter", enter_label),
                ("F2", "Rebuild"),
                ("F3", "QuickView"),
                ("F4", lock_label),
                ("F5", "UpdAll"),
                ("F6", "CheckUpd"),
                ("F7", "CleanCache"),
                ("F8", "DelRepo"),
                ("F10", "Quit"),
            ]
        }
        ActivePane::Right => vec![
            ("F1", "Help"),
            ("Enter", "Build&Repo"),
            ("F3", "QuickView"),
            ("F10", "Quit"),
        ],
    };

    let mut spans = Vec::new();
    for (key, label) in f_keys {
        spans.push(Span::styled(
            format!(" {} ", key),
            Style::default().fg(NC_BLACK).bg(NC_CYAN).add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(
            format!("{} ", label),
            Style::default().fg(NC_BLACK).bg(NC_GRAY),
        ));
        spans.push(Span::raw(" "));
    }

    let p = Paragraph::new(Line::from(spans)).style(Style::default().bg(NC_GRAY));
    f.render_widget(p, area);
}
