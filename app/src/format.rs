//! Numbers, money, times and dates, written the way the window shows them.

use chrono::{DateTime, Datelike, Local, TimeZone, Utc};

/// 12345 as "12,345"; a dash for nothing.
pub fn num(n: Option<u64>) -> String {
    let Some(n) = n else { return "-".into() };
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

pub fn numf(n: f64) -> String {
    if n.fract() == 0.0 && n >= 0.0 {
        num(Some(n as u64))
    } else {
        format!("{n}")
    }
}

pub fn money(amount: f64, currency: Option<&str>) -> String {
    let cur = currency.unwrap_or("USD").to_uppercase();
    let sym = match cur.as_str() {
        "USD" => "$",
        "EUR" => "€",
        "GBP" => "£",
        "JPY" => "¥",
        "CAD" => "CA$",
        "AUD" => "A$",
        _ => "",
    };
    let whole = amount.trunc() as u64;
    let cents = ((amount.fract().abs() * 100.0).round() as u64).min(99);
    let body = if cur == "JPY" { num(Some(amount.round() as u64)) } else { format!("{}.{cents:02}", num(Some(whole))) };
    if sym.is_empty() {
        format!("{body} {cur}")
    } else {
        format!("{sym}{body}")
    }
}

pub fn parse(iso: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(iso).ok().map(|d| d.with_timezone(&Utc))
}

/// "now", "5m ago", "3h ago", "2d ago".
pub fn ago(iso: Option<&str>) -> String {
    let Some(t) = iso.and_then(parse) else { return String::new() };
    let s = (Utc::now() - t).num_seconds().max(0);
    match s {
        0..=59 => "now".into(),
        60..=3599 => format!("{}m ago", s / 60),
        3600..=86399 => format!("{}h ago", s / 3600),
        _ => format!("{}d ago", s / 86400),
    }
}

/// The local day an event happened, as a key.
pub fn day_key(iso: Option<&str>) -> String {
    match iso.and_then(parse) {
        Some(t) => t.with_timezone(&Local).format("%Y-%m-%d").to_string(),
        None => "unknown".into(),
    }
}

/// "Today", "Yesterday", or "Monday, Nov 24" (with the year when it is not this year).
pub fn day_name(iso: Option<&str>) -> String {
    let Some(t) = iso.and_then(parse) else { return "Earlier".into() };
    let d = t.with_timezone(&Local).date_naive();
    let today = Local::now().date_naive();
    match (today - d).num_days() {
        0 => "Today".into(),
        1 => "Yesterday".into(),
        _ if d.year() != today.year() => d.format("%A, %b %-d, %Y").to_string(),
        _ => d.format("%A, %b %-d").to_string(),
    }
}

/// A Unix time as "3:05 PM".
pub fn time_at(unix: i64) -> String {
    Local.timestamp_opt(unix, 0).single().map(|t| t.format("%-I:%M %p").to_string()).unwrap_or_default()
}

/// Seconds as "4:05".
pub fn clock(s: f64) -> String {
    let s = s.max(0.0).ceil() as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

/// Seconds as "25 min" or "1 h 5 min".
pub fn minutes(s: f64) -> String {
    let m = (s / 60.0).round() as u64;
    if m < 60 {
        return format!("{m} min");
    }
    let (h, r) = (m / 60, m % 60);
    if r > 0 {
        format!("{h} h {r} min")
    } else {
        format!("{h} h")
    }
}

/// Milliseconds as "3:05" or "1:02:05".
pub fn song_clock(ms: u64) -> String {
    let s = ms / 1000;
    let (h, m, r) = (s / 3600, (s / 60) % 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{r:02}")
    } else {
        format!("{m}:{r:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_and_money_read_well() {
        assert_eq!(num(Some(1234567)), "1,234,567");
        assert_eq!(num(Some(999)), "999");
        assert_eq!(num(None), "-");
        assert_eq!(money(5.0, Some("usd")), "$5.00");
        assert_eq!(money(1234.5, Some("EUR")), "€1,234.50");
        assert_eq!(money(3.0, Some("SEK")), "3.00 SEK");
    }

    #[test]
    fn times_read_well() {
        assert_eq!(clock(65.2), "1:06");
        assert_eq!(minutes(3900.0), "1 h 5 min");
        assert_eq!(song_clock(3_725_000), "1:02:05");
    }
}
