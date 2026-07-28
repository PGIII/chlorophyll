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

        // Resolution of each row, in seconds; 0 marks pre-rollup rows written at the raw
        // sensor rate. Compaction uses it to tell which rows still need coarsening.
        // ALTER fails once the column exists, which is the normal case after first run.
        let _ = sqlx::query("ALTER TABLE readings ADD COLUMN bucket_secs INTEGER NOT NULL DEFAULT 0")
            .execute(&pool)
            .await;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_readings_bucket ON readings (bucket_secs, timestamp)")
            .execute(&pool)
            .await?;

        Ok(Self(pool))
    }

    pub async fn insert_reading(&self, reading: &Reading) -> anyhow::Result<()> {
        self.insert_reading_at(reading, 0).await
    }

    /// Insert a reading already averaged over a `bucket_secs`-wide window.
    pub async fn insert_reading_at(&self, reading: &Reading, bucket_secs: i64) -> anyhow::Result<()> {
        let sensor_id = format!("{:032x}", reading.sensor_id);
        let timestamp = reading.at.to_rfc3339();
        sqlx::query(
            "INSERT INTO readings (sensor_id, timestamp, data_type, value, bucket_secs)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&sensor_id)
        .bind(&timestamp)
        .bind(reading.kind.as_str())
        .bind(f64::from(reading.value))
        .bind(bucket_secs)
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

/// One rung of the retention ladder: rows older than `older_than_secs` are averaged down
/// to at most one point per `bucket_secs`.
#[derive(Debug, Clone, Copy)]
pub struct RetentionTier {
    pub older_than_secs: i64,
    pub bucket_secs: i64,
}

/// Progressive retention, finest first. Storage per sensor settles around 47k rows
/// instead of growing without bound at ~1.3M rows/day.
///
/// The first rung exists to collapse pre-rollup rows (`bucket_secs = 0`, written at the
/// raw ~5 Hz sensor rate) down to the ingest resolution.
pub const DEFAULT_TIERS: &[RetentionTier] = &[
    RetentionTier { older_than_secs: 3_600, bucket_secs: 60 },           // > 1h   -> 1 min
    RetentionTier { older_than_secs: 172_800, bucket_secs: 300 },        // > 48h  -> 5 min
    RetentionTier { older_than_secs: 1_209_600, bucket_secs: 3_600 },    // > 14d  -> 1 hour
    RetentionTier { older_than_secs: 31_536_000, bucket_secs: 86_400 },  // > 1y   -> 1 day
];

impl Db {
    /// Apply `tiers` as of `now`, returning how many rows were removed.
    ///
    /// Tiers are applied coarsest first so an old row is rewritten once rather than once
    /// per rung. Each pass is idempotent: rows already at or beyond a rung's resolution
    /// fail its `bucket_secs <` predicate and are skipped.
    pub async fn compact(
        &self,
        now: DateTime<Utc>,
        tiers: &[RetentionTier],
    ) -> anyhow::Result<u64> {
        let mut removed = 0;

        for tier in tiers.iter().rev() {
            let cutoff = (now - chrono::Duration::seconds(tier.older_than_secs)).to_rfc3339();
            let bucket = tier.bucket_secs.max(1);

            let mut tx = self.0.begin().await?;

            // Write the averaged replacements first. They carry the tier's bucket_secs, so
            // the DELETE below — which only matches strictly finer rows — cannot reap them.
            sqlx::query(
                "INSERT INTO readings (sensor_id, timestamp, data_type, value, bucket_secs)
                 SELECT sensor_id,
                        strftime('%Y-%m-%dT%H:%M:%S+00:00',
                                 CAST(strftime('%s', timestamp) AS INTEGER) / ?1 * ?1,
                                 'unixepoch'),
                        data_type,
                        AVG(value),
                        ?1
                 FROM readings
                 WHERE timestamp < ?2 AND bucket_secs < ?1
                 GROUP BY sensor_id, data_type,
                          CAST(strftime('%s', timestamp) AS INTEGER) / ?1",
            )
            .bind(bucket)
            .bind(&cutoff)
            .execute(&mut *tx)
            .await?;

            let deleted = sqlx::query(
                "DELETE FROM readings WHERE timestamp < ?2 AND bucket_secs < ?1",
            )
            .bind(bucket)
            .bind(&cutoff)
            .execute(&mut *tx)
            .await?
            .rows_affected();

            tx.commit().await?;
            removed += deleted;
        }

        Ok(removed)
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

#[cfg(test)]
mod compaction_tests {
    use super::*;

    async fn temp_db(tag: &str) -> (Db, std::path::PathBuf) {
        let path = std::env::temp_dir().join(format!(
            "chlorophyll-compact-{}-{tag}.db",
            std::process::id()
        ));
        for suffix in ["", "-wal", "-shm"] {
            let mut p = path.clone().into_os_string();
            p.push(suffix);
            let _ = std::fs::remove_file(p);
        }
        let db = Db::open(path.to_str().unwrap()).await.unwrap();
        (db, path)
    }

    fn cleanup(path: &std::path::Path) {
        for suffix in ["", "-wal", "-shm"] {
            let mut p = path.to_path_buf().into_os_string();
            p.push(suffix);
            let _ = std::fs::remove_file(p);
        }
    }

    async fn count(db: &Db) -> i64 {
        sqlx::query_as::<_, (i64,)>("SELECT COUNT(*) FROM readings")
            .fetch_one(&db.0)
            .await
            .unwrap()
            .0
    }

    #[tokio::test]
    async fn compaction_coarsens_old_rows_and_leaves_recent_ones_alone() {
        let (db, path) = temp_db("ladder").await;
        let now = DateTime::from_timestamp(1_700_000_000, 0).unwrap();

        // 120 raw readings spread over two hours, ending 3h ago: all past the 1h rung.
        for i in 0..120 {
            db.insert_reading(&Reading {
                sensor_id: 1,
                kind: ReadingKind::Temperature,
                value: 20.0,
                at: now - chrono::Duration::minutes(180 - i),
            })
            .await
            .unwrap();
        }
        // 10 raw readings inside the last 10 minutes: must survive untouched.
        for i in 0..10 {
            db.insert_reading(&Reading {
                sensor_id: 1,
                kind: ReadingKind::Temperature,
                value: 30.0,
                at: now - chrono::Duration::minutes(i),
            })
            .await
            .unwrap();
        }
        assert_eq!(count(&db).await, 130);

        let removed = db.compact(now, DEFAULT_TIERS).await.unwrap();
        assert!(removed > 0, "expected the old rows to be coarsened");

        let recent = sqlx::query_as::<_, (i64,)>(
            "SELECT COUNT(*) FROM readings WHERE bucket_secs = 0",
        )
        .fetch_one(&db.0)
        .await
        .unwrap()
        .0;
        assert_eq!(recent, 10, "rows inside the finest rung stay at full resolution");

        // The two hours of 1/minute readings collapse to one row per minute.
        let coarsened = sqlx::query_as::<_, (i64,)>(
            "SELECT COUNT(*) FROM readings WHERE bucket_secs = 60",
        )
        .fetch_one(&db.0)
        .await
        .unwrap()
        .0;
        assert_eq!(coarsened, 120, "one row per minute bucket");

        cleanup(&path);
    }

    #[tokio::test]
    async fn compaction_is_idempotent_and_preserves_the_mean() {
        let (db, path) = temp_db("idempotent").await;
        let now = DateTime::from_timestamp(1_700_000_000, 0).unwrap();
        // Align to a minute boundary so all four readings land in one bucket.
        let old = DateTime::from_timestamp(
            (now - chrono::Duration::hours(3)).timestamp() / 60 * 60,
            0,
        )
        .unwrap();

        // Four readings inside a single minute, averaging 25.
        for (offset, value) in [(0, 10.0), (15, 20.0), (30, 30.0), (45, 40.0)] {
            db.insert_reading(&Reading {
                sensor_id: 1,
                kind: ReadingKind::Temperature,
                value,
                at: old + chrono::Duration::seconds(offset),
            })
            .await
            .unwrap();
        }

        assert!(db.compact(now, DEFAULT_TIERS).await.unwrap() > 0);
        assert_eq!(count(&db).await, 1, "one minute bucket remains");

        let (value,) = sqlx::query_as::<_, (f64,)>("SELECT value FROM readings")
            .fetch_one(&db.0)
            .await
            .unwrap();
        assert!((value - 25.0).abs() < 0.001, "got {value}");

        // Running again must be a no-op rather than re-averaging or deleting.
        assert_eq!(db.compact(now, DEFAULT_TIERS).await.unwrap(), 0);
        assert_eq!(count(&db).await, 1);

        cleanup(&path);
    }

    #[tokio::test]
    async fn year_old_data_collapses_to_daily_points() {
        let (db, path) = temp_db("yearly").await;
        let now = DateTime::from_timestamp(1_700_000_000, 0).unwrap();
        // Align to a day boundary so the readings cover exactly two daily buckets.
        let ancient = DateTime::from_timestamp(
            (now - chrono::Duration::days(400)).timestamp() / 86_400 * 86_400,
            0,
        )
        .unwrap();

        // 48 readings across two days, over a year old.
        for i in 0..48 {
            db.insert_reading(&Reading {
                sensor_id: 1,
                kind: ReadingKind::Temperature,
                value: 20.0,
                at: ancient + chrono::Duration::hours(i),
            })
            .await
            .unwrap();
        }

        db.compact(now, DEFAULT_TIERS).await.unwrap();

        let rows = sqlx::query_as::<_, (i64, i64)>(
            "SELECT bucket_secs, COUNT(*) FROM readings GROUP BY bucket_secs",
        )
        .fetch_all(&db.0)
        .await
        .unwrap();
        assert_eq!(rows, vec![(86_400, 2)], "two days -> two daily averages");

        cleanup(&path);
    }
}
