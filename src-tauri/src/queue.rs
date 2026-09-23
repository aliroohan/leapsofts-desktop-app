use crate::types::{AppUsageSegment, PendingActivitySample, PendingBreak};
use rusqlite::{params, Connection};
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

const MAX_APP_USAGE: usize = 20_000;

pub fn open_db(data_dir: &Path) -> rusqlite::Result<Connection> {
    fs::create_dir_all(data_dir).ok();
    let conn = Connection::open(data_dir.join("tracker.sqlite"))?;
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS kv (
          key TEXT PRIMARY KEY,
          value TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS pending_breaks (
          id TEXT PRIMARY KEY,
          start_time TEXT NOT NULL,
          end_time TEXT,
          source TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS pending_samples (
          id TEXT PRIMARY KEY,
          window_start TEXT NOT NULL,
          captured_at TEXT NOT NULL,
          keyboard_pct INTEGER NOT NULL,
          mouse_pct INTEGER NOT NULL,
          combined_pct INTEGER NOT NULL,
          image_path TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS pending_app_usage (
          client_id TEXT PRIMARY KEY,
          payload TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS pending_heartbeats (
          beat_time TEXT PRIMARY KEY
        );
        "#,
    )?;
    Ok(conn)
}

pub fn image_dir(data_dir: &Path) -> PathBuf {
    let dir = data_dir.join("activity-screenshots");
    let _ = fs::create_dir_all(&dir);
    dir
}

pub fn kv_get(conn: &Connection, key: &str) -> Option<String> {
    conn.query_row("SELECT value FROM kv WHERE key = ?1", params![key], |r| r.get(0))
        .ok()
}

pub fn kv_set(conn: &Connection, key: &str, value: &str) {
    let _ = conn.execute(
        "INSERT INTO kv(key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    );
}

pub fn kv_del(conn: &Connection, key: &str) {
    let _ = conn.execute("DELETE FROM kv WHERE key = ?1", params![key]);
}

pub fn pending_count(conn: &Connection) -> u32 {
    let breaks: i64 = conn
        .query_row("SELECT COUNT(*) FROM pending_breaks", [], |r| r.get(0))
        .unwrap_or(0);
    breaks as u32
}

pub fn has_open_pending(conn: &Connection) -> bool {
    conn.query_row(
        "SELECT COUNT(*) FROM pending_breaks WHERE end_time IS NULL",
        [],
        |r| r.get::<_, i64>(0),
    )
    .unwrap_or(0)
        > 0
}

pub fn append_open_interval(conn: &Connection, source: &str, start_time: &str) {
    if has_open_pending(conn) {
        return;
    }
    let id = Uuid::new_v4().to_string();
    let _ = conn.execute(
        "INSERT INTO pending_breaks(id, start_time, end_time, source) VALUES (?1, ?2, NULL, ?3)",
        params![id, start_time, source],
    );
}

pub fn close_open_intervals(conn: &Connection, end_time: &str) {
    let _ = conn.execute(
        "UPDATE pending_breaks SET end_time = ?1 WHERE end_time IS NULL",
        params![end_time],
    );
}

pub fn closed_pending_breaks(conn: &Connection) -> Vec<PendingBreak> {
    let mut stmt = match conn.prepare(
        "SELECT id, start_time, end_time, source FROM pending_breaks WHERE end_time IS NOT NULL",
    ) {
        Ok(s) => s,
        Err(_) => return vec![],
    };
    stmt.query_map([], |r| {
        Ok(PendingBreak {
            id: r.get(0)?,
            start_time: r.get(1)?,
            end_time: r.get(2)?,
            source: r.get(3)?,
        })
    })
    .map(|rows| rows.filter_map(|x| x.ok()).collect())
    .unwrap_or_default()
}

pub fn remove_pending_break(conn: &Connection, id: &str) {
    let _ = conn.execute("DELETE FROM pending_breaks WHERE id = ?1", params![id]);
}

pub fn enqueue_sample(conn: &Connection, sample: &PendingActivitySample) {
    let _ = conn.execute(
        "INSERT INTO pending_samples(id, window_start, captured_at, keyboard_pct, mouse_pct, combined_pct, image_path)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            sample.id,
            sample.window_start,
            sample.captured_at,
            sample.keyboard_pct,
            sample.mouse_pct,
            sample.combined_pct,
            sample.image_path
        ],
    );
}

pub fn list_samples(conn: &Connection) -> Vec<PendingActivitySample> {
    let mut stmt = match conn.prepare(
        "SELECT id, window_start, captured_at, keyboard_pct, mouse_pct, combined_pct, image_path FROM pending_samples",
    ) {
        Ok(s) => s,
        Err(_) => return vec![],
    };
    stmt.query_map([], |r| {
        Ok(PendingActivitySample {
            id: r.get(0)?,
            window_start: r.get(1)?,
            captured_at: r.get(2)?,
            keyboard_pct: r.get(3)?,
            mouse_pct: r.get(4)?,
            combined_pct: r.get(5)?,
            image_path: r.get(6)?,
        })
    })
    .map(|rows| rows.filter_map(|x| x.ok()).collect())
    .unwrap_or_default()
}

pub fn remove_sample(conn: &Connection, id: &str, image_path: &str) {
    let _ = fs::remove_file(image_path);
    let _ = conn.execute("DELETE FROM pending_samples WHERE id = ?1", params![id]);
}

pub fn enqueue_app_usage(conn: &Connection, segment: &AppUsageSegment) {
    let payload = serde_json::to_string(segment).unwrap_or_default();
    let _ = conn.execute(
        "INSERT OR REPLACE INTO pending_app_usage(client_id, payload) VALUES (?1, ?2)",
        params![segment.client_id, payload],
    );
    cap_app_usage(conn);
}

fn cap_app_usage(conn: &Connection) {
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM pending_app_usage", [], |r| r.get(0))
        .unwrap_or(0);
    if count as usize > MAX_APP_USAGE {
        let drop_n = count as usize - MAX_APP_USAGE;
        let _ = conn.execute(
            "DELETE FROM pending_app_usage WHERE client_id IN (
               SELECT client_id FROM pending_app_usage LIMIT ?1
             )",
            params![drop_n as i64],
        );
    }
}

pub fn list_app_usage(conn: &Connection) -> Vec<AppUsageSegment> {
    let mut stmt = match conn.prepare("SELECT payload FROM pending_app_usage") {
        Ok(s) => s,
        Err(_) => return vec![],
    };
    stmt.query_map([], |r| r.get::<_, String>(0))
        .map(|rows| {
            rows.filter_map(|x| x.ok())
                .filter_map(|s| serde_json::from_str(&s).ok())
                .collect()
        })
        .unwrap_or_default()
}

pub fn pending_app_usage_count(conn: &Connection) -> u32 {
    conn.query_row("SELECT COUNT(*) FROM pending_app_usage", [], |r| r.get::<_, i64>(0))
        .unwrap_or(0) as u32
}

pub fn remove_app_usage(conn: &Connection, ids: &[String]) {
    for id in ids {
        let _ = conn.execute("DELETE FROM pending_app_usage WHERE client_id = ?1", params![id]);
    }
}

pub fn append_heartbeat(conn: &Connection, beat_time: &str) {
    let _ = conn.execute(
        "INSERT OR IGNORE INTO pending_heartbeats(beat_time) VALUES (?1)",
        params![beat_time],
    );
}

pub fn list_heartbeats(conn: &Connection, limit: usize) -> Vec<String> {
    let mut stmt = match conn.prepare(
        "SELECT beat_time FROM pending_heartbeats ORDER BY beat_time ASC LIMIT ?1",
    ) {
        Ok(s) => s,
        Err(_) => return vec![],
    };
    stmt.query_map(params![limit as i64], |r| r.get(0))
        .map(|rows| rows.filter_map(|x| x.ok()).collect())
        .unwrap_or_default()
}

pub fn remove_heartbeats(conn: &Connection, beats: &[String]) {
    for beat in beats {
        let _ = conn.execute("DELETE FROM pending_heartbeats WHERE beat_time = ?1", params![beat]);
    }
}

pub fn clear_image_dir(data_dir: &Path) {
    let dir = image_dir(data_dir);
    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                let _ = fs::remove_file(path);
            }
        }
    }
}

/// Drop shift telemetry after checkout. Keep `kv` (session token) and any
/// closed break rows that still need to reach the server.
pub fn clear_shift_telemetry(conn: &Connection, data_dir: &Path) {
    for sample in list_samples(conn) {
        remove_sample(conn, &sample.id, &sample.image_path);
    }
    let _ = conn.execute("DELETE FROM pending_samples", []);
    let _ = conn.execute("DELETE FROM pending_app_usage", []);
    let _ = conn.execute("DELETE FROM pending_heartbeats", []);
    clear_image_dir(data_dir);
    let _ = conn.execute_batch("VACUUM");
}
