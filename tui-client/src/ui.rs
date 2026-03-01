use std::collections::HashMap;
use std::rc::Rc;

use chlorophyll_protocol::light::Light;
use chlorophyll_protocol::temperature::Temperature;
use chrono::{DateTime, Local, Utc};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style, Stylize},
    symbols::Marker,
    text::Line,
    widgets::{Axis, Block, BorderType, Chart, Dataset, GraphType, List, ListItem, Widget},
};

use crate::app::App;
use crate::log_widget::LogListWidget;

impl Widget for &App {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Outer: content + optional log
        let outer_chunks = if self.log_state.enabled {
            Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(3), Constraint::Percentage(33)])
                .split(area)
        } else {
            Rc::from([area])
        };

        let content_area = outer_chunks[0];

        // Two columns: sensors left (40), charts right (60%)
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
            .split(content_area);

        let sensor_area = cols[0];

        // Right column: temp/humidity on top, light on bottom
        let right_rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(cols[1]);

        let (temp_area, light_area) = (right_rows[0], right_rows[1]);

        let now = Utc::now();
        let x_end = now.timestamp() as f64;

        let window: Vec<_> = self.last_reading.iter().collect();

        // --- Data extraction (x = Unix timestamp) ---

        let temperatures: Vec<(f64, f64)> = window
            .iter()
            .filter_map(|entry| {
                if let chlorophyll_protocol::DataType::Temperature(t) = &entry.data_type {
                    Some((entry.timestamp.timestamp() as f64, t.get_as_f() as f64))
                } else {
                    None
                }
            })
            .collect();

        let humidities: Vec<(f64, f64)> = window
            .iter()
            .filter_map(|entry| {
                if let chlorophyll_protocol::DataType::RelativeHumidity(h) = &entry.data_type {
                    Some((entry.timestamp.timestamp() as f64, h.percent() as f64))
                } else {
                    None
                }
            })
            .collect();

        let lights: Vec<(f64, f64)> = window
            .iter()
            .filter_map(|entry| {
                if let chlorophyll_protocol::DataType::Light(l) = &entry.data_type {
                    Some((entry.timestamp.timestamp() as f64, l.get_as_lux() as f64))
                } else {
                    None
                }
            })
            .collect();

        // --- Sensor summary map: (temp_f, humidity_pct, lux, last_seen) ---
        let mut sensor_map: HashMap<u128, (Option<f32>, Option<f32>, Option<f32>, Option<DateTime<Utc>>)> = HashMap::new();
        for entry in self.last_reading.iter().rev() {
            let e = sensor_map.entry(entry.sensor_id).or_default();
            match &entry.data_type {
                chlorophyll_protocol::DataType::Temperature(t) if e.0.is_none() => {
                    e.0 = Some(t.get_as_f());
                }
                chlorophyll_protocol::DataType::RelativeHumidity(h) if e.1.is_none() => {
                    e.1 = Some(h.percent());
                }
                chlorophyll_protocol::DataType::Light(l) if e.2.is_none() => {
                    e.2 = Some(l.get_as_lux());
                }
                _ => {}
            }
            // Record most-recent timestamp (first time we see this sensor when iterating rev)
            if e.3.is_none() {
                e.3 = Some(entry.timestamp);
            }
        }

        // --- Left panel: sensor list ---
        let mut sensor_ids: Vec<u128> = sensor_map.keys().copied().collect();
        sensor_ids.sort();

        let items: Vec<ListItem> = sensor_ids
            .iter()
            .map(|id| {
                let (temp, hum, lux, last_seen) = sensor_map[id];
                let temp_str = temp.map_or("--".into(), |v| format!("{:.1}°F", v));
                let hum_str = hum.map_or("--".into(), |v| format!("{:.1}%", v));
                let lux_str = lux.map_or("--".into(), |v| format!("{:.0}lx", v));
                let age_str = last_seen.map_or("--".into(), |ts| {
                    let secs = (now - ts).num_seconds().max(0);
                    if secs < 60 {
                        format!("{}s", secs)
                    } else if secs < 3600 {
                        format!("{}m{}s", secs / 60, secs % 60)
                    } else {
                        format!("{}h{}m", secs / 3600, (secs % 3600) / 60)
                    }
                });
                let text = format!(
                    "{:16x} {} {} {} {}",
                    id & 0xFFFFFFFFFFFFFFFF,
                    temp_str, hum_str, lux_str, age_str
                );
                ListItem::new(text)
            })
            .collect();

        let sensor_list = List::new(items).block(
            Block::bordered()
                .title("Sensors")
                .border_type(BorderType::Rounded),
        );
        sensor_list.render(sensor_area, buf);

        // --- Center panel: Temp & Humidity chart ---
        let temp_dataset = Dataset::default()
            .name("Temp (°F)")
            .style(Style::default().fg(Color::Yellow))
            .graph_type(GraphType::Line)
            .marker(Marker::Braille)
            .data(&temperatures);

        let hum_dataset = Dataset::default()
            .name("Humidity (%)")
            .style(Style::default().fg(Color::Blue))
            .graph_type(GraphType::Line)
            .marker(Marker::Braille)
            .data(&humidities);

        // Y-bounds across both temperature and humidity
        let all_y = temperatures
            .iter()
            .chain(humidities.iter())
            .map(|(_, y)| *y);
        let cy_min = all_y.clone().fold(f64::INFINITY, f64::min);
        let cy_max = all_y.fold(f64::NEG_INFINITY, f64::max);

        let (cy_min, cy_max) = if temperatures.is_empty() && humidities.is_empty() {
            (0.0, 100.0)
        } else if (cy_max - cy_min).abs() < 1.0 {
            (cy_min - 5.0, cy_max + 5.0)
        } else {
            let padding = (cy_max - cy_min) * 0.1;
            (cy_min - padding, cy_max + padding)
        };

        // x_start = timestamp of the oldest reading, or 1 min before now if no data
        let x_start = window
            .first()
            .map(|e| e.timestamp.timestamp() as f64)
            .unwrap_or(x_end - 60.0);

        let x_labels: Vec<Line> = vec![
            window
                .first()
                .map(|e| e.timestamp.with_timezone(&Local).format("%H:%M").to_string())
                .unwrap_or_else(|| "No data".into())
                .bold()
                .into(),
            now.with_timezone(&Local)
                .format("%H:%M")
                .to_string()
                .bold()
                .into(),
        ];

        let cy_mid = (cy_min + cy_max) / 2.0;

        let cur_temp = temperatures.last().map(|(_, v)| *v);
        let cur_hum = humidities.last().map(|(_, v)| *v);
        let cy_title = match (cur_temp, cur_hum) {
            (Some(t), Some(h)) => format!("{:.1}°F / {:.1}%", t, h),
            (Some(t), None) => format!("{:.1}°F / %", t),
            (None, Some(h)) => format!("°F / {:.1}%", h),
            (None, None) => "°F / %".into(),
        };

        let temp_chart = Chart::new(vec![temp_dataset, hum_dataset])
            .block(
                Block::bordered()
                    .title("Temp & Humidity")
                    .border_type(BorderType::Rounded),
            )
            .x_axis(
                Axis::default()
                    .title("Time")
                    .style(Style::default().fg(Color::Gray))
                    .bounds([x_start, x_end])
                    .labels(x_labels),
            )
            .y_axis(
                Axis::default()
                    .title(cy_title)
                    .style(Style::default().fg(Color::Gray))
                    .bounds([cy_min, cy_max])
                    .labels(vec![
                        Line::from(format!("{:.1}", cy_min)),
                        Line::from(format!("{:.1}", cy_mid)),
                        Line::from(format!("{:.1}", cy_max)),
                    ]),
            );
        temp_chart.render(temp_area, buf);

        // --- Right panel: Light chart ---
        let light_dataset = Dataset::default()
            .name("Lux")
            .style(Style::default().fg(Color::Cyan))
            .graph_type(GraphType::Line)
            .marker(Marker::Braille)
            .data(&lights);

        let ly_min = lights.iter().map(|(_, y)| *y).fold(f64::INFINITY, f64::min);
        let ly_max = lights
            .iter()
            .map(|(_, y)| *y)
            .fold(f64::NEG_INFINITY, f64::max);

        let (ly_min, ly_max) = if lights.is_empty() {
            (0.0, 1000.0)
        } else if (ly_max - ly_min).abs() < 1.0 {
            (ly_min - 5.0, ly_max + 5.0)
        } else {
            let padding = (ly_max - ly_min) * 0.1;
            (ly_min - padding, ly_max + padding)
        };

        let cur_lux = lights.last().map(|(_, v)| *v);
        let ly_title = cur_lux.map_or("lux".into(), |v| format!("{:.0} lux", v));

        let light_chart = Chart::new(vec![light_dataset])
            .block(
                Block::bordered()
                    .title("Light")
                    .border_type(BorderType::Rounded),
            )
            .x_axis(
                Axis::default()
                    .title("Time")
                    .style(Style::default().fg(Color::Gray))
                    .bounds([x_start, x_end])
                    .labels(Vec::<Line>::new()),
            )
            .y_axis(
                Axis::default()
                    .title(ly_title)
                    .style(Style::default().fg(Color::Gray))
                    .bounds([ly_min, ly_max])
                    .labels(vec![
                        Line::from(format!("{:.0}", ly_min)),
                        Line::from(format!("{:.0}", (ly_min + ly_max) / 2.0)),
                        Line::from(format!("{:.0}", ly_max)),
                    ]),
            );
        light_chart.render(light_area, buf);

        if self.log_state.enabled && outer_chunks.len() > 1 {
            let log_area = outer_chunks[1];
            let logs = self.log_state.logs();
            let log_widget = LogListWidget::new(&logs, "Logs", self.log_state.scroll);
            log_widget.render(log_area, buf);
        }
    }
}
