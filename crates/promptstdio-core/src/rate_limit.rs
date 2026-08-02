//! 进程内滑动窗口限流（单实例）；多实例须配合 nginx `limit_req`（见 `infra/nginx/`）。

use std::collections::HashMap;
use std::time::{Duration, Instant};

use tokio::sync::Mutex;

/// 按 key（user id / client IP）限制单位时间内的请求次数。
#[derive(Clone)]
pub struct SlidingWindowRateLimiter {
    inner: std::sync::Arc<Mutex<HashMap<String, Vec<Instant>>>>,
    max_requests: usize,
    window: Duration,
}

impl SlidingWindowRateLimiter {
    pub fn per_minute(max_requests: usize) -> Self {
        Self::with_window(max_requests, Duration::from_secs(60))
    }

    fn with_window(max_requests: usize, window: Duration) -> Self {
        Self {
            inner: std::sync::Arc::new(Mutex::new(HashMap::new())),
            max_requests: max_requests.max(1),
            window,
        }
    }

    /// 返回 `true` 表示允许，`false` 表示已超限。
    pub async fn allow(&self, key: &str) -> bool {
        let now = Instant::now();
        let cutoff = now - self.window;
        let mut guard = self.inner.lock().await;
        let entries = guard.entry(key.to_string()).or_default();
        entries.retain(|t| *t > cutoff);
        if entries.len() >= self.max_requests {
            return false;
        }
        entries.push(now);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn blocks_after_max_requests() {
        let limiter = SlidingWindowRateLimiter::per_minute(2);
        assert!(limiter.allow("ip-1").await);
        assert!(limiter.allow("ip-1").await);
        assert!(!limiter.allow("ip-1").await);
        assert!(limiter.allow("ip-2").await);
    }

    #[tokio::test]
    async fn window_expiry_allows_new_requests() {
        let limiter = SlidingWindowRateLimiter::with_window(1, Duration::from_millis(50));
        assert!(limiter.allow("key").await);
        assert!(!limiter.allow("key").await);
        std::thread::sleep(Duration::from_millis(60));
        assert!(limiter.allow("key").await);
    }

    #[tokio::test]
    async fn off_by_one_at_exact_limit() {
        let limiter = SlidingWindowRateLimiter::with_window(3, Duration::from_secs(60));
        assert!(limiter.allow("u").await);
        assert!(limiter.allow("u").await);
        assert!(limiter.allow("u").await);
        assert!(!limiter.allow("u").await);
    }
}
