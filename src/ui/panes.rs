use crate::app::{ActivePane, App};
use crate::theme::*;
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, List, ListItem, Paragraph, Wrap},
    Frame,
};

pub fn truncate_middle(s: &str, max_len: usize) -> String {
    let count = s.chars().count();
    if count <= max_len {
        return s.to_string();
    }
    if max_len <= 3 {
        return s.chars().take(max_len).collect();
    }
    let keep = max_len.saturating_sub(1);
    let front = keep / 2;
    let back = keep - front;
    let s_chars: Vec<char> = s.chars().collect();
    let front_str: String = s_chars[..front].iter().collect();
    let back_str: String = s_chars[count - back..].iter().collect();
    format!("{front_str}…{back_str}")
}

pub fn clean_version(ver: &str, max_len: usize) -> String {
    if max_len == 6 {
        let without_epoch = ver.split_once(':').map_or(ver, |(_, version)| version);
        let pkgver = without_epoch
            .rsplit_once('-')
            .map_or(without_epoch, |(version, _)| version);
        return pkgver.chars().take(max_len).collect();
    }
    if ver.chars().count() <= max_len {
        return ver.to_string();
    }
    if let Some((base, _)) = ver.split_once(".r") {
        let candidate = format!("{base}.r…");
        if candidate.chars().count() <= max_len {
            return candidate;
        }
    } else if let Some((base, _)) = ver.split_once(".git") {
        let candidate = format!("{base}…");
        if candidate.chars().count() <= max_len {
            return candidate;
        }
    }
    truncate_middle(ver, max_len)
}

fn draw_quick_view(f: &mut Frame, area: Rect, title: &str, content: &[String], scroll: usize) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(NC_YELLOW))
        .style(Style::default().bg(NC_BLUE).fg(NC_WHITE))
        .title(Span::styled(
            format!(" [ Quick View: {} (F3/Esc: Close) ] ", title),
            Style::default().fg(NC_YELLOW).add_modifier(Modifier::BOLD),
        ));

    let visible_rows = area.height.saturating_sub(2) as usize;
    let lines: Vec<Line> = content
        .iter()
        .skip(scroll)
        .take(visible_rows)
        .map(|l| Line::from(Span::raw(l)))
        .collect();

    let p = Paragraph::new(lines).block(block).wrap(Wrap { trim: false });
    f.render_widget(p, area);
}

pub fn draw_panes(f: &mut Frame, left_area: Rect, right_area: Rect, app: &App) {
    if app.quick_view && app.active_pane == ActivePane::Right {
        draw_quick_view(f, left_area, &app.quick_view_title, &app.quick_view_content, app.quick_view_scroll);
        draw_right_pane(f, right_area, app);
        return;
    }
    if app.quick_view && app.active_pane == ActivePane::Left {
        draw_left_pane(f, left_area, app);
        draw_quick_view(f, right_area, &app.quick_view_title, &app.quick_view_content, app.quick_view_scroll);
        return;
    }

    draw_left_pane(f, left_area, app);
    draw_right_pane(f, right_area, app);
}

fn draw_left_pane(f: &mut Frame, area: Rect, app: &App) {
    let is_active = app.active_pane == ActivePane::Left;
    let border_style = if is_active {
        Style::default().fg(NC_WHITE)
    } else {
        Style::default().fg(NC_CYAN)
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

    let name_w = (area.width.saturating_sub(39) as usize).max(10);
    let header = format!(
        "      {:<name_w$} {:<12} {:>7}  {:<8}",
        "Package",
        "Version",
        "Size",
        "Status",
        name_w = name_w
    );

    let items: Vec<ListItem> = filtered
        .iter()
        .enumerate()
        .skip(offset)
        .take(visible_rows)
        .map(|(idx, pkg)| {
            let is_tagged = app.local_tagged.contains(&pkg.name);
            let tag_mark = if is_tagged { "[✔] " } else { "[ ] " };
            let lock_mark = if app.locked_pkgs.contains(&pkg.name) { "\u{f023} " } else { "  " };
            let size_mib = pkg.size as f64 / (1024.0 * 1024.0);

            let is_repo_dep = app
                .local_pkgs
                .iter()
                .any(|other| other.name != pkg.name && other.depends.contains(&pkg.name));

            let status_str = if let Some(ref new_ver) = pkg.aur_update {
                format!("▲ {}", clean_version(new_ver, 6))
            } else if pkg.is_installed {
                "   ✔    ".to_string()
            } else if is_repo_dep {
                "   \u{f44c}    ".to_string()
            } else {
                "   \u{f487}    ".to_string()
            };

            let trunc_name = truncate_middle(&pkg.name, name_w);
            let trunc_ver = clean_version(&pkg.version, 12);

            let row_text = format!(
                "{}{}{:<name_w$} {:<12} {:>6.1}M  {:<8}",
                tag_mark,
                lock_mark,
                trunc_name,
                trunc_ver,
                size_mib,
                status_str,
                name_w = name_w
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
            } else if app.locked_pkgs.contains(&pkg.name) {
                ListItem::new(row_text).style(Style::default().fg(NC_YELLOW))
            } else if pkg.aur_update.is_some() {
                ListItem::new(row_text).style(Style::default().fg(NC_YELLOW))
            } else if pkg.is_installed {
                ListItem::new(row_text).style(Style::default().fg(NC_WHITE))
            } else {
                ListItem::new(row_text).style(Style::default().fg(NC_GRAY))
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
        Style::default().fg(NC_WHITE)
    } else {
        Style::default().fg(NC_CYAN)
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
            Span::styled("[ Type to search ] ", Style::default().fg(NC_GRAY))
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

    let max_name_w = (area.width.saturating_sub(25) as usize).max(10);
    let header = format!(
        "    {:<name_w$} {:<12} {:>5}",
        "Package",
        "Version",
        "Votes",
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

            let trunc_name = truncate_middle(&pkg.name, max_name_w);
            let trunc_ver = clean_version(&pkg.version, 12);

            let row_text = format!(
                "{}{:<name_w$} {:<12} {:>5}",
                tag_mark,
                trunc_name,
                trunc_ver,
                pkg.votes,
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
