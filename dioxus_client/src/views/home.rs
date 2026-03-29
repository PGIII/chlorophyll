use dioxus::prelude::*;

use crate::components::{ChartSeries, LineChart, SensorList};
use crate::types::{SensorRow, SensorSnapshot};

// ── Desktop ──────────────────────────────────────────────────────────────────
// Direct UDP networking via a background coroutine; no server process needed.

#[cfg(feature = "desktop")]
#[component]
pub fn Home() -> Element {
    use std::collections::HashMap;
    use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};

    use chlorophyll_protocol::DataType;
    use chlorophyll_protocol::light::Light;
    use chlorophyll_protocol::temperature::Temperature;
    use chrono::Utc;
    use sensor_server::{
        DataEntry, MULTICAST_ADDR, PORT, REDISCOVER_TICKS, process_packets, send_discover,
    };

    let mut readings: Signal<Vec<DataEntry>> = use_signal(Vec::new);
    let mut known_devices: Signal<HashMap<u128, SocketAddr>> = use_signal(HashMap::new);

    use_coroutine(move |_rx: UnboundedReceiver<()>| async move {
        let addr = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, PORT);
        let socket = match tokio::net::UdpSocket::bind(addr).await {
            Ok(s) => s,
            Err(e) => {
                tracing::error!("Failed to bind UDP socket on port {PORT}: {e}");
                return;
            }
        };
        if let Err(e) = socket.join_multicast_v4(MULTICAST_ADDR, Ipv4Addr::UNSPECIFIED) {
            tracing::error!("Failed to join multicast group: {e}");
            return;
        }
        send_discover(&socket).await.ok();

        let mut tick: u64 = 0;
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            if tick > 0 && tick % REDISCOVER_TICKS == 0 {
                send_discover(&socket).await.ok();
            }
            let mut local_devices = known_devices.read().clone();
            let mut new_readings: Vec<DataEntry> = Vec::new();
            process_packets(&socket, &mut local_devices, &mut new_readings).await.ok();
            if !new_readings.is_empty() {
                readings.write().extend(new_readings);
            }
            if local_devices.len() != known_devices.read().len() {
                *known_devices.write() = local_devices;
            }
            tick = tick.wrapping_add(1);
        }
    });

    // Compute snapshot from local signals on every render
    let snap = {
        let r = readings.read();
        let d = known_devices.read();
        let now = Utc::now();

        let mut seen_ids: std::collections::HashSet<u128> = r.iter().map(|e| e.sensor_id).collect();
        seen_ids.extend(d.keys());
        let mut sensors: Vec<SensorRow> = seen_ids
            .iter()
            .map(|&id| {
                let mut temp_f = None;
                let mut humidity_pct = None;
                let mut lux = None;
                let mut last_seen = chrono::DateTime::<Utc>::MIN_UTC;
                for entry in r.iter().rev() {
                    if entry.sensor_id != id {
                        continue;
                    }
                    if entry.timestamp > last_seen {
                        last_seen = entry.timestamp;
                    }
                    match &entry.data_type {
                        DataType::Temperature(t) if temp_f.is_none() => {
                            temp_f = Some(t.get_as_f());
                        }
                        DataType::RelativeHumidity(h) if humidity_pct.is_none() => {
                            humidity_pct = Some(h.percent());
                        }
                        DataType::Light(l) if lux.is_none() => {
                            lux = Some(l.get_as_lux());
                        }
                        _ => {}
                    }
                    if temp_f.is_some() && humidity_pct.is_some() && lux.is_some() {
                        break;
                    }
                }
                SensorRow {
                    id: format!("{id:016x}"),
                    temp_f,
                    humidity_pct,
                    lux,
                    age_secs: (now - last_seen).num_seconds().max(0),
                }
            })
            .collect();
        sensors.sort_by(|a, b| a.id.cmp(&b.id));

        SensorSnapshot {
            sensors,
            temp_series: r
                .iter()
                .filter_map(|e| {
                    if let DataType::Temperature(t) = &e.data_type {
                        Some((e.timestamp.timestamp(), t.get_as_f()))
                    } else {
                        None
                    }
                })
                .collect(),
            humidity_series: r
                .iter()
                .filter_map(|e| {
                    if let DataType::RelativeHumidity(h) = &e.data_type {
                        Some((e.timestamp.timestamp(), h.percent()))
                    } else {
                        None
                    }
                })
                .collect(),
            light_series: r
                .iter()
                .filter_map(|e| {
                    if let DataType::Light(l) = &e.data_type {
                        Some((e.timestamp.timestamp(), l.get_as_lux()))
                    } else {
                        None
                    }
                })
                .collect(),
        }
    };

    render_dashboard(&snap)
}

// ── Web ───────────────────────────────────────────────────────────────────────
// Polls the server function every 2 s; networking runs in the server process.

#[cfg(not(feature = "desktop"))]
#[component]
pub fn Home() -> Element {
    use crate::api::get_snapshot;

    #[allow(unused_mut)]
    let mut refresh = use_signal(|| 0u32);

    let snapshot = use_resource(move || {
        let _ = refresh(); // reactive dependency → reruns when refresh ticks
        async move { get_snapshot().await.unwrap_or_default() }
    });

    // Tick every 2 s using the browser timer (no tokio in WASM)
    #[cfg(feature = "web")]
    use_coroutine(move |_rx: UnboundedReceiver<()>| async move {
        loop {
            gloo_timers::future::sleep(std::time::Duration::from_secs(2)).await;
            *refresh.write() += 1;
        }
    });

    let guard = snapshot.read();
    match guard.as_ref() {
        Some(snap) => render_dashboard(snap),
        None => rsx! {
            div { class: "loading", "Connecting to sensor server…" }
        },
    }
}

// ── Shared render ─────────────────────────────────────────────────────────────

fn render_dashboard(snap: &SensorSnapshot) -> Element {
    let temp_pts: Vec<(f64, f64)> =
        snap.temp_series.iter().map(|(t, v)| (*t as f64, *v as f64)).collect();
    let hum_pts: Vec<(f64, f64)> =
        snap.humidity_series.iter().map(|(t, v)| (*t as f64, *v as f64)).collect();
    let lux_pts: Vec<(f64, f64)> =
        snap.light_series.iter().map(|(t, v)| (*t as f64, *v as f64)).collect();

    rsx! {
        div { class: "dashboard",
            div { class: "sidebar",
                SensorList { sensors: snap.sensors.clone() }
            }
            div { class: "charts",
                div { class: "chart",
                    LineChart {
                        series: vec![
                            ChartSeries { label: "Temp (°F)".to_string(), points: temp_pts, color: "#f59e0b".to_string() },
                            ChartSeries { label: "Humidity (%)".to_string(), points: hum_pts, color: "#3b82f6".to_string() },
                        ],
                        title: "Temperature & Humidity",
                    }
                }
                div { class: "chart",
                    LineChart {
                        series: vec![
                            ChartSeries { label: "Lux".to_string(), points: lux_pts, color: "#06b6d4".to_string() },
                        ],
                        title: "Light",
                    }
                }
            }
        }
    }
}
