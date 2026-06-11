#![warn(clippy::pedantic)]

pub mod api;
pub mod dashboard;
pub mod state;
pub mod svg;

use axum::Router;

pub use state::AppState;

/// Combined router for the JSON API and HTML dashboard.
///
/// Callers should `.with_state(state)` and then `.merge(orbit_ui::assets_router())`
/// to add Orbit's shared static assets (which carry no state).
pub fn router() -> Router<AppState> {
    api::router().merge(dashboard::router())
}
