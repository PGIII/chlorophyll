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
            let kind = parse_kind(&data_type)?;
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

    /// Downsampled history over `[from, to]`: one averaged point per `bucket_secs` window,
    /// per `(sensor_id, kind)`, ordered ascending.
    ///
    /// The sensors emit roughly five readings per second per metric, so the raw row count
    /// scales with the window (a day is >1M rows). Averaging in SQL keeps the result
    /// proportional to the chart's pixel width instead, which is what makes multi-day
    /// windows viable at all.
    pub async fn history_bucketed(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        bucket_secs: i64,
    ) -> anyhow::Result<Vec<(String, ReadingKind, Vec<Point>)>> {
        let bucket_secs = bucket_secs.max(1);
        let rows = sqlx::query_as::<_, (String, String, i64, f64)>(
            "SELECT sensor_id, data_type,
                    CAST(strftime('%s', timestamp) AS INTEGER) / ? AS bucket,
                    AVG(value)
             FROM readings
             WHERE timestamp >= ? AND timestamp <= ?
             GROUP BY sensor_id, data_type, bucket
             ORDER BY sensor_id, data_type, bucket ASC",
        )
        .bind(bucket_secs)
        .bind(from.to_rfc3339())
        .bind(to.to_rfc3339())
        .fetch_all(&self.0)
        .await?;

        let mut series: Vec<(String, ReadingKind, Vec<Point>)> = Vec::new();
        for (sensor_id, data_type, bucket, value) in rows {
            let kind = parse_kind(&data_type)?;
            let at = DateTime::from_timestamp(bucket * bucket_secs, 0)
                .ok_or_else(|| anyhow::anyhow!("bucket {bucket} out of range"))?;
            #[allow(clippy::cast_possible_truncation)]
            let point = (at, value as f32);
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

fn parse_kind(data_type: &str) -> anyhow::Result<ReadingKind> {
    match data_type {
        "temperature" => Ok(ReadingKind::Temperature),
        "humidity" => Ok(ReadingKind::Humidity),
        "light" => Ok(ReadingKind::Light),
        other => Err(anyhow::anyhow!("unknown data_type: {other}")),
    }
}

fn parse_point(ts: &str, value: f64) -> anyhow::Result<Point> {
    let timestamp = ts.parse::<DateTime<Utc>>()?;
    #[allow(clippy::cast_possible_truncation)]
    Ok((timestamp, value as f32))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two readings per bucket at a known epoch offset; each bucket should collapse to the
    /// mean of its members, which is what keeps multi-day windows chart-sized.
    #[tokio::test]
    async fn history_bucketed_averages_within_each_bucket() {
        let path = std::env::temp_dir().join(format!("chlorophyll-bucket-{}.db", std::process::id()));
        let db = Db::open(path.to_str().unwrap()).await.unwrap();

        let base = DateTime::from_timestamp(1_700_000_000, 0).unwrap();
        for (offset_secs, value) in [(0, 10.0), (30, 20.0), (60, 30.0), (90, 50.0)] {
            db.insert_reading(&Reading {
                sensor_id: 1,
                kind: ReadingKind::Temperature,
                value,
                at: base + chrono::Duration::seconds(offset_secs),
            })
            .await
            .unwrap();
        }

        let series = db
            .history_bucketed(base, base + chrono::Duration::seconds(120), 60)
            .await
            .unwrap();

        assert_eq!(series.len(), 1);
        let (_, kind, points) = &series[0];
        assert_eq!(*kind, ReadingKind::Temperature);
        assert_eq!(points.len(), 2, "4 readings across 2 buckets collapse to 2 points");
        assert!((points[0].1 - 15.0).abs() < 0.001, "got {}", points[0].1);
        assert!((points[1].1 - 40.0).abs() < 0.001, "got {}", points[1].1);
        assert!(points[0].0 < points[1].0);

        for suffix in ["", "-wal", "-shm"] {
            let mut p = path.clone().into_os_string();
            p.push(suffix);
            let _ = std::fs::remove_file(p);
        }
    }
}
