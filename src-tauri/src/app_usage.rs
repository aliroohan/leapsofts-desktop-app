use crate::active_window::{current_platform, get_active_window, sanitized_from_snapshot};
use crate::api::Api;
use crate::input;
use crate::queue;
use crate::state::{monitoring_should_run, Hub};
use crate::types::{AppUsageSegment, TrackingSummary};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Manager};
use uuid::Uuid;

const POLL_MS: u64 = 5000;
const WINDOW_MS: i64 = 10 * 60 * 1000;
const MIN_SEGMENT_SEC: i64 = 2;
const BATCH_SIZE: usize = 200;

static RUNNING: AtomicBool = AtomicBool::new(false);
static LOOP_SPAWNED: AtomicBool = AtomicBool::new(false);

struct OpenSegment {
    app: String,
    exec_name: String,
    title: String,
    url: Option<String>,
    domain: Option<String>,
    started_at_ms: i64,
    active_origin: u64,
    active_sec: u64,
}

static OPEN: Mutex<Option<OpenSegment>> = Mutex::new(None);

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn iso(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms)
        .unwrap_or_else(chrono::Utc::now)
        .to_rfc3339()
}

fn measure_active(seg: &OpenSegment) -> u64 {
    let from_cumul = input::get_cumulative_active_sec().saturating_sub(seg.active_origin);
    let mut active = from_cumul.max(seg.active_sec);
    if input::has_pending_input_this_second() {
        active += 1;
    }
    active
}

fn close_open(app: &AppHandle, end_ms: i64) {
    let Some(api) = app.try_state::<Api>() else {
        return;
    };
    let mut guard = OPEN.lock().unwrap();
    let Some(segment) = guard.take() else {
        return;
    };
    drop(guard);
    let elapsed_ms = end_ms - segment.started_at_ms;
    if elapsed_ms < MIN_SEGMENT_SEC * 1000 {
        return;
    }
    let duration_sec = (elapsed_ms / 1000).max(MIN_SEGMENT_SEC) as u64;
    let active_sec = measure_active(&segment).min(duration_sec);
    let entry = AppUsageSegment {
        client_id: Uuid::new_v4().to_string(),
        app: segment.app,
        exec_name: segment.exec_name,
        title: segment.title,
        url: segment.url,
        domain: segment.domain,
        started_at: iso(segment.started_at_ms),
        ended_at: iso(end_ms),
        duration_sec,
        active_sec,
        platform: current_platform().to_string(),
    };
    let _ = api.db.lock().map(|db| queue::enqueue_app_usage(&db, &entry));
}

pub fn close_open_usage_segment(app: &AppHandle) {
    close_open(app, now_ms());
    publish_summary(app);
}

fn publish_summary(app: &AppHandle) {
    let pending = app
        .try_state::<Api>()
        .and_then(|api| api.db.try_lock().ok().map(|db| queue::pending_app_usage_count(&db)))
        .unwrap_or(0);
    let Ok(open) = OPEN.try_lock() else {
        return;
    };
    let summary = TrackingSummary {
        app: open.as_ref().map(|o| o.app.clone()),
        domain: open.as_ref().and_then(|o| o.domain.clone()),
        pending_segments: pending,
    };
    drop(open);
    if let Some(hub) = app.try_state::<Hub>() {
        hub.patch(app, |s| s.tracking_summary = Some(summary));
    }
}

fn poll_once(app: &AppHandle) {
    let should = app
        .try_state::<Hub>()
        .map(|h| {
            let st = h.get(app);
            monitoring_should_run(&st) && st.user.as_ref().map(|u| u.wants_app_usage()).unwrap_or(true)
        })
        .unwrap_or(false);
    if !should {
        close_open(app, now_ms());
        publish_summary(app);
        return;
    }

    let snapshot = match get_active_window() {
        Ok(s) => s,
        Err(_) => {
            if let Some(hub) = app.try_state::<Hub>() {
                hub.patch(app, |s| {
                    s.monitoring_error = Some(
                        "Window tracking unavailable — grant Screen Recording and Automation permission in System Settings and restart the app."
                            .into(),
                    );
                });
            }
            close_open(app, now_ms());
            publish_summary(app);
            return;
        }
    };

    let Some(snap) = snapshot else {
        close_open(app, now_ms());
        publish_summary(app);
        return;
    };

    let (url, domain) = sanitized_from_snapshot(&snap);
    let now = now_ms();
    let (k, m) = input::last_tick();

    let mut open = OPEN.lock().unwrap();
    let crosses = open
        .as_ref()
        .map(|o| o.started_at_ms / WINDOW_MS != now / WINDOW_MS)
        .unwrap_or(false);
    let changed = match open.as_ref() {
        None => true,
        Some(o) => o.app != snap.app || o.title != snap.title || o.url != url,
    };
    if let Some(o) = open.as_mut() {
        if k || m {
            o.active_sec += 5; // poll interval seconds of possible activity; refined by cumulative on close
        }
    }
    if changed || crosses {
        drop(open);
        close_open(app, now);
        *OPEN.lock().unwrap() = Some(OpenSegment {
            app: snap.app,
            exec_name: snap.exec_name,
            title: snap.title,
            url,
            domain,
            started_at_ms: now,
            active_origin: input::get_cumulative_active_sec(),
            active_sec: 0,
        });
    }
    publish_summary(app);
}

pub fn start(app: AppHandle) {
    input::ensure_started();
    RUNNING.store(true, Ordering::SeqCst);
    if LOOP_SPAWNED.swap(true, Ordering::SeqCst) {
        return;
    }
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(POLL_MS));
        loop {
            interval.tick().await;
            if !RUNNING.load(Ordering::SeqCst) {
                continue;
            }
            poll_once(&app);
        }
    });
}

pub fn stop(app: &AppHandle) {
    RUNNING.store(false, Ordering::SeqCst);
    close_open(app, now_ms());
    publish_summary(app);
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let _ = flush_pending(&handle).await;
    });
}

pub async fn flush_pending(app: &AppHandle) -> Result<(), String> {
    let Some(api) = app.try_state::<Api>() else {
        return Ok(());
    };
    let pending = {
        let db = api.db.lock().map_err(|e| e.to_string())?;
        queue::list_app_usage(&db)
    };
    for chunk in pending.chunks(BATCH_SIZE) {
        let ids: Vec<String> = chunk.iter().map(|s| s.client_id.clone()).collect();
        match api.upload_app_usage(chunk).await {
            Ok(()) => {
                if let Ok(db) = api.db.lock() {
                    queue::remove_app_usage(&db, &ids);
                }
                if let Some(hub) = app.try_state::<Hub>() {
                    hub.patch(app, |s| s.last_error = None);
                }
            }
            Err(e) if e.is_network() => {
                if let Some(hub) = app.try_state::<Hub>() {
                    hub.patch(app, |s| s.is_online = false);
                }
                return Ok(());
            }
            Err(_) => {
                if let Ok(db) = api.db.lock() {
                    queue::remove_app_usage(&db, &ids);
                }
            }
        }
    }
    publish_summary(app);
    Ok(())
}
