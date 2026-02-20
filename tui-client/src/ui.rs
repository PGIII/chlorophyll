use std::rc::Rc;

use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Stylize},
    widgets::{Block, BorderType, Paragraph, Widget},
};

use crate::app::App;
use crate::log_widget::LogListWidget;

impl Widget for &App {
    /// Renders the user interface widgets.
    ///
    // This is where you add new widgets.
    // See the following resources:
    // - https://docs.rs/ratatui/latest/ratatui/widgets/index.html
    // - https://github.com/ratatui/ratatui/tree/master/examples
    fn render(self, area: Rect, buf: &mut Buffer) {
        let chunks = if self.log_state.enabled {
            Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(3), Constraint::Percentage(33)])
                .split(area)
        } else {
            Rc::from([area])
        };

        let main_area = chunks[0];

        let block = Block::bordered()
            .title("tui-client")
            .title_alignment(Alignment::Center)
            .border_type(BorderType::Rounded);

        let last_msg = if let Some(last_msg) = self.last_reading.clone() {
            format!("{:?}", last_msg)
        } else {
            String::from("")
        };

        let text = format!(
            "This is a tui template.\n\
                Press `Esc`, `Ctrl-C` or `q` to stop running.\n\
                Press left and right to increment and decrement the counter respectively.\n\
                Counter: {}
                Last Message: '{}'\n\
                Press Shift+L to toggle log panel, Up/Down to scroll, PgUp/PgDn for fast scroll",
            self.counter, last_msg
        );

        let paragraph = Paragraph::new(text)
            .block(block)
            .fg(Color::Cyan)
            .bg(Color::Black)
            .centered();

        paragraph.render(main_area, buf);

        if self.log_state.enabled && chunks.len() > 1 {
            let log_area = chunks[1];
            let logs = self.log_state.logs();
            let log_widget = LogListWidget::new(&logs, "Logs", self.log_state.scroll);
            log_widget.render(log_area, buf);
        }
    }
}
