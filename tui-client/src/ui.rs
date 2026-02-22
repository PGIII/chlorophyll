use std::rc::Rc;

use chrono::Local;
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style, Stylize},
    text::Line,
    widgets::{Axis, Block, BorderType, Chart, Dataset, GraphType, Widget},
};

use crate::app::App;
use crate::log_widget::LogListWidget;

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

        let temperatures: Vec<(f64, f64)> = self
            .last_reading
            .iter()
            .enumerate()
            .map(|(i, entry)| {
                let chlorophyll_protocol::DataType::Temperature(t) = &entry.reading.value;
                (i as f64, *t as f64)
            })
            .collect();

        let dataset = Dataset::default()
            .name("Temperature (°C)")
            .style(Style::default().fg(Color::Yellow))
            .graph_type(GraphType::Line)
            .data(&temperatures);

        let y_min = temperatures.iter().map(|(_, y)| *y).fold(0.0, f64::min);
        let y_max = temperatures.iter().map(|(_, y)| *y).fold(100.0, f64::max);
        let y_bounds = if (y_max - y_min) < 10.0 {
            [y_min - 5.0, y_max + 5.0]
        } else {
            [y_min, y_max]
        };

        let x_max = temperatures.len().max(1) as f64;
        let x_label = if let Some(first) = self.last_reading.first() {
            let start = first.timestamp.with_timezone(&Local).format("%H:%M:%S");
            if let Some(last) = self.last_reading.last() {
                let end = last.timestamp.with_timezone(&Local).format("%H:%M:%S");
                Line::from(vec![
                    start.to_string().bold(),
                    " → ".into(),
                    end.to_string().bold(),
                ])
            } else {
                Line::from(start.to_string())
            }
        } else {
            Line::from("No data")
        };

        let y_label = format!("{:.1} - {:.1} °C", y_bounds[0], y_bounds[1]);

        let chart = Chart::new(vec![dataset])
            .block(
                Block::bordered()
                    .title("Temperature")
                    .border_type(BorderType::Rounded),
            )
            .x_axis(
                Axis::default()
                    .style(Style::default().fg(Color::Gray))
                    .bounds([0.0, x_max])
                    .labels([x_label]),
            )
            .y_axis(
                Axis::default()
                    .style(Style::default().fg(Color::Gray))
                    .bounds(y_bounds)
                    .labels(["0".into(), y_label, format!("{:.0}", y_bounds[1])]),
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
