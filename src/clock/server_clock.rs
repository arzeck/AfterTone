//! Monotonic Server Clock Synchronization
//! High precision RTT drift correction and clock estimation.

use std::sync::OnceLock;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy)]
pub struct ClockSample {
    pub offset_ms: i64,
    pub round_trip_ms: i64,
    pub taken_at_ms: i64,
}

const WINDOW_SIZE: usize = 12;
const SAMPLE_TTL_MS: i64 = 120_000;

static BOOT_INSTANT: OnceLock<Instant> = OnceLock::new();
static BOOT_UNIX_MS: OnceLock<i64> = OnceLock::new();

fn init_boot() -> (Instant, i64) {
    let now_inst = *BOOT_INSTANT.get_or_init(Instant::now);
    let now_unix = *BOOT_UNIX_MS.get_or_init(|| {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_millis() as i64
    });
    (now_inst, now_unix)
}

#[derive(Debug, Clone, Default)]
pub struct ServerClock {
    samples: Vec<ClockSample>,
    pub offset_ms: Option<i64>,
    pub round_trip_ms: i64,
}

impl ServerClock {
    pub fn new() -> Self {
        init_boot();
        Self {
            samples: Vec::with_capacity(WINDOW_SIZE),
            offset_ms: None,
            round_trip_ms: 0,
        }
    }

    /// Monotonic local timestamp in milliseconds.
    /// Immune to system clock steps, NTP adjustments, or DST shifts.
    pub fn local_now_ms() -> i64 {
        let (boot_inst, boot_unix) = init_boot();
        let elapsed = boot_inst.elapsed().as_millis() as i64;
        boot_unix + elapsed
    }

    /// Records a ping/pong exchange.
    pub fn record(&mut self, sent_at_local_ms: i64, server_ms: i64, received_at_local_ms: i64) {
        let round_trip = (received_at_local_ms - sent_at_local_ms).max(0);
        let midpoint = sent_at_local_ms + round_trip / 2;
        let sample_offset = server_ms - midpoint;

        self.samples.push(ClockSample {
            offset_ms: sample_offset,
            round_trip_ms: round_trip,
            taken_at_ms: received_at_local_ms,
        });

        while self.samples.len() > WINDOW_SIZE {
            self.samples.remove(0);
        }

        // Discard samples older than TTL
        let cutoff = received_at_local_ms - SAMPLE_TTL_MS;
        let usable: Vec<ClockSample> = self
            .samples
            .iter()
            .copied()
            .filter(|s| s.taken_at_ms >= cutoff)
            .collect();
        let pool = if !usable.is_empty() {
            usable
        } else {
            self.samples.clone()
        };

        // Sample with the smallest round-trip latency wins
        if let Some(best) = pool.iter().min_by_key(|s| s.round_trip_ms) {
            self.offset_ms = Some(best.offset_ms);
            self.round_trip_ms = best.round_trip_ms;
        }
    }

    /// Approximates the current server timestamp in milliseconds.
    /// Returns None if no sync sample has arrived yet.
    pub fn server_now_ms(&self) -> Option<i64> {
        self.offset_ms.map(|off| Self::local_now_ms() + off)
    }

    /// Computes the current playhead position in milliseconds.
    pub fn compute_playhead(
        &self,
        position_ms: i64,
        anchor_ms: i64,
        is_playing: bool,
        duration_ms: Option<i64>,
    ) -> i64 {
        if !is_playing {
            return position_ms.max(0);
        }

        let s_now = match self.server_now_ms() {
            Some(s) => s,
            None => {
                if anchor_ms > 1_000_000_000_000 {
                    let now_unix = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or(Duration::ZERO)
                        .as_millis() as i64;
                    let elapsed = (now_unix - anchor_ms).max(0);
                    let current = position_ms + elapsed;
                    if let Some(dur) = duration_ms {
                        if dur > 0 && current > dur {
                            return dur;
                        }
                    }
                    return current.max(0);
                }
                return position_ms.max(0);
            }
        };

        let elapsed = (s_now - anchor_ms).max(0);
        let current = position_ms + elapsed;

        if let Some(dur) = duration_ms {
            if dur > 0 && current > dur {
                return dur;
            }
        }
        current.max(0)
    }

    pub fn reset(&mut self) {
        self.samples.clear();
        self.offset_ms = None;
        self.round_trip_ms = 0;
    }
}
