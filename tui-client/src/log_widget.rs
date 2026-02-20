use std::collections::VecDeque;
use std::sync::Mutex;

use lazy_static::lazy_static;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    widgets::{Block, BorderType, List, ListItem, Widget},
};

lazy_static! {
    pub static ref LOGS: Mutex<VecDeque<String>> = Mutex::new(VecDeque::with_capacity(1000));
}

#[derive(Debug, Clone)]
pub struct LogState {
    pub enabled: bool,
    pub scroll: u16,
}

impl LogState {
    pub fn new(enabled: bool) -> Self {
        Self { enabled, scroll: 0 }
    }

    pub fn toggle(&mut self) {
        self.enabled = !self.enabled;
    }

    pub fn scroll_up(&mut self, amount: u16) {
        self.scroll = self.scroll.saturating_add(amount);
    }

    pub fn scroll_down(&mut self, amount: u16) {
        self.scroll = self.scroll.saturating_sub(amount);
    }

    pub fn logs(&self) -> std::sync::MutexGuard<'static, VecDeque<String>> {
        LOGS.lock().unwrap()
    }
}

#[derive(Debug, Clone)]
pub struct LogDebugWidget {
    pub title: String,
}

impl Default for LogDebugWidget {
    fn default() -> Self {
        Self {
            title: String::from("Log Debug"),
        }
    }
}

impl LogDebugWidget {
    pub fn new(title: &str) -> Self {
        Self {
            title: title.to_string(),
        }
    }
}

impl Widget for &LogDebugWidget {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width < 10 || area.height < 3 {
            return;
        }

        let block = Block::bordered()
            .title(self.title.as_str())
            .border_type(BorderType::Rounded);

        let inner_area = block.inner(area);
        block.render(area, buf);

        let log_items: Vec<ListItem> =
            std::iter::repeat(ListItem::new("").style(Style::default().fg(Color::Gray)))
                .take(inner_area.height as usize)
                .collect();

        let list = List::new(log_items)
            .block(Block::new())
            .style(Style::default().fg(Color::White));

        list.render(inner_area, buf);
    }
}

pub struct LogListWidget<'a> {
    logs: &'a VecDeque<String>,
    title: String,
    scroll_offset: u16,
}

impl<'a> LogListWidget<'a> {
    pub fn new(logs: &'a VecDeque<String>, title: &str, scroll_offset: u16) -> Self {
        Self {
            logs,
            title: title.to_string(),
            scroll_offset,
        }
    }
}

impl Widget for &LogListWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width < 10 || area.height < 3 {
            return;
        }

        let block = Block::bordered()
            .title(self.title.as_str())
            .border_type(BorderType::Rounded);

        let inner_area = block.inner(area);
        block.render(area, buf);

        let total_logs = self.logs.len();
        let visible_height = inner_area.height as usize;

        let max_scroll = total_logs.saturating_sub(visible_height);
        let scroll = std::cmp::min(self.scroll_offset as usize, max_scroll);

        let items: Vec<ListItem> = self
            .logs
            .iter()
            .skip(scroll)
            .take(visible_height)
            .map(|log: &String| {
                let style = if log.contains("ERROR") {
                    Style::default().fg(Color::Red)
                } else if log.contains("WARN") {
                    Style::default().fg(Color::Yellow)
                } else if log.contains("DEBUG") {
                    Style::default().fg(Color::Green)
                } else if log.contains("TRACE") {
                    Style::default().fg(Color::DarkGray)
                } else {
                    Style::default().fg(Color::White)
                };
                ListItem::new(log.as_str()).style(style)
            })
            .collect();

        if !items.is_empty() {
            let list = List::new(items).block(Block::new()).style(Style::default());
            list.render(inner_area, buf);
        }
    }
}
