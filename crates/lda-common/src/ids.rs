use std::time::{SystemTime, UNIX_EPOCH};

use uuid::Uuid;

pub fn new_id() -> String {
    Uuid::new_v4().simple().to_string()
}

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

pub fn title_from(text: &str) -> String {
    let one_line = text.lines().next().unwrap_or("").trim();
    if one_line.is_empty() {
        return "New conversation".to_string();
    }
    let mut out = String::new();
    for (i, ch) in one_line.chars().enumerate() {
        if i == 48 {
            out.push('…');
            break;
        }
        out.push(ch);
    }
    out
}
