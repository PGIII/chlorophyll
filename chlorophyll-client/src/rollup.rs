//! Averaging of readings into fixed-width time buckets before they reach storage.
//!
//! The sensors emit roughly five readings per second per metric. Persisting every one of
//! them costs ~1.3M rows per sensor per day and buys nothing: the dashboard's live values
//! come from the in-memory registry, and the charts only ever read averaged history.

use std::collections::HashMap;

use chrono::{DateTime, Utc};

use crate::reading::{Reading, ReadingKind};

/// Default width of an ingest bucket. Matches the finest chart resolution, so averaging
/// here costs no visible detail.
pub const INGEST_BUCKET_SECS: i64 = 60;

struct OpenBucket {
    index: i64,
    sum: f64,
    count: u32,
}

impl OpenBucket {
    fn average(&self, bucket_secs: i64) -> Option<(DateTime<Utc>, f32)> {
        if self.count == 0 {
            return None;
        }
        let at = DateTime::from_timestamp(self.index * bucket_secs, 0)?;
        #[allow(clippy::cast_possible_truncation)]
        let mean = (self.sum / f64::from(self.count)) as f32;
        Some((at, mean))
    }
}

/// Averages readings per `(sensor, metric)` into `bucket_secs`-wide windows.
///
/// A bucket is emitted once a reading for a *later* bucket arrives, so output lags real
/// time by up to one bucket. [`Self::drain_before`] closes buckets for sensors that have
/// gone quiet and would otherwise never see a successor.
pub struct ReadingAggregator {
    bucket_secs: i64,
    open: HashMap<(u128, ReadingKind), OpenBucket>,
}

impl ReadingAggregator {
    #[must_use]
    pub fn new(bucket_secs: i64) -> Self {
        Self {
            bucket_secs: bucket_secs.max(1),
            open: HashMap::new(),
        }
    }

    fn bucket_index(&self, at: DateTime<Utc>) -> i64 {
        at.timestamp().div_euclid(self.bucket_secs)
    }

    /// Feed one reading. Returns the completed average for the previous bucket, if this
    /// reading closed one.
    pub fn push(&mut self, reading: &Reading) -> Option<Reading> {
        let index = self.bucket_index(reading.at);
        let key = (reading.sensor_id, reading.kind);
        let bucket_secs = self.bucket_secs;

        match self.open.get_mut(&key) {
            Some(open) if open.index == index => {
                open.sum += f64::from(reading.value);
                open.count += 1;
                None
            }
            Some(open) => {
                let finished = open.average(bucket_secs);
                open.index = index;
                open.sum = f64::from(reading.value);
                open.count = 1;
                finished.map(|(at, value)| Reading {
                    sensor_id: reading.sensor_id,
                    kind: reading.kind,
                    value,
                    at,
                })
            }
            None => {
                self.open.insert(
                    key,
                    OpenBucket {
                        index,
                        sum: f64::from(reading.value),
                        count: 1,
                    },
                );
                None
            }
        }
    }

    /// Close and return every bucket strictly older than the bucket containing `now`.
    ///
    /// Without this, the last bucket of a sensor that stops transmitting would sit
    /// unwritten indefinitely.
    pub fn drain_before(&mut self, now: DateTime<Utc>) -> Vec<Reading> {
        let current = self.bucket_index(now);
        let bucket_secs = self.bucket_secs;
        let mut done = Vec::new();

        self.open.retain(|(sensor_id, kind), open| {
            if open.index >= current {
                return true;
            }
            if let Some((at, value)) = open.average(bucket_secs) {
                done.push(Reading {
                    sensor_id: *sensor_id,
                    kind: *kind,
                    value,
                    at,
                });
            }
            false
        });

        done
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reading(value: f32, secs: i64) -> Reading {
        Reading {
            sensor_id: 1,
            kind: ReadingKind::Temperature,
            value,
            at: DateTime::from_timestamp(secs, 0).unwrap(),
        }
    }

    #[test]
    fn averages_a_bucket_and_emits_it_when_the_next_one_opens() {
        let mut agg = ReadingAggregator::new(60);

        // Three readings inside the first minute: nothing emitted yet.
        assert!(agg.push(&reading(10.0, 0)).is_none());
        assert!(agg.push(&reading(20.0, 20)).is_none());
        assert!(agg.push(&reading(30.0, 59)).is_none());

        // A reading in the next minute closes the first.
        let emitted = agg.push(&reading(99.0, 60)).expect("bucket should close");
        assert!((emitted.value - 20.0).abs() < 0.001, "got {}", emitted.value);
        assert_eq!(emitted.at.timestamp(), 0, "timestamp is the bucket start");
    }

    #[test]
    fn drain_before_closes_buckets_from_sensors_that_went_quiet() {
        let mut agg = ReadingAggregator::new(60);
        agg.push(&reading(5.0, 0));
        agg.push(&reading(15.0, 30));

        // Nothing is due while we are still inside the same bucket.
        assert!(agg.drain_before(DateTime::from_timestamp(45, 0).unwrap()).is_empty());

        let drained = agg.drain_before(DateTime::from_timestamp(120, 0).unwrap());
        assert_eq!(drained.len(), 1);
        assert!((drained[0].value - 10.0).abs() < 0.001);

        // Draining twice must not duplicate the row.
        assert!(agg.drain_before(DateTime::from_timestamp(180, 0).unwrap()).is_empty());
    }

    #[test]
    fn separate_sensors_and_metrics_do_not_mix() {
        let mut agg = ReadingAggregator::new(60);
        agg.push(&Reading { sensor_id: 1, kind: ReadingKind::Temperature, value: 10.0, at: DateTime::from_timestamp(0, 0).unwrap() });
        agg.push(&Reading { sensor_id: 2, kind: ReadingKind::Temperature, value: 30.0, at: DateTime::from_timestamp(0, 0).unwrap() });
        agg.push(&Reading { sensor_id: 1, kind: ReadingKind::Humidity, value: 50.0, at: DateTime::from_timestamp(0, 0).unwrap() });

        let mut drained = agg.drain_before(DateTime::from_timestamp(120, 0).unwrap());
        drained.sort_by_key(|r| (r.sensor_id, r.kind.as_str()));
        assert_eq!(drained.len(), 3);
        assert!((drained[0].value - 50.0).abs() < 0.001, "sensor 1 humidity");
        assert!((drained[1].value - 10.0).abs() < 0.001, "sensor 1 temperature");
        assert!((drained[2].value - 30.0).abs() < 0.001, "sensor 2 temperature");
    }
}
