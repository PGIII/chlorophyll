#![warn(clippy::pedantic)]

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use chlorophyll_client::db::Db;
use chlorophyll_client::rollup::{INGEST_BUCKET_SECS, ReadingAggregator};
use chlorophyll_client::{ClientConfig, SensorClient};
use chrono::Utc;
use sensor_server::AppState;
use tracing::*;
use tracing_subscriber::EnvFilter;

const DEFAULT_HTTP_PORT: u16 = 5001;

/// Hard ceiling for the `CHLOROPHYLL_LOG` file. The server writes a few KB a day, so this
/// is a runaway guard rather than a rotation scheme: on overflow the file starts over.
const MAX_LOG_BYTES: u64 = 512 * 1024 * 1024;

struct CappedLog {
    path: PathBuf,
    file: File,
    written: u64,
    max: u64,
}

impl CappedLog {
    fn open(path: PathBuf, max: u64) -> std::io::Result<Self> {
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        let written = file.metadata()?.len();
        Ok(Self { path, file, written, max })
    }
}

impl Write for CappedLog {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if self.written + buf.len() as u64 > self.max {
            self.file = File::create(&self.path)?;
            let marker = format!("--- log restarted: exceeded {} bytes ---\n", self.max);
            self.file.write_all(marker.as_bytes())?;
            self.written = marker.len() as u64;
        }
        let n = self.file.write(buf)?;
        self.written += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }
}

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into());
    match std::env::var("CHLOROPHYLL_LOG") {
        Ok(path) => tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_ansi(false)
            .with_writer(Mutex::new(CappedLog::open(PathBuf::from(path), MAX_LOG_BYTES)?))
            .init(),
        Err(_) => tracing_subscriber::fmt().with_env_filter(filter).init(),
    }

    let args: Vec<String> = std::env::args().collect();

    // set-name <hex_id> <name> — broadcast SetName to the multicast group
    if args.get(1).map(String::as_str) == Some("set-name") {
        let id_hex = args
            .get(2)
            .expect("usage: sensor_server set-name <hex_id> <name>");
        let name = args
            .get(3)
            .expect("usage: sensor_server set-name <hex_id> <name>");
        let sensor_id = u128::from_str_radix(id_hex.trim_start_matches("0x"), 16)
            .expect("invalid sensor id hex");

        let client = SensorClient::start(ClientConfig::default())
            .map_err(|e| color_eyre::eyre::eyre!("{e}"))?;
        client
            .set_name(sensor_id, name)
            .map_err(|e| color_eyre::eyre::eyre!("{e}"))?;
        info!("Sent SetName(\"{name}\") for sensor {sensor_id:032x}");
        return Ok(());
    }

    // Normal server mode
    let db_path = std::env::var("CHLOROPHYLL_DB").unwrap_or_else(|_| "chlorophyll.db".to_string());
    let db = Db::open(&db_path)
        .await
        .map_err(|e| color_eyre::eyre::eyre!("{e}"))?;
    info!("Database opened at {db_path}");

    let client = Arc::new(
        SensorClient::start(ClientConfig::default()).map_err(|e| color_eyre::eyre::eyre!("{e}"))?,
    );
    info!("Listening for sensor readings");

    let port = std::env::var("CHLOROPHYLL_HTTP_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(DEFAULT_HTTP_PORT);

    let state = AppState {
        client: client.clone(),
        db: db.clone(),
    };
    let router = sensor_server::router().with_state(state);
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::UNSPECIFIED, port)).await?;
    info!("Listening on http://{}", listener.local_addr()?);

    // Compact once at startup so a restart also catches up any backlog, then hourly.
    {
        let db = db.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(std::time::Duration::from_secs(3600));
            loop {
                ticker.tick().await;
                match db.compact(Utc::now(), chlorophyll_client::db::DEFAULT_TIERS).await {
                    Ok(0) => {}
                    Ok(removed) => info!("compacted history, removed {removed} rows"),
                    Err(e) => error!("compaction error: {e}"),
                }
            }
        });
    }

    let mut readings = client.subscribe();
    tokio::spawn(async move {
        // Readings arrive at ~5 Hz per metric; average them into one row per minute
        // rather than persisting every sample. The dashboard's live values come from the
        // in-memory registry, so this costs no visible freshness.
        let mut aggregator = ReadingAggregator::new(INGEST_BUCKET_SECS);
        let mut flush = tokio::time::interval(std::time::Duration::from_secs(INGEST_BUCKET_SECS as u64));

        loop {
            tokio::select! {
                received = readings.recv() => match received {
                    Ok(reading) => {
                        if let Some(averaged) = aggregator.push(&reading) {
                            if let Err(e) = db.insert_reading_at(&averaged, INGEST_BUCKET_SECS).await {
                                error!("DB insert error: {e}");
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        warn!("readings channel lagged, dropped {n} messages");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                },
                // Closes buckets for sensors that stopped transmitting mid-window.
                _ = flush.tick() => {
                    for averaged in aggregator.drain_before(Utc::now()) {
                        if let Err(e) = db.insert_reading_at(&averaged, INGEST_BUCKET_SECS).await {
                            error!("DB insert error: {e}");
                        }
                    }
                }
            }
        }
    });

    axum::serve(listener, router)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
            info!("Shutting down");
        })
        .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restarts_instead_of_growing_past_the_cap() {
        let path = std::env::temp_dir().join("capped_log_test.log");
        let _ = std::fs::remove_file(&path);

        let mut log = CappedLog::open(path.clone(), 200).unwrap();
        for _ in 0..50 {
            log.write_all(b"0123456789012345678901234567890123456789\n").unwrap();
        }
        log.flush().unwrap();

        let len = std::fs::metadata(&path).unwrap().len();
        assert!(len <= 200, "log grew to {len} bytes, past the 200 byte cap");

        let contents = std::fs::read_to_string(&path).unwrap();
        assert!(contents.starts_with("--- log restarted: exceeded 200 bytes ---"));
        assert!(contents.ends_with("0123456789012345678901234567890123456789\n"));

        std::fs::remove_file(&path).unwrap();
    }
}
