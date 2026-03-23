use dioxus::prelude::*;

use crate::types::SensorRow;

#[component]
pub fn SensorList(sensors: Vec<SensorRow>) -> Element {
    rsx! {
        div { class: "sensor-list",
            h2 { class: "section-title", "Sensors" }
            if sensors.is_empty() {
                div { class: "no-sensors", "Discovering sensors…" }
            } else {
                table { class: "sensor-table",
                    thead {
                        tr {
                            th { "ID" }
                            th { "Temp" }
                            th { "Humidity" }
                            th { "Light" }
                            th { "Age" }
                        }
                    }
                    tbody {
                        for sensor in sensors {
                            tr {
                                td { class: "sensor-id", "{sensor.id}" }
                                td {
                                    match sensor.temp_f {
                                        Some(t) => rsx! { "{t:.1}°F" },
                                        None => rsx! { span { class: "dim", "--°F" } },
                                    }
                                }
                                td {
                                    match sensor.humidity_pct {
                                        Some(h) => rsx! { "{h:.1}%" },
                                        None => rsx! { span { class: "dim", "--%"} },
                                    }
                                }
                                td {
                                    match sensor.lux {
                                        Some(l) => rsx! { "{l:.0}lx" },
                                        None => rsx! { span { class: "dim", "--lx" } },
                                    }
                                }
                                td { class: "dim", "{format_age(sensor.age_secs)}" }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn format_age(secs: i64) -> String {
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else {
        format!("{}h", secs / 3600)
    }
}
