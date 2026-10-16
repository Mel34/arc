use crate::app::{ActivePane, App};
use crate::theme::*;
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, List, ListItem, Paragraph},
    Frame,
};

pub fn draw_panes(f: &mut Frame, left_area: Rect, right_area: Rect, app: &App) {
    draw_left_pane(f, left_area, app);
    draw_right_pane(f, right_area, app);
}

fn draw_left_pane(f: &mut Frame, area: Rect, app: &App) {
    let is_active = app.active_pane == ActivePane::Left;
    let border_style = if is_active {
        Style::default().fg(NC_CYAN)
    } else {
        Style::default().fg(NC_WHITE)
    };

    let title_line = Line::from(vec![
        Span::styled(
            format!(" [ Local Repo: {} ] ", app.repo_mgr.name),
            Style::default().fg(NC_YELLOW).add_modifier(Modifier::BOLD),
        ),
        if !app.local_filter.is_empty() {
            Span::styled(
                format!("[ Filter: {} ] ", app.local_filter),
                Style::default().fg(NC_WHITE).add_modifier(Modifier::BOLD),
            )
        } else {
            Span::raw("")
        },
    ]);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(border_style)
        .style(Style::default().bg(NC_BLUE).fg(NC_WHITE))
        .title(title_line);

    let filtered = app.filtered_local_pkgs();
    let total_count = filtered.len();
    let visible_rows = area.height.saturating_sub(3) as usize;

    let offset = if app.local_cursor >= visible_rows {
        app.local_cursor - visible_rows + 1
    } else {
        0
    };

    let max_name_w = (area.width.saturating_sub(34) as usize).max(10);
    let header = format!(
        "      {:<name_w$} {:<14} {:>7}  {}",
        "Package",
        "Version",
        "Size",
        "Status",
        name_w = max_name_w
    );

    let items: Vec<ListItem> = filtered
        .iter()
        .enumerate()
        .skip(offset)
        .take(visible_rows)
        .map(|(idx, pkg)| {
            let is_tagged = app.local_tagged.contains(&pkg.name);
            let tag_mark = if is_tagged { "[✔] " } else { "[ ] " };
            let inst_mark = if pkg.is_installed { "[I] " } else { "    " };
            let size_mib = pkg.size as f64 / (1024.0 * 1024.0);

            let status_mark = if let Some(ref new_ver) = pkg.aur_update {
                format!("▲ {}", new_ver)
            } else {
                "  OK".to_string()
            };

            let row_text = format!(
                "{}{}{:<name_w$} {:<14} {:>6.1}M  {}",
                tag_mark,
                inst_mark,
                pkg.name,
                pkg.version,
                size_mib,
                status_mark,
                name_w = max_name_w
            );

            if idx == app.local_cursor && is_active {
                ListItem::new(row_text).style(
                    Style::default()
                        .bg(NC_CYAN)
                        .fg(NC_BLACK)
                        .add_modifier(Modifier::BOLD),
                )
            } else if is_tagged {
                ListItem::new(row_text).style(
                    Style::default()
                        .fg(NC_YELLOW)
                        .add_modifier(Modifier::BOLD),
                )
            } else if pkg.aur_update.is_some() {
                ListItem::new(row_text).style(Style::default().fg(NC_YELLOW))
            } else {
                ListItem::new(row_text).style(Style::default().fg(NC_WHITE))
            }
        })
        .collect();

    f.render_widget(block, area);

    if area.height > 3 && area.width > 20 {
        let header_rect = Rect {
            x: area.x + 1,
            y: area.y + 1,
            width: area.width.saturating_sub(2),
            height: 1,
        };
        f.render_widget(
            Paragraph::new(header).style(
                Style::default()
                    .fg(NC_CYAN)
                    .bg(NC_BLUE)
                    .add_modifier(Modifier::BOLD),
            ),
            header_rect,
        );

        let list_rect = Rect {
            x: area.x + 1,
            y: area.y + 2,
            width: area.width.saturating_sub(2),
            height: area.height.saturating_sub(3),
        };
        f.render_widget(List::new(items), list_rect);
    }

    let status_str = format!(
        " {}/{} pkgs ({} sel) ",
        if total_count == 0 { 0 } else { app.local_cursor + 1 },
        total_count,
        app.local_tagged.len()
    );
    let status_rect = Rect {
        x: area.x + area.width.saturating_sub(status_str.len() as u16 + 2),
        y: area.y + area.height - 1,
        width: status_str.len() as u16,
        height: 1,
    };
    f.render_widget(
        Paragraph::new(status_str).style(Style::default().fg(NC_CYAN).bg(NC_BLUE)),
        status_rect,
    );
}

