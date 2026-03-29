use chlorophyll_protocol::light::Light;
use chlorophyll_protocol::temperature::Temperature;
use chlorophyll_protocol::DataType;
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::SqlitePool;

use crate::DataEntry;

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
}
