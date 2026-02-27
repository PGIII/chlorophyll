use std::rc::Rc;

use chlorophyll_protocol::temperature::Temperature;
use chrono::Local;
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style, Stylize},
    symbols::Marker,
    text::Line,
    widgets::{Axis, Block, BorderType, Chart, Dataset, GraphType, Widget},
};

use crate::app::App;
use crate::log_widget::LogListWidget;

/// Maximum number of samples to display in the chart window
const CHART_WINDOW_SIZE: usize = 1000;

impl Widget for &App {
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

        // Take only the latest CHART_WINDOW_SIZE readings
        let window_start = self.last_reading.len().saturating_sub(CHART_WINDOW_SIZE);
        let window = &self.last_reading[window_start..];

        let temperatures: Vec<(f64, f64)> = window
            .iter()
            .enumerate()
            .map(|(i, entry)| {
                let chlorophyll_protocol::DataType::Temperature(t) = &entry.data_type;
                (i as f64, t.get_as_f() as f64)
            })
            .collect();

        let dataset = Dataset::default()
            .name("Temperature (°F)")
            .style(Style::default().fg(Color::Yellow))
            .graph_type(GraphType::Line)
            .marker(Marker::Braille)
            .data(&temperatures);

        let y_min = temperatures
            .iter()
            .map(|(_, y)| *y)
            .fold(f64::INFINITY, f64::min);
        let y_max = temperatures
            .iter()
            .map(|(_, y)| *y)
            .fold(f64::NEG_INFINITY, f64::max);

        let (y_min, y_max) = if temperatures.is_empty() {
            (32.0, 212.0)
        } else if (y_max - y_min).abs() < 1.0 {
            (y_min - 5.0, y_max + 5.0)
        } else {
            let padding = (y_max - y_min) * 0.1;
            (y_min - padding, y_max + padding)
        };

        // X bounds always 0..window length so latest data is at the right edge
        let x_max = (window.len().max(1) - 1).max(1) as f64;

        let x_labels: Vec<Line> = if let Some(first) = window.first() {
            let start = first
                .timestamp
                .with_timezone(&Local)
                .format("%H:%M:%S")
                .to_string();
            if let Some(last) = window.last() {
                let end = last
                    .timestamp
                    .with_timezone(&Local)
                    .format("%H:%M:%S")
                    .to_string();
                vec![start.bold().into(), end.bold().into()]
            } else {
                vec![start.into()]
            }
        } else {
            vec!["No data".into()]
        };

        let y_mid = (y_min + y_max) / 2.0;

        let chart = Chart::new(vec![dataset])
            .block(
                Block::bordered()
                    .title("Temperature")
                    .border_type(BorderType::Rounded),
            )
            .x_axis(
                Axis::default()
                    .title("Time")
                    .style(Style::default().fg(Color::Gray))
                    .bounds([0.0, x_max])
                    .labels(x_labels),
            )
            .y_axis(
                Axis::default()
                    .title("°F")
                    .style(Style::default().fg(Color::Gray))
                    .bounds([y_min, y_max])
                    .labels(vec![
                        Line::from(format!("{:.1}", y_min)),
                        Line::from(format!("{:.1}", y_mid)),
                        Line::from(format!("{:.1}", y_max)),
                    ]),
            );

        chart.render(main_area, buf);

        if self.log_state.enabled && chunks.len() > 1 {
            let log_area = chunks[1];
            let logs = self.log_state.logs();
            let log_widget = LogListWidget::new(&logs, "Logs", self.log_state.scroll);
            log_widget.render(log_area, buf);
        }
    }
}
