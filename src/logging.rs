use anyhow::{Context, Result};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};

static LAST_MAINTENANCE_DAY: OnceLock<Mutex<BTreeMap<PathBuf, i64>>> = OnceLock::new();

pub fn append_jsonl(
    log_dir: &Path,
    retention_days: u64,
    category: &str,
    value: &impl Serialize,
) -> Result<PathBuf> {
    let now = now_unix_ts();
    maybe_maintain(log_dir, retention_days, now)?;

    let mut path = archive_root(log_dir)
        .join(day_string(now))
        .join(Path::new(category));
    path.set_extension("jsonl");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    let raw = serde_json::to_string(value).context("failed to encode log entry")?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("failed to open {}", path.display()))?;
    writeln!(file, "{raw}").with_context(|| format!("failed to append {}", path.display()))?;
    Ok(path)
}

pub fn day_string(ts: u64) -> String {
    let days = unix_day(ts);
    let (year, month, day) = civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02}")
}

pub fn now_unix_ts() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn maybe_maintain(log_dir: &Path, retention_days: u64, now: u64) -> Result<()> {
    let current_day = unix_day(now);
    let registry = LAST_MAINTENANCE_DAY.get_or_init(|| Mutex::new(BTreeMap::new()));
    {
        let seen = registry.lock().expect("log maintenance lock poisoned");
        if seen.get(log_dir).copied() == Some(current_day) {
            return Ok(());
        }
    }

    prune_old_archives(log_dir, retention_days, current_day)?;
    let mut seen = registry.lock().expect("log maintenance lock poisoned");
    seen.insert(log_dir.to_path_buf(), current_day);
    Ok(())
}

fn prune_old_archives(log_dir: &Path, retention_days: u64, current_day: i64) -> Result<()> {
    if retention_days == 0 {
        return Ok(());
    }

    let archive = archive_root(log_dir);
    if !archive.exists() {
        return Ok(());
    }

    for entry in
        fs::read_dir(&archive).with_context(|| format!("failed to read {}", archive.display()))?
    {
        let entry = entry.with_context(|| format!("failed to inspect {}", archive.display()))?;
        if !entry
            .file_type()
            .with_context(|| format!("failed to inspect {}", entry.path().display()))?
            .is_dir()
        {
            continue;
        }

        let file_name = entry.file_name();
        let Some(day_name) = file_name.to_str() else {
            continue;
        };
        let Some(day) = parse_day_string(day_name) else {
            continue;
        };
        if current_day.saturating_sub(day) >= retention_days as i64 {
            fs::remove_dir_all(entry.path())
                .with_context(|| format!("failed to remove {}", entry.path().display()))?;
        }
    }

    Ok(())
}

fn archive_root(log_dir: &Path) -> PathBuf {
    log_dir.join("archive")
}

fn unix_day(ts: u64) -> i64 {
    (ts / 86_400) as i64
}

fn parse_day_string(value: &str) -> Option<i64> {
    let mut parts = value.split('-');
    let year = parts.next()?.parse::<i32>().ok()?;
    let month = parts.next()?.parse::<u32>().ok()?;
    let day = parts.next()?.parse::<u32>().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some(days_from_civil(year, month, day))
}

fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    let year = y + if m <= 2 { 1 } else { 0 };
    (year as i32, m as u32, d as u32)
}

fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let year = year as i64 - if month <= 2 { 1 } else { 0 };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let yoe = year - era * 400;
    let month = month as i64;
    let day = day as i64;
    let doy = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::{day_string, parse_day_string};

    #[test]
    fn day_string_round_trips_unix_days() {
        let day = day_string(1_773_360_000);
        assert_eq!(day, "2026-03-13");
        assert_eq!(
            parse_day_string(&day),
            Some((1_773_360_000 / 86_400) as i64)
        );
    }
}
