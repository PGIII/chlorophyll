use dioxus::prelude::*;

use crate::types::SensorSnapshot;
#[cfg(feature = "server")]
use crate::types::SensorRow;

/// Called once by the web client on mount to load all historical data from the DB.
#[server]
pub async fn get_history() -> Result<SensorSnapshot, ServerFnError> {
    use sensor_server::db::Db;

    let db_path =
        std::env::var("CHLOROPHYLL_DB").unwrap_or_else(|_| "chlorophyll.db".to_string());
    let db = Db::open(&db_path)
        .await
        .map_err(|e| ServerFnError::ServerError {
            message: e.to_string(),
            code: 500,
            details: None,
        })?;
    let entries = db.load_all().await.map_err(|e| ServerFnError::ServerError {
        message: e.to_string(),
        code: 500,
        details: None,
    })?;

    use chlorophyll_protocol::DataType;
    use chlorophyll_protocol::light::Light;
    use chlorophyll_protocol::temperature::Temperature;

    let temp_series = entries
        .iter()
        .filter_map(|e| {
            if let DataType::Temperature(t) = &e.data_type {
                Some((e.timestamp.timestamp(), t.get_as_f()))
            } else {
                None
            }
        })
        .collect();
    let humidity_series = entries
        .iter()
        .filter_map(|e| {
            if let DataType::RelativeHumidity(h) = &e.data_type {
                Some((e.timestamp.timestamp(), h.percent()))
            } else {
                None
            }
        })
        .collect();
    let light_series = entries
        .iter()
        .filter_map(|e| {
            if let DataType::Light(l) = &e.data_type {
                Some((e.timestamp.timestamp(), l.get_as_lux()))
            } else {
                None
            }
        })
        .collect();

    Ok(SensorSnapshot { sensors: vec![], temp_series, humidity_series, light_series })
}

/// Called by the web client every ~2 s to get readings newer than `since` (unix timestamp).
/// Returns current sensor state + only the series entries after the cursor.
#[server]
pub async fn get_snapshot(since: i64) -> Result<SensorSnapshot, ServerFnError> {
    use std::collections::HashMap;
    use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
    use std::sync::OnceLock;

    use chlorophyll_protocol::DataType;
    use chlorophyll_protocol::light::Light;
    use chlorophyll_protocol::temperature::Temperature;
    use chrono::Utc;
    use sensor_server::{DataEntry, MULTICAST_ADDR, PORT, REDISCOVER_TICKS, process_packets, send_discover};
    use tokio::sync::Mutex;

    struct State {
        socket: Option<tokio::net::UdpSocket>,
        known_devices: HashMap<u128, SocketAddr>,
        readings: Vec<DataEntry>,
        tick: u64,
        db: Option<sensor_server::db::Db>,
        db_initialized: bool,
    }

    static S: OnceLock<Mutex<State>> = OnceLock::new();
    let mtx = S.get_or_init(|| {
        Mutex::new(State {
            socket: None,
            known_devices: HashMap::new(),
            readings: Vec::new(),
            tick: 0,
            db: None,
            db_initialized: false,
        })
    });

    let mut s = mtx.lock().await;

    // Lazy DB initialisation: load history once on first call
    if !s.db_initialized {
        s.db_initialized = true;
        let db_path =
            std::env::var("CHLOROPHYLL_DB").unwrap_or_else(|_| "chlorophyll.db".to_string());
        match sensor_server::db::Db::open(&db_path).await {
            Ok(db) => {
                match db.load_all().await {
                    Ok(history) => {
                        tracing::info!("Loaded {} historical readings from {db_path}", history.len());
                        s.readings = history;
                    }
                    Err(e) => tracing::error!("Failed to load history: {e}"),
                }
                s.db = Some(db);
            }
            Err(e) => tracing::error!("Failed to open DB at {db_path}: {e}"),
        }
    }

    // Lazy socket initialisation
    if s.socket.is_none() {
        let addr = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, PORT);
        let sock = tokio::net::UdpSocket::bind(addr)
            .await
            .map_err(|e| ServerFnError::ServerError { message: e.to_string(), code: 500, details: None })?;
        sock.join_multicast_v4(MULTICAST_ADDR, Ipv4Addr::UNSPECIFIED)
            .map_err(|e| ServerFnError::ServerError { message: e.to_string(), code: 500, details: None })?;
        send_discover(&sock).await.ok();
        s.socket = Some(sock);
    }

    s.tick = s.tick.wrapping_add(1);

    if let Some(sock) = s.socket.take() {
        if s.tick % REDISCOVER_TICKS == 0 {
            send_discover(&sock).await.ok();
        }
        let mut devices = std::mem::take(&mut s.known_devices);
        let mut all_readings = std::mem::take(&mut s.readings);
        let prev_len = all_readings.len();
        process_packets(&sock, &mut devices, &mut all_readings).await.ok();
        if let Some(db) = &s.db {
            for entry in &all_readings[prev_len..] {
                if let Err(e) = db.insert_entry(entry).await {
                    tracing::error!("DB insert error: {e}");
                }
            }
        }
        s.known_devices = devices;
        s.readings = all_readings;
        s.socket = Some(sock);
    }

    // Build sensors from ALL readings (current state)
    let now = Utc::now();
    let mut seen_ids: std::collections::HashSet<u128> =
        s.readings.iter().map(|e| e.sensor_id).collect();
    seen_ids.extend(s.known_devices.keys());
    let mut sensors: Vec<SensorRow> = seen_ids
        .iter()
        .map(|&id| {
            let mut temp_f = None;
            let mut humidity_pct = None;
            let mut lux = None;
            let mut last_seen = chrono::DateTime::<Utc>::MIN_UTC;
            for entry in s.readings.iter().rev() {
                if entry.sensor_id != id { continue; }
                if entry.timestamp > last_seen { last_seen = entry.timestamp; }
                match &entry.data_type {
                    DataType::Temperature(t) if temp_f.is_none() => { temp_f = Some(t.get_as_f()); }
                    DataType::RelativeHumidity(h) if humidity_pct.is_none() => { humidity_pct = Some(h.percent()); }
                    DataType::Light(l) if lux.is_none() => { lux = Some(l.get_as_lux()); }
                    _ => {}
                }
                if temp_f.is_some() && humidity_pct.is_some() && lux.is_some() { break; }
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

    // Series: only entries newer than the client's cursor
    let temp_series = s
        .readings
        .iter()
        .filter_map(|e| {
            if let DataType::Temperature(t) = &e.data_type {
                let ts = e.timestamp.timestamp();
                if ts > since { Some((ts, t.get_as_f())) } else { None }
            } else {
                None
            }
        })
        .collect();
    let humidity_series = s
        .readings
        .iter()
        .filter_map(|e| {
            if let DataType::RelativeHumidity(h) = &e.data_type {
                let ts = e.timestamp.timestamp();
                if ts > since { Some((ts, h.percent())) } else { None }
            } else {
                None
            }
        })
        .collect();
    let light_series = s
        .readings
        .iter()
        .filter_map(|e| {
            if let DataType::Light(l) = &e.data_type {
                let ts = e.timestamp.timestamp();
                if ts > since { Some((ts, l.get_as_lux())) } else { None }
            } else {
                None
            }
        })
        .collect();

    Ok(SensorSnapshot { sensors, temp_series, humidity_series, light_series })
}
