//! Operational metrics (T-97) as structured log events until a metrics backend exists: the
//! worker emits `metrics.*` snapshots every minute and `alert.*` events when a threshold is
//! crossed, so the log pipeline can chart p50/p95 and page on alerts.

/// Latency over a window: how many samples, and the 50th / 95th percentiles in seconds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LatencyStats {
    pub count: i64,
    pub p50_secs: f64,
    pub p95_secs: f64,
}

impl LatencyStats {
    /// From a `(count, p50, p95)` SQL row (`percentile_cont` is NULL without samples).
    pub fn from_row(row: (i64, Option<f64>, Option<f64>)) -> Self {
        let (count, p50, p95) = row;
        Self {
            count,
            p50_secs: p50.unwrap_or(0.0),
            p95_secs: p95.unwrap_or(0.0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_windows_report_zeroes() {
        assert_eq!(
            LatencyStats::from_row((0, None, None)),
            LatencyStats {
                count: 0,
                p50_secs: 0.0,
                p95_secs: 0.0
            }
        );
    }
}
