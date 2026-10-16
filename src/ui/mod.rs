pub mod bottom_bar;
pub mod dialogs;
pub mod panes;

use crate::app::App;
use dialogs::*;
use ratatui::{
    layout::{Constraint, Direction, Layout},
    Frame,
};

pub fn draw_ui(f: &mut Frame, app: &App) {
    let size = f.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(size);

    let pane_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[0]);

    panes::draw_panes(f, pane_chunks[0], pane_chunks[1], app);
    bottom_bar::draw_bottom_bar(f, chunks[1], app);

    if let Some(ref modal) = app.modal {
        match modal {
            crate::app::Modal::Confirm(diag) => draw_confirmation_dialog(f, diag),
            crate::app::Modal::Help => draw_help_dialog(f),
            crate::app::Modal::Message(msg) => draw_message_dialog(f, msg),
        }
    } else if let Some(ref prog) = app.progress {
        draw_progress_dialog(f, prog);
    }
}
