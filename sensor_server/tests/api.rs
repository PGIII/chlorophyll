//! HTTP-level tests for the JSON API and dashboard, exercised in-process via
//! `tower::ServiceExt::oneshot` (no socket needed).

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use chlorophyll_client::db::Db;
use chlorophyll_client::reading::{Reading, ReadingKind};
use chlorophyll_client::{ClientConfig, SensorClient};
use chrono::Utc;
use http_body_util::BodyExt;
use sensor_server::AppState;
use tower::ServiceExt;

/// Removes the sqlite db file (and its `-wal`/`-shm` siblings) when dropped.
struct TempDb(std::path::PathBuf);

impl Drop for TempDb {
    fn drop(&mut self) {
        for suffix in ["", "-wal", "-shm"] {
            let mut path = self.0.clone().into_os_string();
            path.push(suffix);
            let _ = std::fs::remove_file(path);
        }
    }
}

async fn test_state() -> (AppState, TempDb) {
    let path = std::env::temp_dir().join(format!("chlorophyll-test-{}.db", uuid_like()));
    let db = Db::open(path.to_str().unwrap()).await.unwrap();

    let now = Utc::now();
    db.insert_reading(&Reading { sensor_id: 1, kind: ReadingKind::Temperature, value: 21.5, at: now })
        .await
        .unwrap();
    db.insert_reading(&Reading { sensor_id: 1, kind: ReadingKind::Humidity, value: 55.0, at: now })
        .await
        .unwrap();
    db.insert_reading(&Reading { sensor_id: 1, kind: ReadingKind::Light, value: 123.0, at: now })
        .await
        .unwrap();

    // Use a non-default multicast port so this never collides with a real
    // chlorophyll network running on the dev machine; if the join fails the
    // listener thread just logs and exits, leaving an empty registry.
    let cfg = ClientConfig { port: 0, ..ClientConfig::default() };
    let client = Arc::new(SensorClient::start(cfg).unwrap());

    (AppState { client, db }, TempDb(path))
}

fn uuid_like() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{:x}-{:x}-{n:x}", std::process::id(), Utc::now().timestamp_nanos_opt().unwrap_or_default())
}

async fn body_string(response: axum::response::Response) -> String {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}

#[tokio::test]
async fn api_sensors_returns_json_array() {
    let (state, _db) = test_state().await;
    let router = sensor_server::router().with_state(state);

    let response = router.oneshot(Request::builder().uri("/api/sensors").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = body_string(response).await;
    let value: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(value.is_array());
}

#[tokio::test]
async fn api_sensors_history_since_zero_returns_seeded_readings() {
    let (state, _db) = test_state().await;
    let router = sensor_server::router().with_state(state);

    let response = router
        .oneshot(Request::builder().uri("/api/sensors/history?since=0").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = body_string(response).await;
    let series: serde_json::Value = serde_json::from_str(&body).unwrap();
    let series = series.as_array().unwrap();
    assert_eq!(series.len(), 3, "expected one series per metric: {body}");

    let metrics: Vec<&str> = series.iter().map(|s| s["metric"].as_str().unwrap()).collect();
    assert!(metrics.contains(&"temperature"));
    assert!(metrics.contains(&"humidity"));
    assert!(metrics.contains(&"light"));
}

#[tokio::test]
async fn dashboard_renders_table_and_charts() {
    let (state, _db) = test_state().await;
    let router = sensor_server::router().with_state(state);

    let response = router.oneshot(Request::builder().uri("/").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = body_string(response).await;
    assert!(body.contains("<table"), "expected a sensor table: {body}");
    assert!(body.contains("<svg"), "expected at least one chart svg: {body}");
}
