use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::Instant;

/// Adaptive rate limiter that adjusts based on API response headers and errors.
pub struct RateLimiter {
    state: Arc<Mutex<RateLimitState>>,
}

struct RateLimitState {
    /// Minimum interval between requests.
    min_interval: Duration,
    /// Last request timestamp.
    last_request: Option<Instant>,
    /// Current backoff multiplier (increases on rate limit errors).
    backoff_multiplier: f64,
    /// Retry-after deadline (from 429 response header).
    retry_after_deadline: Option<Instant>,
}

impl RateLimiter {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(RateLimitState {
                min_interval: Duration::from_millis(100),
                last_request: None,
                backoff_multiplier: 1.0,
                retry_after_deadline: None,
            })),
        }
    }

    /// Wait until it's safe to make a request.
    pub async fn acquire(&self) {
        let wait_duration = {
            let state = self.state.lock().await;
            let now = Instant::now();

            // Check retry-after deadline
            if let Some(deadline) = state.retry_after_deadline {
                if now < deadline {
                    Some(deadline - now)
                } else {
                    state.last_request.and_then(|last| {
                        let interval = state.min_interval.mul_f64(state.backoff_multiplier);
                        let next = last + interval;
                        (next > now).then(|| next - now)
                    })
                }
            } else {
                state.last_request.and_then(|last| {
                    let interval = state.min_interval.mul_f64(state.backoff_multiplier);
                    let next = last + interval;
                    (next > now).then(|| next - now)
                })
            }
        };

        if let Some(duration) = wait_duration {
            tokio::time::sleep(duration).await;
        }

        let mut state = self.state.lock().await;
        state.last_request = Some(Instant::now());
    }

    /// Report a successful request. Gradually reduce backoff.
    pub async fn report_success(&self) {
        let mut state = self.state.lock().await;
        state.backoff_multiplier = (state.backoff_multiplier * 0.9).max(1.0);
        state.retry_after_deadline = None;
    }

    /// Report a rate limit (429) error.
    pub async fn report_rate_limited(&self, retry_after_secs: Option<u64>) {
        let mut state = self.state.lock().await;
        state.backoff_multiplier = (state.backoff_multiplier * 2.0).min(64.0);
        if let Some(secs) = retry_after_secs {
            state.retry_after_deadline = Some(Instant::now() + Duration::from_secs(secs));
        }
    }

    /// Update rate based on remaining quota from response headers.
    pub async fn update_from_remaining(&self, remaining: u64) {
        let mut state = self.state.lock().await;
        if remaining == 0 {
            state.backoff_multiplier = (state.backoff_multiplier * 2.0).min(64.0);
        } else if remaining > 100 {
            state.backoff_multiplier = (state.backoff_multiplier * 0.8).max(1.0);
        }
    }
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn acquire_does_not_block_on_first_call() {
        let limiter = RateLimiter::new();
        let start = Instant::now();
        limiter.acquire().await;
        assert!(start.elapsed() < Duration::from_millis(50));
    }

    #[tokio::test]
    async fn backoff_increases_on_rate_limit() {
        let limiter = RateLimiter::new();
        limiter.report_rate_limited(None).await;
        let state = limiter.state.lock().await;
        assert!(state.backoff_multiplier > 1.0);
    }

    #[tokio::test]
    async fn backoff_decreases_on_success() {
        let limiter = RateLimiter::new();
        limiter.report_rate_limited(None).await;
        limiter.report_success().await;
        let state = limiter.state.lock().await;
        assert!(state.backoff_multiplier < 2.0);
    }

    #[tokio::test]
    async fn backoff_capped_at_max() {
        let limiter = RateLimiter::new();
        for _ in 0..20 {
            limiter.report_rate_limited(None).await;
        }
        let state = limiter.state.lock().await;
        assert!(state.backoff_multiplier <= 64.0);
    }
}
