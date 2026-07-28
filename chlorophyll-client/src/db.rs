use chrono::{DateTime, Utc};
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::SqlitePool;

use crate::reading::{Reading, ReadingKind};

#[derive(Debug, Clone)]
pub struct Db(SqlitePool);

/// One point in a metric's history: `(timestamp, value)`.
pub type Point = (DateTime<Utc>, f32);

impl Db {
    /// Open (or create) the SQLite database at `path` and run migrations.
    pub async fn open(path: &str) -> anyhow::Result<Self> {
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

    pub async fn insert_reading(&self, reading: &Reading) -> anyhow::Result<()> {
        let sensor_id = format!("{:032x}", reading.sensor_id);
        let timestamp = reading.at.to_rfc3339();
        sqlx::query(
            "INSERT INTO readings (sensor_id, timestamp, data_type, value)
             VALUES (?, ?, ?, ?)",
        )
        .bind(&sensor_id)
        .bind(&timestamp)
        .bind(reading.kind.as_str())
        .bind(f64::from(reading.value))
        .execute(&self.0)
        .await?;
        Ok(())
    }

    /// Latest value for `(sensor_id, kind)`, if any reading has been stored.
    pub async fn latest(&self, sensor_id: u128, kind: ReadingKind) -> anyhow::Result<Option<Point>> {
        let sensor_id = format!("{sensor_id:032x}");
        let row = sqlx::query_as::<_, (String, f64)>(
            "SELECT timestamp, value FROM readings
             WHERE sensor_id = ? AND data_type = ?
             ORDER BY timestamp DESC LIMIT 1",
        )
        .bind(&sensor_id)
        .bind(kind.as_str())
        .fetch_optional(&self.0)
        .await?;

        row.map(|(ts, value)| parse_point(&ts, value)).transpose()
    }

    /// All `(sensor_id, kind)` series with at least one reading since `since`, as
    /// `(sensor_id_hex, kind, points)` ordered by timestamp ascending.
    pub async fn history_since(&self, since: DateTime<Utc>) -> anyhow::Result<Vec<(String, ReadingKind, Vec<Point>)>> {
        let rows = sqlx::query_as::<_, (String, String, String, f64)>(
            "SELECT sensor_id, data_type, timestamp, value FROM readings
             WHERE timestamp >= ?
             ORDER BY sensor_id, data_type, timestamp ASC",
        )
        .bind(since.to_rfc3339())
        .fetch_all(&self.0)
        .await?;

        let mut series: Vec<(String, ReadingKind, Vec<Point>)> = Vec::new();
        for (sensor_id, data_type, ts, value) in rows {
            let kind = match data_type.as_str() {
                "temperature" => ReadingKind::Temperature,
                "humidity" => ReadingKind::Humidity,
                "light" => ReadingKind::Light,
                other => return Err(anyhow::anyhow!("unknown data_type: {other}")),
            };
            let point = parse_point(&ts, value)?;
            match series.last_mut() {
                Some((last_id, last_kind, points)) if *last_id == sensor_id && *last_kind == kind => {
                    points.push(point);
                }
                _ => series.push((sensor_id, kind, vec![point])),
            }
        }
        Ok(series)
    }
}

fn parse_point(ts: &str, value: f64) -> anyhow::Result<Point> {
    let timestamp = ts.parse::<DateTime<Utc>>()?;
    #[allow(clippy::cast_possible_truncation)]
    Ok((timestamp, value as f32))
}
