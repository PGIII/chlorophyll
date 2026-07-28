#![warn(clippy::pedantic)]

pub mod api;
pub mod dashboard;
pub mod state;
pub mod svg;

use axum::Router;
use axum::routing::get;

pub use state::AppState;

/// Combined router for the JSON API and HTML dashboard.
pub fn router() -> Router<AppState> {
    api::router()
        .merge(dashboard::router())
        .route("/healthz", get(|| async { "ok" }))
}
