use chrono::{Local, Timelike};

/// Minimal cron subset: `M H * * *` (minute hour every day).
/// Returns true if we should run in this wall-clock minute and haven't marked it yet.
pub fn should_run_now(cron: &str, last_run_minute_key: &str) -> (bool, String) {
    let parts: Vec<&str> = cron.split_whitespace().collect();
    let (minute, hour) = if parts.len() >= 2 {
        (
            parts[0].parse::<u32>().unwrap_or(0),
            parts[1].parse::<u32>().unwrap_or(2),
        )
    } else {
        (0, 2)
    };

    let now = Local::now();
    let key = format!("{}-{:02}-{:02}", now.date_naive(), now.hour(), now.minute());
    let due = now.hour() == hour && now.minute() == minute;
    let should = due && key != last_run_minute_key;
    (should, key)
}