fn draw_right_pane(f: &mut Frame, area: Rect, app: &App) {
    let is_active = app.active_pane == ActivePane::Right;
    let border_style = if is_active {
        Style::default().fg(NC_CYAN)
    } else {
        Style::default().fg(NC_WHITE)
    };

    let title_line = Line::from(vec![
        Span::styled(
            " [ AUR Explorer ] ",
            Style::default().fg(NC_YELLOW).add_modifier(Modifier::BOLD),
        ),
        if !app.aur_search_query.is_empty() {
            Span::styled(
                format!("[ Search: {} ] ", app.aur_search_query),
                Style::default().fg(NC_WHITE).add_modifier(Modifier::BOLD),
            )
        } else {
            Span::styled(
                "[ Type to search ] ",
                Style::default().fg(NC_GRAY),
            )
        },
    ]);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(border_style)
        .style(Style::default().bg(NC_BLUE).fg(NC_WHITE))
        .title(title_line);

    let total_count = app.aur_results.len();
    let visible_rows = area.height.saturating_sub(3) as usize;

    let offset = if app.aur_cursor >= visible_rows {
        app.aur_cursor - visible_rows + 1
    } else {
        0
    };

    let max_name_w = (area.width.saturating_sub(32) as usize).max(10);
    let header = format!(
        "    {:<name_w$} {:<14} {:>5} {:>6}",
        "Package",
        "Version",
        "Votes",
        "Pop",
        name_w = max_name_w
    );

    let items: Vec<ListItem> = app
        .aur_results
        .iter()
        .enumerate()
        .skip(offset)
        .take(visible_rows)
        .map(|(idx, pkg)| {
            let is_tagged = app.aur_tagged.contains(&pkg.name);
            let tag_mark = if is_tagged { "[✔] " } else { "    " };

            let row_text = format!(
                "{}{:<name_w$} {:<14} {:>5} {:>6.1}",
                tag_mark,
                pkg.name,
                pkg.version,
                pkg.votes,
                pkg.popularity,
                name_w = max_name_w
            );

            if idx == app.aur_cursor && is_active {
                ListItem::new(row_text).style(
                    Style::default()
                        .bg(NC_CYAN)
                        .fg(NC_BLACK)
                        .add_modifier(Modifier::BOLD),
                )
            } else if is_tagged {
                ListItem::new(row_text).style(
                    Style::default()
                        .fg(NC_YELLOW)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                ListItem::new(row_text).style(Style::default().fg(NC_WHITE))
            }
        })
        .collect();

    f.render_widget(block, area);

    if area.height > 3 && area.width > 20 {
        let header_rect = Rect {
            x: area.x + 1,
            y: area.y + 1,
            width: area.width.saturating_sub(2),
            height: 1,
        };
        f.render_widget(
            Paragraph::new(header).style(
                Style::default()
                    .fg(NC_CYAN)
                    .bg(NC_BLUE)
                    .add_modifier(Modifier::BOLD),
            ),
            header_rect,
        );

        let list_rect = Rect {
            x: area.x + 1,
            y: area.y + 2,
            width: area.width.saturating_sub(2),
            height: area.height.saturating_sub(3),
        };
        f.render_widget(List::new(items), list_rect);
    }

    let status_str = format!(
        " {}/{} results ",
        if total_count == 0 { 0 } else { app.aur_cursor + 1 },
        total_count
    );
    let status_rect = Rect {
        x: area.x + area.width.saturating_sub(status_str.len() as u16 + 2),
        y: area.y + area.height - 1,
        width: status_str.len() as u16,
        height: 1,
    };
    f.render_widget(
        Paragraph::new(status_str).style(Style::default().fg(NC_CYAN).bg(NC_BLUE)),
        status_rect,
    );
}
