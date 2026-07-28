#![warn(clippy::pedantic)]

use std::sync::Arc;

use chlorophyll_client::db::Db;
use chlorophyll_client::rollup::{INGEST_BUCKET_SECS, ReadingAggregator};
use chlorophyll_client::{ClientConfig, SensorClient};
use chrono::Utc;
use sensor_server::AppState;
use tracing::*;
use tracing_subscriber::EnvFilter;

const DEFAULT_HTTP_PORT: u16 = 5001;

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

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
