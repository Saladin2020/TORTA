use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Simple in-memory login rate limiter (per username + IP key).
pub struct LoginLimiter {
    inner: Mutex<HashMap<String, Window>>,
    max_attempts: u32,
    window: Duration,
}

struct Window {
    count: u32,
    reset_at: Instant,
}

impl LoginLimiter {
    pub fn new(max_attempts: u32, window_secs: u64) -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
            max_attempts,
            window: Duration::from_secs(window_secs),
        }
    }

    pub fn check_and_hit(&self, key: &str) -> Result<(), String> {
        let mut map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        let entry = map.entry(key.to_string()).or_insert(Window {
            count: 0,
            reset_at: now + self.window,
        });
        if now >= entry.reset_at {
            entry.count = 0;
            entry.reset_at = now + self.window;
        }
        if entry.count >= self.max_attempts {
            let secs = entry
                .reset_at
                .saturating_duration_since(now)
                .as_secs()
                .max(1);
            return Err(format!("too many login attempts — retry in {secs}s"));
        }
        entry.count += 1;
        Ok(())
    }

    pub fn clear(&self, key: &str) {
        let mut map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        map.remove(key);
    }
}
