//! Server-rendered inline SVG line charts for sensor history.

use std::fmt::Write;

use chlorophyll_client::db::Point;
use chrono::{DateTime, Utc};

const WIDTH: f64 = 1000.0;
const HEIGHT: f64 = 300.0;
const PAD_LEFT: f64 = 52.0;
const PAD_RIGHT: f64 = 16.0;
const PAD_TOP: f64 = 14.0;
const PAD_BOTTOM: f64 = 30.0;

/// A named series of points to plot as one polyline.
pub struct Series<'a> {
    pub label: &'a str,
    pub color: &'a str,
    pub points: &'a [Point],
}

/// Render an inline SVG line chart for `series` spanning `[from, to]`.
///
/// `unit_suffix` is appended to the y-axis labels (e.g. `"°C"`, `"%"`, `"lux"`).
/// Returns `None` if every series is empty (nothing to plot).
#[must_use]
pub fn line_chart(
    series: &[Series],
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    unit_suffix: &str,
) -> Option<String> {
    let all_values: Vec<f32> = series
        .iter()
        .flat_map(|s| s.points.iter().map(|(_, v)| *v))
        .collect();
    if all_values.is_empty() {
        return None;
    }

    let min_v = all_values.iter().copied().fold(f32::INFINITY, f32::min);
    let max_v = all_values.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    // Avoid a zero-height range when all values are identical.
    let (min_v, max_v) = if (max_v - min_v).abs() < f32::EPSILON {
        (min_v - 1.0, max_v + 1.0)
    } else {
        (min_v, max_v)
    };

    #[allow(clippy::cast_precision_loss)]
    let span_ms = (to - from).num_milliseconds().max(1) as f64;
    let plot_w = WIDTH - PAD_LEFT - PAD_RIGHT;
    let plot_h = HEIGHT - PAD_TOP - PAD_BOTTOM;

    let x_for = |t: DateTime<Utc>| -> f64 {
        #[allow(clippy::cast_precision_loss)]
        let elapsed_ms = (t - from).num_milliseconds() as f64;
        let frac = elapsed_ms / span_ms;
        PAD_LEFT + frac.clamp(0.0, 1.0) * plot_w
    };
    let y_for = |v: f32| -> f64 {
        let frac = f64::from(v - min_v) / f64::from(max_v - min_v);
        PAD_TOP + (1.0 - frac.clamp(0.0, 1.0)) * plot_h
    };

    let mut svg = String::new();
    let _ = write!(
        svg,
        r#"<svg viewBox="0 0 {WIDTH} {HEIGHT}" class="chart" preserveAspectRatio="none" role="img">"#
    );

    // Horizontal gridlines with y-axis labels (top, quarters, middle, bottom).
    for step in 0..=4 {
        let frac = f64::from(step) / 4.0;
        let y = PAD_TOP + frac * plot_h;
        let value = f64::from(max_v) - (f64::from(max_v) - f64::from(min_v)) * frac;
        let _ = write!(
            svg,
            r#"<line x1="{PAD_LEFT}" y1="{y:.1}" x2="{:.1}" y2="{y:.1}" class="chart-grid" stroke-width="1" />"#,
            WIDTH - PAD_RIGHT,
        );
        let _ = write!(
            svg,
            r#"<text x="{:.1}" y="{:.1}" class="chart-label" text-anchor="end">{value:.1}{unit_suffix}</text>"#,
            PAD_LEFT - 8.0,
            y + 4.0,
        );
    }

    // Baseline axis.
    let _ = write!(
        svg,
        r#"<line x1="{PAD_LEFT}" y1="{:.1}" x2="{:.1}" y2="{:.1}" class="chart-axis" stroke-width="1" />"#,
        PAD_TOP + plot_h,
        WIDTH - PAD_RIGHT,
        PAD_TOP + plot_h,
    );

    // X-axis time labels; the format widens with the window so multi-day ranges stay legible.
    let span_minutes = (to - from).num_minutes();
    let time_fmt = if span_minutes > 72 * 60 {
        "%b %-d"
    } else if span_minutes > 24 * 60 {
        "%b %-d %H:%M"
    } else {
        "%H:%M"
    };
    for step in 0..=4 {
        let frac = f64::from(step) / 4.0;
        let at = from + (to - from) * step / 4;
        let anchor = match step {
            0 => "start",
            4 => "end",
            _ => "middle",
        };
        let _ = write!(
            svg,
            r#"<text x="{:.1}" y="{:.1}" class="chart-label" text-anchor="{anchor}">{}</text>"#,
            PAD_LEFT + frac * plot_w,
            HEIGHT - 10.0,
            at.format(time_fmt),
        );
    }

    for s in series {
        if s.points.is_empty() {
            continue;
        }
        let mut path = String::from("M ");
        for (i, (at, value)) in s.points.iter().enumerate() {
            if i > 0 {
                path.push_str(" L ");
            }
            let _ = write!(path, "{:.1} {:.1}", x_for(*at), y_for(*value));
        }
        let label = escape_xml_text(s.label);
        let _ = write!(
            svg,
            r#"<path d="{path}" fill="none" stroke="{}" stroke-width="2" stroke-linejoin="round" stroke-linecap="round"><title>{}</title></path>"#,
            s.color, label,
        );
    }

    svg.push_str("</svg>");
    Some(svg)
}

fn escape_xml_text(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

/// Stable color for a sensor, cycling through a small accessible palette.
#[must_use]
pub fn series_color(index: usize) -> &'static str {
    const PALETTE: &[&str] = &[
        "#6ee7a0", "#e8c05a", "#4fbfa4", "#d9825f", "#a3d977", "#c98bd9",
    ];
    PALETTE[index % PALETTE.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_chart_escapes_series_labels() {
        let from = Utc::now();
        let points = [(from, 1.0)];
        let series = [Series {
            label: r#"</title><script>alert('xss')</script><title>"#,
            color: "#000",
            points: &points,
        }];

        let chart =
            line_chart(&series, from, from + chrono::Duration::seconds(1), "").expect("chart");

        assert!(!chart.contains("<script>"));
        assert!(chart.contains(
            "&lt;/title&gt;&lt;script&gt;alert(&#39;xss&#39;)&lt;/script&gt;&lt;title&gt;"
        ));
    }
}
