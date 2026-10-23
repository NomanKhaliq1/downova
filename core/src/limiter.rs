use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Shared bytes-per-second limiter. Rate 0 means unlimited.
pub struct Limiter {
    rate: AtomicU64,
    next: Mutex<Instant>,
}

impl Limiter {
    pub fn new(rate: Option<u64>) -> Self {
        Limiter { rate: AtomicU64::new(rate.unwrap_or(0)), next: Mutex::new(Instant::now()) }
    }

    pub fn set_rate(&self, rate: Option<u64>) {
        self.rate.store(rate.unwrap_or(0), Ordering::Relaxed);
    }

    pub async fn take(&self, n: u64) {
        let rate = self.rate.load(Ordering::Relaxed);
        if rate == 0 {
            return;
        }
        let wait = {
            let mut next = self.next.lock().unwrap();
            let now = Instant::now();
            let start = (*next).max(now);
            *next = start + Duration::from_secs_f64(n as f64 / rate as f64);
            start - now
        };
        if !wait.is_zero() {
            tokio::time::sleep(wait).await;
        }
    }
}
