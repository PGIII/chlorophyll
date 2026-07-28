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

#[tokio::test]
async fn dashboard_range_selects_window_and_falls_back_on_garbage() {
    let (state, _db) = test_state().await;
    let router = sensor_server::router().with_state(state);

    let response = router
        .clone()
        .oneshot(Request::builder().uri("/?range=7d").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_string(response).await;
    assert!(body.contains("last 7 days"), "expected the 7d window to be selected: {body}");
    assert!(
        body.contains(r#"href="/?range=30d""#),
        "expected every range to stay reachable: {body}"
    );

    // An unknown range must fall back to the default rather than erroring.
    let response = router
        .oneshot(Request::builder().uri("/?range=nonsense").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_string(response).await;
    assert!(body.contains("last 24 hours"), "expected fallback to the default window: {body}");
}

/// `/api/sensors/history` must keep routing to the history handler rather than being
/// swallowed by the `{id_hex}` route that shares its prefix.
#[tokio::test]
async fn history_path_is_not_captured_by_the_id_route() {
    let (state, _db) = test_state().await;
    let router = sensor_server::router().with_state(state);

    let response = router
        .oneshot(Request::builder().uri("/api/sensors/history?since=0").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let value: serde_json::Value = serde_json::from_str(&body_string(response).await).unwrap();
    let series = value.as_array().expect("history returns an array of series");
    assert!(
        series.iter().all(|s| s.get("metric").is_some()),
        "expected series objects, not a single sensor summary: {value}"
    );
}

#[tokio::test]
async fn single_sensor_endpoints_filter_and_404() {
    let (state, _db) = test_state().await;
    let router = sensor_server::router().with_state(state);
    let seeded = format!("{:032x}", 1_u128);

    let response = router
        .clone()
        .oneshot(Request::builder().uri(format!("/api/sensors/{seeded}/history?since=0")).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let value: serde_json::Value = serde_json::from_str(&body_string(response).await).unwrap();
    let series = value.as_array().unwrap();
    assert!(!series.is_empty(), "seeded sensor should have history: {value}");
    assert!(
        series.iter().all(|s| s["id_hex"] == seeded.as_str()),
        "history must be filtered to the requested sensor: {value}"
    );

    // A well-formed but unseen id is a 404, not an empty 200.
    let missing = format!("{:032x}", 999_u128);
    let response = router
        .clone()
        .oneshot(Request::builder().uri(format!("/api/sensors/{missing}")).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    // Garbage that isn't hex is a client error.
    let response = router
        .oneshot(Request::builder().uri("/api/sensors/zzzz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn history_bucket_is_bounded_even_for_a_huge_window() {
    let (state, _db) = test_state().await;
    let router = sensor_server::router().with_state(state);

    // Ten years back: the auto-bucket must widen rather than return a point per minute.
    let response = router
        .oneshot(Request::builder().uri("/api/sensors/history?since=0").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let value: serde_json::Value = serde_json::from_str(&body_string(response).await).unwrap();
    for s in value.as_array().unwrap() {
        let bucket = s["bucket_secs"].as_i64().unwrap();
        assert!(bucket >= 60, "bucket must not go below the stored resolution: {bucket}");
        let points = s["points"].as_array().unwrap().len();
        assert!(points <= 1000, "series should stay bounded, got {points} points");
    }
}

#[tokio::test]
async fn set_name_rejects_bad_input_before_touching_the_network() {
    let (state, _db) = test_state().await;
    let router = sensor_server::router().with_state(state);
    let seeded = format!("{:032x}", 1_u128);

    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/sensors/{seeded}/name"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"name":"   "}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST, "blank names are rejected");

    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/sensors/nothex/name")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"name":"kitchen"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST, "unparseable id is rejected");
}
