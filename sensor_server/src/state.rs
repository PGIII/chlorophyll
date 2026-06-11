use std::sync::Arc;

use chlorophyll_client::db::Db;
use chlorophyll_client::SensorClient;

/// Shared state for the HTTP API and dashboard: the live sensor registry plus
/// the SQLite-backed reading history.
#[derive(Clone)]
pub struct AppState {
    pub client: Arc<SensorClient>,
    pub db: Db,
}
