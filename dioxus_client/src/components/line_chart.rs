use dioxus::prelude::*;

#[derive(Clone, PartialEq)]
pub struct ChartSeries {
    pub label: String,
    pub points: Vec<(f64, f64)>,
    pub color: String,
}

#[component]
pub fn LineChart(series: Vec<ChartSeries>, title: String) -> Element {
    let all_pts: Vec<&(f64, f64)> = series.iter().flat_map(|s| s.points.iter()).collect();

    if all_pts.is_empty() {
        return rsx! {
            div { class: "chart-empty",
                span { class: "chart-title", "{title}" }
                span { class: "dim", "Waiting for data…" }
            }
        };
    }

    let min_x = all_pts.iter().map(|(x, _)| *x).fold(f64::INFINITY, f64::min);
    let max_x = all_pts.iter().map(|(x, _)| *x).fold(f64::NEG_INFINITY, f64::max);
    let min_y = all_pts.iter().map(|(_, y)| *y).fold(f64::INFINITY, f64::min);
    let max_y = all_pts.iter().map(|(_, y)| *y).fold(f64::NEG_INFINITY, f64::max);

    let y_range = (max_y - min_y).max(1.0);
    let min_y = min_y - y_range * 0.1;
    let max_y = max_y + y_range * 0.1;
    let x_range = (max_x - min_x).max(1.0);

    // Fixed SVG coordinate space; rendered responsively via viewBox
    let w = 800.0_f64;
    let h = 180.0_f64;
    let pl = 52.0_f64; // pad left  (y-axis labels)
    let pr = 10.0_f64; // pad right
    let pt = 28.0_f64; // pad top   (title + legend)
    let pb = 20.0_f64; // pad bottom
    let cw = w - pl - pr;
    let ch = h - pt - pb;

    let tx = |t: f64| pl + (t - min_x) / x_range * cw;
    let ty = |v: f64| pt + (1.0 - (v - min_y) / (max_y - min_y)) * ch;

    // Y-axis tick labels (5 ticks)
    let y_ticks: Vec<(f64, String)> = (0..=4)
        .map(|i| {
            let v = min_y + (max_y - min_y) * i as f64 / 4.0;
            (ty(v), format!("{v:.0}"))
        })
        .collect();

    // Grid line y-positions
    let grid_ys: Vec<f64> = (0..=4)
        .map(|i| ty(min_y + (max_y - min_y) * i as f64 / 4.0))
        .collect();

    // Precompute polyline point strings
    let polylines: Vec<(String, String)> = series
        .iter()
        .filter(|s| s.points.len() >= 2)
        .map(|s| {
            let pts = s
                .points
                .iter()
                .map(|(t, v)| format!("{:.1},{:.1}", tx(*t), ty(*v)))
                .collect::<Vec<_>>()
                .join(" ");
            (pts, s.color.clone())
        })
        .collect();

    rsx! {
        svg {
            class: "line-chart",
            width: "100%",
            view_box: "0 0 {w} {h}",

            // Background
            rect { x: "0", y: "0", width: "{w}", height: "{h}", fill: "#0a0d13" }

            // Title
            text {
                x: "{pl}",
                y: "16",
                fill: "#d1d5db",
                font_size: "12",
                font_family: "monospace",
                font_weight: "600",
                "{title}"
            }

            // Legend chips
            for (i, s) in series.iter().enumerate() {
                rect {
                    x: "{pl + 170.0 + i as f64 * 110.0}",
                    y: "9",
                    width: "14",
                    height: "3",
                    rx: "1",
                    fill: "{s.color}",
                }
                text {
                    x: "{pl + 188.0 + i as f64 * 110.0}",
                    y: "16",
                    fill: "{s.color}",
                    font_size: "10",
                    font_family: "monospace",
                    "{s.label}"
                }
            }

            // Horizontal grid lines
            for gy in grid_ys.iter() {
                line {
                    x1: "{pl}", y1: "{gy}",
                    x2: "{pl + cw}", y2: "{gy}",
                    stroke: "#1e2230", stroke_width: "1",
                }
            }

            // Y-axis tick labels
            for (ypos, label) in y_ticks.iter() {
                text {
                    x: "{pl - 4.0}",
                    y: "{ypos + 4.0}",
                    fill: "#6b7280",
                    font_size: "9",
                    font_family: "monospace",
                    text_anchor: "end",
                    "{label}"
                }
            }

            // Axes
            line {
                x1: "{pl}", y1: "{pt}",
                x2: "{pl}", y2: "{pt + ch}",
                stroke: "#374151", stroke_width: "1",
            }
            line {
                x1: "{pl}", y1: "{pt + ch}",
                x2: "{pl + cw}", y2: "{pt + ch}",
                stroke: "#374151", stroke_width: "1",
            }

            // Data lines
            for (pts, color) in polylines.iter() {
                polyline {
                    points: "{pts}",
                    fill: "none",
                    stroke: "{color}",
                    stroke_width: "1.5",
                    stroke_linejoin: "round",
                    stroke_linecap: "round",
                }
            }
        }
    }
}
