use crate::api::{is_online, Api, ApiError, PasswordLogin};
use crate::app_usage;
use crate::queue;
use crate::screenshots;
use crate::state::Hub;
use crate::types::{has_open_meeting, is_checked_in, open_break_source, Shift};
use tauri::{AppHandle, Manager};

pub fn sync_monitors(app: &AppHandle) {
    let Some(hub) = app.try_state::<Hub>() else {
        return;
    };
    let st = hub.get(app);
    let should = crate::state::monitoring_should_run(&st);
    let shots = st.user.as_ref().map(|u| u.wants_screenshots()).unwrap_or(true);
    let usage = st.user.as_ref().map(|u| u.wants_app_usage()).unwrap_or(true);
    if should && shots {
        screenshots::start(app.clone());
    } else {
        screenshots::stop();
        hub.patch(app, |s| s.monitoring_active = false);
    }
    if should && usage {
        app_usage::start(app.clone());
    } else {
        app_usage::stop(app);
    }
}

async fn apply_shift(app: &AppHandle, shift: Option<Shift>) {
    refresh_me_quietly(app).await;
    if let Some(hub) = app.try_state::<Hub>() {
        hub.patch(app, |s| s.shift = shift);
    }
    sync_monitors(app);
}

fn is_stale_shift_conflict(err: &ApiError) -> bool {
    if err.is_network() {
        return false;
    }
    let lower = err.to_string().to_lowercase();
    [
        "not currently on break",
        "already on break",
        "not currently checked in",
        "already checked in",
        "meeting is already in progress",
        "no meeting in progress",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
}

async fn reconcile_shift_from_server(app: &AppHandle) -> Result<(), String> {
    let shift = app
        .state::<Api>()
        .fetch_today_shift()
        .await
        .map_err(|e| e.to_string())?;
    apply_shift(app, shift).await;
    if let Some(hub) = app.try_state::<Hub>() {
        hub.patch(app, |s| s.last_error = None);
    }
    Ok(())
}

struct ShiftBusyGuard;

impl ShiftBusyGuard {
    fn enter() -> Self {
        SHIFT_BUSY.store(true, std::sync::atomic::Ordering::SeqCst);
        Self
    }
}

impl Drop for ShiftBusyGuard {
    fn drop(&mut self) {
        SHIFT_BUSY.store(false, std::sync::atomic::Ordering::SeqCst);
    }
}

/// When the server says local state is already behind, refetches today's shift
/// and returns `Ok(None)`. A failed refetch keeps the original conflict message.
async fn finish_shift_mutation(
    app: &AppHandle,
    result: Result<Shift, ApiError>,
) -> Result<Option<Shift>, String> {
    match result {
        Ok(shift) => Ok(Some(shift)),
        Err(e) if is_stale_shift_conflict(&e) => {
            let message = e.to_string();
            reconcile_shift_from_server(app)
                .await
                .map_err(|_| message)?;
            Ok(None)
        }
        Err(e) => Err(e.to_string()),
    }
}

async fn refresh_me_quietly(app: &AppHandle) {
    let Some(api) = app.try_state::<Api>() else {
        return;
    };
    if let Ok(user) = api.fetch_me().await {
        if let Some(hub) = app.try_state::<Hub>() {
            hub.set_user(app, Some(user));
        }
        sync_monitors(app);
    }
}

pub fn should_record_sleep(app: &AppHandle) -> bool {
    let Some(hub) = app.try_state::<Hub>() else {
        return false;
    };
    let st = hub.get(app);
    if !st.is_authenticated || !is_checked_in(&st.shift) {
        return false;
    }
    // Meeting mode: do not record sleep/offline intervals.
    if has_open_meeting(&st.shift) {
        return false;
    }
    !matches!(
        open_break_source(&st.shift).as_deref(),
        Some("manual") | Some("idle") | Some("sleep")
    )
}

fn should_log_heartbeat(app: &AppHandle) -> bool {
    let Some(hub) = app.try_state::<Hub>() else {
        return false;
    };
    let st = hub.get(app);
    if !st.is_authenticated || !is_checked_in(&st.shift) {
        return false;
    }
    // Meeting mode: pause heartbeats while meetings has an open entry.
    if has_open_meeting(&st.shift) {
        return false;
    }
    let threshold = (st.idle_timeout_minutes.max(1) as u64) * 60;
    if st.idle_seconds >= threshold {
        return false;
    }
    if matches!(
        open_break_source(&st.shift).as_deref(),
        Some("manual") | Some("idle") | Some("sleep")
    ) {
        return false;
    }
    let has_open = app
        .try_state::<Api>()
        .and_then(|api| api.db.lock().ok().map(|db| queue::has_open_pending(&db)))
        .unwrap_or(false);
    !has_open
}

pub fn record_sleep_or_offline_start(app: &AppHandle) {
    if !should_record_sleep(app) {
        return;
    }
    if let Some(api) = app.try_state::<Api>() {
        if let Ok(db) = api.db.lock() {
            queue::append_open_interval(&db, "sleep", &chrono::Utc::now().to_rfc3339());
        }
    }
    if let Some(hub) = app.try_state::<Hub>() {
        hub.emit(app);
    }
}

pub async fn flush_pending_breaks(app: &AppHandle) {
    let Some(api) = app.try_state::<Api>() else {
        return;
    };
    let closed = {
        let Ok(db) = api.db.lock() else { return };
        queue::closed_pending_breaks(&db)
    };
    if closed.is_empty() {
        if let Some(hub) = app.try_state::<Hub>() {
            hub.emit(app);
        }
        return;
    }
    for item in closed {
        let Some(end) = item.end_time.clone() else { continue };
        match api.record_break(&item.start_time, &end, &item.source).await {
            Ok(shift) => {
                if let Ok(db) = api.db.lock() {
                    queue::remove_pending_break(&db, &item.id);
                }
                apply_shift(app, Some(shift)).await;
                if let Some(hub) = app.try_state::<Hub>() {
                    hub.patch(app, |s| s.last_error = None);
                }
            }
            Err(e) if e.is_network() => {
                if let Some(hub) = app.try_state::<Hub>() {
                    hub.patch(app, |s| {
                        s.is_online = false;
                        s.last_error = Some(
                            "Offline — sleep/idle intervals will sync when the network returns".into(),
                        );
                    });
                }
                return;
            }
            Err(e) => {
                let lower = e.to_string().to_lowercase();
                if lower.contains("duplicate") || lower.contains("already") || lower.contains("idempotent") {
                    if let Ok(db) = api.db.lock() {
                        queue::remove_pending_break(&db, &item.id);
                    }
                    continue;
                }
                if let Some(hub) = app.try_state::<Hub>() {
                    hub.patch(app, |s| s.last_error = Some(e.to_string()));
                }
                return;
            }
        }
    }
    if let Some(hub) = app.try_state::<Hub>() {
        hub.emit(app);
    }
}

pub async fn flush_pending_heartbeats(app: &AppHandle, stopping: bool) {
    let Some(api) = app.try_state::<Api>() else {
        return;
    };
    loop {
        let beats = {
            let Ok(db) = api.db.lock() else { return };
            queue::list_heartbeats(&db, 500)
        };
        if beats.is_empty() && !stopping {
            return;
        }
        let last_batch = beats.len() < 500;
        match api.send_heartbeat(&beats, stopping && last_batch).await {
            Ok(shift) => {
                if !beats.is_empty() {
                    if let Ok(db) = api.db.lock() {
                        queue::remove_heartbeats(&db, &beats);
                    }
                }
                apply_shift(app, Some(shift)).await;
                if last_batch {
                    return;
                }
            }
            Err(e) if e.is_network() => {
                if let Some(hub) = app.try_state::<Hub>() {
                    hub.patch(app, |s| s.is_online = false);
                }
                return;
            }
            Err(e) => {
                if let Some(hub) = app.try_state::<Hub>() {
                    hub.patch(app, |s| s.last_error = Some(e.to_string()));
                }
                return;
            }
        }
    }
}

fn app_data_dir(app: &AppHandle) -> std::path::PathBuf {
    app.path()
        .app_data_dir()
        .unwrap_or_else(|_| std::env::temp_dir().join("leapsofts-erp"))
}

pub async fn close_open_and_flush(app: &AppHandle) {
    app_usage::close_open_usage_segment(app);
    if let Some(api) = app.try_state::<Api>() {
        if let Ok(db) = api.db.lock() {
            queue::close_open_intervals(&db, &chrono::Utc::now().to_rfc3339());
        }
    }
    if let Some(hub) = app.try_state::<Hub>() {
        hub.emit(app);
    }
    flush_pending_heartbeats(app, false).await;
    flush_pending_breaks(app).await;
    let _ = screenshots::flush_pending(app).await;
    let _ = app_usage::flush_pending(app).await;
}

pub async fn on_app_quit(app: &AppHandle) {
    close_open_and_flush(app).await;
    flush_pending_heartbeats(app, true).await;
}

async fn refresh_profile_and_shift(app: &AppHandle) -> Result<(), ApiError> {
    let api = app.state::<Api>();
    let user = api.fetch_me().await?;
    app.state::<Hub>().set_user(app, Some(user));
    let shift = api.fetch_today_shift().await?;
    apply_shift(app, shift).await;
    Ok(())
}

pub async fn bootstrap_session(app: &AppHandle) {
    let api = app.state::<Api>();
    api.load_token_bootstrap();
    let has_token = api.db.lock().ok().and_then(|db| queue::kv_get(&db, "access_token"));
    if has_token.is_none() {
        app.state::<Hub>().reset(app);
        screenshots::stop();
        app_usage::stop(app);
        return;
    }
    match refresh_profile_and_shift(app).await {
        Ok(()) => {
            app.state::<Hub>().patch(app, |s| s.last_error = None);
            close_open_and_flush(app).await;
        }
        Err(e) if e.is_network() => {
            app.state::<Hub>().patch(app, |s| s.is_online = false);
        }
        Err(_) => {
            app.state::<Hub>().reset(app);
        }
    }
}

pub enum LoginStep {
    Done,
    NeedsCode(String),
    NeedsSetup,
}

async fn complete_session(app: &AppHandle, user: crate::types::TrackerUser) -> Result<(), String> {
    let api = app.state::<Api>();
    app.state::<Hub>().set_user(app, Some(user));
    if let Ok(me) = api.fetch_me().await {
        app.state::<Hub>().set_user(app, Some(me));
    }
    let shift = api.fetch_today_shift().await.map_err(|e| e.to_string())?;
    apply_shift(app, shift).await;
    app.state::<Hub>().patch(app, |s| s.last_error = None);
    flush_pending_breaks(app).await;
    Ok(())
}

pub async fn login(app: &AppHandle, email: String, password: String) -> Result<LoginStep, String> {
    let api = app.state::<Api>();
    match api.login(&email, &password).await.map_err(|e| e.to_string())? {
        PasswordLogin::Session(user) => {
            complete_session(app, user).await?;
            Ok(LoginStep::Done)
        }
        PasswordLogin::NeedsCode(temp_token) => Ok(LoginStep::NeedsCode(temp_token)),
        PasswordLogin::NeedsSetup => Ok(LoginStep::NeedsSetup),
    }
}

pub async fn verify_two_factor(app: &AppHandle, temp_token: String, code: String) -> Result<(), String> {
    let user = app
        .state::<Api>()
        .verify_two_factor(&temp_token, &code)
        .await
        .map_err(|e| e.to_string())?;
    complete_session(app, user).await
}

pub async fn logout(app: &AppHandle) -> Result<(), String> {
    screenshots::stop();
    app_usage::stop(app);
    app.state::<Api>().logout_remote().await;
    app.state::<Hub>().reset(app);
    Ok(())
}

pub async fn user_check_in(app: &AppHandle) -> Result<(), String> {
    let _busy = ShiftBusyGuard::enter();
    let Some(shift) =
        finish_shift_mutation(app, app.state::<Api>().check_in().await).await?
    else {
        return Ok(());
    };
    apply_shift(app, Some(shift)).await;
    if let Some(api) = app.try_state::<Api>() {
        if let Ok(db) = api.db.lock() {
            queue::append_heartbeat(&db, &chrono::Utc::now().to_rfc3339());
        }
    }
    flush_pending_heartbeats(app, false).await;
    Ok(())
}

pub async fn user_check_out(app: &AppHandle) -> Result<(), String> {
    app_usage::close_open_usage_segment(app);
    flush_pending_heartbeats(app, false).await;
    flush_pending_breaks(app).await;
    let _ = screenshots::flush_pending(app).await;
    let _ = app_usage::flush_pending(app).await;
    let _busy = ShiftBusyGuard::enter();
    let Some(shift) =
        finish_shift_mutation(app, app.state::<Api>().check_out().await).await?
    else {
        return Ok(());
    };
    apply_shift(app, Some(shift)).await;
    if let Some(api) = app.try_state::<Api>() {
        if let Ok(db) = api.db.lock() {
            queue::clear_shift_telemetry(&db, &app_data_dir(app));
        }
    }
    if let Some(hub) = app.try_state::<Hub>() {
        hub.emit(app);
    }
    Ok(())
}

pub async fn user_start_break(app: &AppHandle) -> Result<(), String> {
    app_usage::close_open_usage_segment(app);
    let _ = screenshots::flush_pending(app).await;
    let _ = app_usage::flush_pending(app).await;
    let _busy = ShiftBusyGuard::enter();
    let Some(shift) =
        finish_shift_mutation(app, app.state::<Api>().start_break("manual").await).await?
    else {
        return Ok(());
    };
    apply_shift(app, Some(shift)).await;
    Ok(())
}

pub async fn user_end_break(app: &AppHandle) -> Result<(), String> {
    let _busy = ShiftBusyGuard::enter();
    let Some(shift) = finish_shift_mutation(app, app.state::<Api>().end_break().await).await?
    else {
        return Ok(());
    };
    apply_shift(app, Some(shift)).await;
    Ok(())
}

pub async fn user_start_meeting(app: &AppHandle) -> Result<(), String> {
    let _busy = ShiftBusyGuard::enter();
    let Some(shift) =
        finish_shift_mutation(app, app.state::<Api>().start_meeting().await).await?
    else {
        return Ok(());
    };
    apply_shift(app, Some(shift)).await;
    Ok(())
}

pub async fn user_end_meeting(app: &AppHandle) -> Result<(), String> {
    let _busy = ShiftBusyGuard::enter();
    let Some(shift) = finish_shift_mutation(app, app.state::<Api>().end_meeting().await).await?
    else {
        return Ok(());
    };
    apply_shift(app, Some(shift)).await;
    Ok(())
}

static IN_FLIGHT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static SHIFT_BUSY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static REFRESH_IN_FLIGHT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static ACTIVITY_STREAK: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

pub async fn on_idle_tick(app: &AppHandle) {
    let online = is_online();
    let was_online = app.state::<Hub>().get(app).is_online;
    app.state::<Hub>().patch(app, |s| s.is_online = online);
    if !was_online && online {
        close_open_and_flush(app).await;
    }
    maybe_log_and_flush_heartbeat(app, online).await;

    let idle_seconds = user_idle::UserIdle::get_time()
        .map(|t| t.as_seconds())
        .unwrap_or(0);
    app.state::<Hub>().patch(app, |s| s.idle_seconds = idle_seconds);

    if idle_seconds < 3 {
        ACTIVITY_STREAK.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    } else {
        ACTIVITY_STREAK.store(0, std::sync::atomic::Ordering::Relaxed);
    }

    if ACTIVITY_STREAK.load(std::sync::atomic::Ordering::Relaxed) >= 3 {
        maybe_end_idle_break(app).await;
    } else {
        maybe_start_idle_break(app, idle_seconds).await;
    }
    maybe_refresh_shift_from_server(app).await;
}

async fn maybe_start_idle_break(app: &AppHandle, idle_seconds: u64) {
    let st = app.state::<Hub>().get(app);
    if !st.is_authenticated || !st.is_online || !is_checked_in(&st.shift) {
        return;
    }
    // Meeting mode: do not open idle breaks.
    if has_open_meeting(&st.shift) {
        return;
    }
    if open_break_source(&st.shift).is_some() {
        return;
    }
    let has_open = app
        .try_state::<Api>()
        .and_then(|api| api.db.lock().ok().map(|db| queue::has_open_pending(&db)))
        .unwrap_or(false);
    if has_open {
        return;
    }
    if IN_FLIGHT.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    let threshold = (st.idle_timeout_minutes.max(1) as u64) * 60;
    if idle_seconds < threshold {
        IN_FLIGHT.store(false, std::sync::atomic::Ordering::SeqCst);
        return;
    }
    match app.state::<Api>().start_break("idle").await {
        Ok(shift) => {
            apply_shift(app, Some(shift)).await;
            app.state::<Hub>().patch(app, |s| s.last_error = None);
        }
        Err(e) if e.is_network() => {
            app.state::<Hub>().patch(app, |s| s.is_online = false);
        }
        Err(e) if is_stale_shift_conflict(&e) => {
            let message = e.to_string();
            if reconcile_shift_from_server(app).await.is_err() {
                app.state::<Hub>().patch(app, |s| {
                    s.last_error = Some(message);
                });
            }
        }
        Err(e) => {
            app.state::<Hub>().patch(app, |s| {
                s.last_error = Some(e.to_string());
            });
        }
    }
    IN_FLIGHT.store(false, std::sync::atomic::Ordering::SeqCst);
}

async fn maybe_end_idle_break(app: &AppHandle) {
    let st = app.state::<Hub>().get(app);
    if !st.is_online || open_break_source(&st.shift).as_deref() != Some("idle") {
        return;
    }
    if IN_FLIGHT.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    match app.state::<Api>().end_break().await {
        Ok(shift) => {
            apply_shift(app, Some(shift)).await;
            app.state::<Hub>().patch(app, |s| s.last_error = None);
        }
        Err(e) if e.is_network() => {
            app.state::<Hub>().patch(app, |s| s.is_online = false);
        }
        Err(e) if is_stale_shift_conflict(&e) => {
            let message = e.to_string();
            if reconcile_shift_from_server(app).await.is_err() {
                app.state::<Hub>().patch(app, |s| {
                    s.last_error = Some(message);
                });
            }
        }
        Err(e) => {
            app.state::<Hub>().patch(app, |s| s.last_error = Some(e.to_string()));
        }
    }
    IN_FLIGHT.store(false, std::sync::atomic::Ordering::SeqCst);
}

use std::time::Instant;
static LAST_TICK: std::sync::Mutex<Option<Instant>> = std::sync::Mutex::new(None);
static LAST_HEARTBEAT: std::sync::Mutex<Option<Instant>> = std::sync::Mutex::new(None);
static LAST_SHIFT_REFRESH: std::sync::Mutex<Option<Instant>> = std::sync::Mutex::new(None);

const SHIFT_REFRESH_SECS: u64 = 30;

fn shift_refresh_due() -> bool {
    let Ok(mut last) = LAST_SHIFT_REFRESH.lock() else {
        return false;
    };
    let due = last
        .map(|prev| prev.elapsed().as_secs() >= SHIFT_REFRESH_SECS)
        .unwrap_or(true);
    if due {
        *last = Some(Instant::now());
    }
    due
}

fn clear_shift_refresh_deadline() {
    if let Ok(mut last) = LAST_SHIFT_REFRESH.lock() {
        *last = None;
    }
}

/// While heartbeats are paused (checked in, on a break, or in a meeting), pull
/// today's shift so a server-side change replaces the local badge.
async fn maybe_refresh_shift_from_server(app: &AppHandle) {
    let st = app.state::<Hub>().get(app);
    let tracked = st.is_authenticated
        && st.is_online
        && (is_checked_in(&st.shift)
            || has_open_meeting(&st.shift)
            || open_break_source(&st.shift).is_some());
    if !tracked {
        return;
    }
    if IN_FLIGHT.load(std::sync::atomic::Ordering::SeqCst)
        || SHIFT_BUSY.load(std::sync::atomic::Ordering::SeqCst)
    {
        return;
    }
    if !shift_refresh_due() {
        return;
    }
    if IN_FLIGHT.load(std::sync::atomic::Ordering::SeqCst)
        || SHIFT_BUSY.load(std::sync::atomic::Ordering::SeqCst)
        || REFRESH_IN_FLIGHT.swap(true, std::sync::atomic::Ordering::SeqCst)
    {
        clear_shift_refresh_deadline();
        return;
    }
    match app.state::<Api>().fetch_today_shift().await {
        Ok(shift) => {
            apply_shift(app, shift).await;
            app.state::<Hub>().patch(app, |s| s.last_error = None);
        }
        Err(e) if e.is_network() => {
            app.state::<Hub>().patch(app, |s| s.is_online = false);
        }
        Err(e) => {
            app.state::<Hub>().patch(app, |s| s.last_error = Some(e.to_string()));
        }
    }
    REFRESH_IN_FLIGHT.store(false, std::sync::atomic::Ordering::SeqCst);
}

async fn maybe_log_and_flush_heartbeat(app: &AppHandle, online: bool) {
    if !should_log_heartbeat(app) {
        return;
    }
    let due = {
        let mut last = LAST_HEARTBEAT.lock().unwrap();
        let due = last
            .map(|prev| prev.elapsed().as_secs() >= 60)
            .unwrap_or(true);
        if due {
            *last = Some(Instant::now());
        }
        due
    };
    if !due {
        return;
    }
    if let Some(api) = app.try_state::<Api>() {
        if let Ok(db) = api.db.lock() {
            queue::append_heartbeat(&db, &chrono::Utc::now().to_rfc3339());
        }
    }
    if online {
        flush_pending_heartbeats(app, false).await;
    }
}

pub async fn on_idle_tick_with_sleep_gap(app: &AppHandle) {
    let now = Instant::now();
    let slept = {
        let mut last = LAST_TICK.lock().unwrap();
        let slept = last
            .map(|prev| now.duration_since(prev).as_secs() > 30)
            .unwrap_or(false);
        *last = Some(now);
        slept
    };
    if slept {
        record_sleep_or_offline_start(app);
        close_open_and_flush(app).await;
    }
    on_idle_tick(app).await;
}
