use chlorophyll_protocol::humidity::RelativeHumidity;
use chlorophyll_protocol::light::{Light, Lux};
use chlorophyll_protocol::temperature::{Celsius, Temperature};
use chlorophyll_protocol::DataType;
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::SqlitePool;

use crate::DataEntry;

#[derive(Debug)]
pub struct Db(SqlitePool);

impl Db {
    /// Open (or create) the SQLite database at `path` and run migrations.
    pub async fn open(path: &str) -> color_eyre::Result<Self> {
        let pool = SqlitePoolOptions::new()
            .connect(&format!("sqlite:{path}?mode=rwc"))
            .await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS readings (
                 id        INTEGER PRIMARY KEY AUTOINCREMENT,
                 sensor_id TEXT    NOT NULL,
                 timestamp TEXT    NOT NULL,
                 data_type TEXT    NOT NULL,
                 value     REAL    NOT NULL
             );
             CREATE INDEX IF NOT EXISTS idx_readings_sensor_timestamp
                 ON readings (sensor_id, timestamp);",
        )
        .execute(&pool)
        .await?;
        Ok(Self(pool))
    }

    pub async fn insert_entry(&self, entry: &DataEntry) -> color_eyre::Result<()> {
        let (data_type, value) = match &entry.data_type {
            DataType::Temperature(t) => ("temperature", f64::from(t.get_as_c())),
            DataType::RelativeHumidity(h) => ("humidity", f64::from(h.percent())),
            DataType::Light(l) => ("light", f64::from(l.get_as_lux())),
        };
        let sensor_id = format!("{:032x}", entry.sensor_id);
        let timestamp = entry.timestamp.to_rfc3339();
        sqlx::query(
            "INSERT INTO readings (sensor_id, timestamp, data_type, value)
             VALUES (?, ?, ?, ?)",
        )
        .bind(&sensor_id)
        .bind(&timestamp)
        .bind(data_type)
        .bind(value)
        .execute(&self.0)
        .await?;
        Ok(())
    }

    /// Load all stored readings ordered by timestamp ascending.
    pub async fn load_all(&self) -> color_eyre::Result<Vec<DataEntry>> {
        let rows = sqlx::query_as::<_, (String, String, String, f64)>(
            "SELECT sensor_id, timestamp, data_type, value FROM readings ORDER BY timestamp ASC",
        )
        .fetch_all(&self.0)
        .await?;

        let mut entries = Vec::with_capacity(rows.len());
        for (sensor_id_hex, timestamp_str, data_type_str, value) in rows {
            let sensor_id = u128::from_str_radix(&sensor_id_hex, 16)
                .map_err(|e| color_eyre::eyre::eyre!("invalid sensor_id hex: {e}"))?;
            let timestamp = timestamp_str
                .parse::<chrono::DateTime<chrono::Utc>>()
                .map_err(|e| color_eyre::eyre::eyre!("invalid timestamp: {e}"))?;
            #[allow(clippy::cast_possible_truncation)]
            let data_type = match data_type_str.as_str() {
                "temperature" => DataType::Temperature(Celsius::new(value as f32)),
                "humidity" => DataType::RelativeHumidity(RelativeHumidity::new(value as f32)),
                "light" => DataType::Light(Lux::new(value as f32)),
                other => {
                    return Err(color_eyre::eyre::eyre!("unknown data_type: {other}"));
                }
            };
            entries.push(DataEntry { data_type, sensor_id, timestamp });
        }
        Ok(entries)
    }
}
