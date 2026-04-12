use dioxus::prelude::*;

const SVG_W: f64 = 800.0;
const SVG_H: f64 = 300.0;
const PL: f64 = 52.0;
const PR: f64 = 10.0;
const PT: f64 = 28.0;
const PB: f64 = 32.0;
const CW: f64 = SVG_W - PL - PR; // 738
const CH: f64 = SVG_H - PT - PB; // 240

/// Format a Unix timestamp (seconds) as `HH:MM` UTC.
fn fmt_time(ts: f64) -> String {
    let secs = ts as i64;
    let h = (secs % 86400) / 3600;
    let m = (secs % 3600) / 60;
    format!("{h:02}:{m:02}")
}

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
    let x_range = (max_x - min_x).max(1.0);

    let min_y_raw = all_pts.iter().map(|(_, y)| *y).fold(f64::INFINITY, f64::min);
    let max_y_raw = all_pts.iter().map(|(_, y)| *y).fold(f64::NEG_INFINITY, f64::max);
    let mid_y = (min_y_raw + max_y_raw) / 2.0;
    let y_rng = (max_y_raw - min_y_raw).max(mid_y.abs() * 0.10).max(1.0);
    let min_y = mid_y - y_rng / 2.0 - y_rng * 0.1;
    let max_y = mid_y + y_rng / 2.0 + y_rng * 0.1;

    // zoom: fraction of full x_range visible (1.0 = all data, 0.01 = 1%)
    let mut zoom = use_signal(|| 1.0_f64);
    // time_offset: seconds relative to max_x for the right edge (≤ 0).
    // Default 0 = always follow the live right edge → auto-scrolls as data arrives.
    let mut time_offset = use_signal(|| 0.0_f64);
    // CSS pixel width of the wrapper div (from onmounted, for coordinate conversion)
    let mut css_width = use_signal(|| SVG_W);
    // drag: Some(last element-relative x in CSS pixels) while mouse is held
    let mut drag_x: Signal<Option<f64>> = use_signal(|| None);
    // normalized mouse x in data area [0,1], used to zoom toward cursor
    let mut mouse_norm = use_signal(|| 0.5_f64);

    // ── Visible window ────────────────────────────────────────────────────────
    let z = zoom();
    let vis_range = x_range * z;
    // vis_max tracks the live right edge unless the user has panned left
    let vis_max = (max_x + time_offset()).min(max_x);
    let vis_min = (vis_max - vis_range).max(min_x);
    let vis_range_act = (vis_max - vis_min).max(1.0);

    let tx = |t: f64| PL + (t - vis_min) / vis_range_act * CW;
    let ty = |v: f64| PT + (1.0 - (v - min_y) / (max_y - min_y)) * CH;

    // ── Ticks ─────────────────────────────────────────────────────────────────
    let y_ticks: Vec<(f64, String)> = (0..=4)
        .map(|i| {
            let v = min_y + (max_y - min_y) * i as f64 / 4.0;
            (ty(v), format!("{v:.0}"))
        })
        .collect();

    let grid_ys: Vec<f64> = (0..=4)
        .map(|i| ty(min_y + (max_y - min_y) * i as f64 / 4.0))
        .collect();

    let x_ticks: Vec<(f64, String)> = (0..=4)
        .map(|i| {
            let t = vis_min + vis_range_act * i as f64 / 4.0;
            (tx(t), fmt_time(t))
        })
        .collect();

    // ── Polylines (smooth + clip to visible window with a small buffer) ───────
    let buf = vis_range_act * 0.05;
    let polylines: Vec<(String, String)> = series
        .iter()
        .map(|s| {
            let visible: Vec<(f64, f64)> = s
                .points
                .iter()
                .filter(|(t, _)| *t >= vis_min - buf && *t <= vis_max + buf)
                .copied()
                .collect();
            if visible.len() < 2 {
                return (String::new(), s.color.clone());
            }
            let w = 7_usize;
            let smoothed: Vec<(f64, f64)> = visible
                .iter()
                .enumerate()
                .map(|(i, (t, _))| {
                    let lo = i.saturating_sub(w / 2);
                    let hi = (i + w / 2 + 1).min(visible.len());
                    let avg =
                        visible[lo..hi].iter().map(|(_, v)| v).sum::<f64>() / (hi - lo) as f64;
                    (*t, avg)
                })
                .collect();
            let pts = smoothed
                .iter()
                .map(|(t, v)| format!("{:.1},{:.1}", tx(*t), ty(*v)))
                .collect::<Vec<_>>()
                .join(" ");
            (pts, s.color.clone())
        })
        .filter(|(pts, _)| !pts.is_empty())
        .collect();

    // ── Event handlers ────────────────────────────────────────────────────────

    let onmounted = move |e: MountedEvent| {
        spawn(async move {
            if let Ok(rect) = e.get_client_rect().await {
                css_width.set(rect.size.width);
            }
        });
    };

    // Zoom toward cursor. dy>0 = scroll down = zoom out.
    let onwheel = move |evt: WheelEvent| {
        evt.prevent_default();
        let dy = evt.delta().strip_units().y;
        let factor = if dy > 0.0 { 1.25_f64 } else { 1.0 / 1.25 };

        let old_z = zoom();
        let new_z = (old_z * factor).clamp(0.005, 1.0);
        let old_vis = x_range * old_z;
        let new_vis = x_range * new_z;
        let mn = mouse_norm();

        // Keep the timestamp under the cursor fixed while zooming
        let old_vis_max = (max_x + time_offset()).min(max_x);
        let old_vis_min = (old_vis_max - old_vis).max(min_x);
        let t_cursor = old_vis_min + mn * (old_vis_max - old_vis_min).max(1.0);

        let new_vis_max = t_cursor + (1.0 - mn) * new_vis;
        let new_offset = (new_vis_max - max_x).clamp(-x_range, 0.0);

        zoom.set(new_z);
        time_offset.set(new_offset);
    };

    let onmousemove = move |evt: MouseEvent| {
        let ex = evt.element_coordinates().x;
        let w = css_width();
        // Convert CSS px → SVG units → normalize over data area
        let svg_x = ex * (SVG_W / w);
        mouse_norm.set(((svg_x - PL) / CW).clamp(0.0, 1.0));

        if let Some(last) = drag_x() {
            let delta_css = ex - last;
            let data_area_css = w * CW / SVG_W;
            let delta_t = -(delta_css / data_area_css) * vis_range_act;
            // Shift the view window; clamp so we can't scroll past the data
            let new_offset = (time_offset() + delta_t).clamp(-x_range, 0.0);
            time_offset.set(new_offset);
            drag_x.set(Some(ex));
        }
    };

    let onmousedown = move |evt: MouseEvent| {
        drag_x.set(Some(evt.element_coordinates().x));
    };
    let onmouseup = move |_: MouseEvent| drag_x.set(None);
    let onmouseleave = move |_: MouseEvent| drag_x.set(None);

    let cursor = if drag_x().is_some() { "grabbing" } else { "crosshair" };

    rsx! {
        div {
            style: "width: 100%; height: 100%; overflow: hidden; cursor: {cursor};",
            onmounted,
            onwheel,
            onmousemove,
            onmousedown,
            onmouseup,
            onmouseleave,

            svg {
                class: "line-chart",
                width: "100%",
                height: "100%",
                view_box: "0 0 {SVG_W} {SVG_H}",
                preserve_aspect_ratio: "none",

                // Background
                rect { x: "0", y: "0", width: "{SVG_W}", height: "{SVG_H}", fill: "#0a0d13" }

                // Title
                text {
                    x: "{PL}",
                    y: "16",
                    fill: "#d1d5db",
                    font_size: "12",
                    font_family: "monospace",
                    font_weight: "600",
                    "{title}"
                }

                // Zoom hint (only when zoomed in)
                if zoom() < 0.99 {
                    text {
                        x: "{SVG_W - PR - 2.0}",
                        y: "16",
                        fill: "#4b5563",
                        font_size: "9",
                        font_family: "monospace",
                        text_anchor: "end",
                        "scroll to zoom · drag to pan"
                    }
                }

                // Legend chips
                for (i, s) in series.iter().enumerate() {
                    rect {
                        x: "{PL + 170.0 + i as f64 * 110.0}",
                        y: "9",
                        width: "14",
                        height: "3",
                        rx: "1",
                        fill: "{s.color}",
                    }
                    text {
                        x: "{PL + 188.0 + i as f64 * 110.0}",
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
                        x1: "{PL}", y1: "{gy}",
                        x2: "{PL + CW}", y2: "{gy}",
                        stroke: "#1e2230", stroke_width: "1",
                    }
                }

                // Vertical grid lines + X-axis tick labels
                for (xpos, label) in x_ticks.iter() {
                    line {
                        x1: "{xpos}", y1: "{PT}",
                        x2: "{xpos}", y2: "{PT + CH}",
                        stroke: "#1e2230", stroke_width: "1",
                    }
                    text {
                        x: "{xpos}",
                        y: "{PT + CH + 16.0}",
                        fill: "#6b7280",
                        font_size: "9",
                        font_family: "monospace",
                        text_anchor: "middle",
                        "{label}"
                    }
                }

                // Y-axis tick labels
                for (ypos, label) in y_ticks.iter() {
                    text {
                        x: "{PL - 4.0}",
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
                    x1: "{PL}", y1: "{PT}",
                    x2: "{PL}", y2: "{PT + CH}",
                    stroke: "#374151", stroke_width: "1",
                }
                line {
                    x1: "{PL}", y1: "{PT + CH}",
                    x2: "{PL + CW}", y2: "{PT + CH}",
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
}
