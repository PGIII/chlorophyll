//! Server-rendered inline SVG line charts for sensor history.

use std::fmt::Write;

use chlorophyll_client::db::Point;
use chrono::{DateTime, Utc};

const WIDTH: f64 = 600.0;
const HEIGHT: f64 = 160.0;
const PAD_LEFT: f64 = 36.0;
const PAD_RIGHT: f64 = 8.0;
const PAD_TOP: f64 = 8.0;
const PAD_BOTTOM: f64 = 20.0;

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
pub fn line_chart(series: &[Series], from: DateTime<Utc>, to: DateTime<Utc>, unit_suffix: &str) -> Option<String> {
    let all_values: Vec<f32> = series.iter().flat_map(|s| s.points.iter().map(|(_, v)| *v)).collect();
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
    let _ = write!(svg, r#"<svg viewBox="0 0 {WIDTH} {HEIGHT}" class="w-full h-auto" preserveAspectRatio="none" role="img">"#);

    // Y-axis labels (top, middle, bottom).
    for (frac, value) in [(0.0, max_v), (0.5, f32::midpoint(min_v, max_v)), (1.0, min_v)] {
        let y = PAD_TOP + frac * plot_h;
        let _ = write!(
            svg,
            r#"<text x="2" y="{:.1}" class="fill-night-owl-text/40 text-[8px] font-mono">{:.0}{unit_suffix}</text>"#,
            y + 3.0,
            value,
        );
    }

    // Baseline axis.
    let _ = write!(
        svg,
        r#"<line x1="{PAD_LEFT}" y1="{:.1}" x2="{:.1}" y2="{:.1}" class="stroke-white/10" stroke-width="1" />"#,
        PAD_TOP + plot_h,
        WIDTH - PAD_RIGHT,
        PAD_TOP + plot_h,
    );

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
        let _ = write!(
            svg,
            r#"<path d="{path}" fill="none" stroke="{}" stroke-width="1.5" stroke-linejoin="round" stroke-linecap="round"><title>{}</title></path>"#,
            s.color, s.label,
        );
    }

    svg.push_str("</svg>");
    Some(svg)
}

/// Stable color for a sensor, cycling through a small accessible palette.
#[must_use]
pub fn series_color(index: usize) -> &'static str {
    const PALETTE: &[&str] = &["#38cfe6", "#f59e0b", "#34d399", "#f472b6", "#a78bfa", "#fb7185"];
    PALETTE[index % PALETTE.len()]
}
