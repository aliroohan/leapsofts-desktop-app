use crate::queue;
use crate::types::{
    is_checked_in, open_break_source, TrackerState, TrackerUser, DEFAULT_IDLE_TIMEOUT_MINUTES,
};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager};

pub struct Hub {
    snapshot: Mutex<TrackerState>,
}

impl Hub {
    pub fn new() -> Self {
        Self {
            snapshot: Mutex::new(TrackerState::default()),
        }
    }

    pub fn get(&self, app: &AppHandle) -> TrackerState {
        let mut s = self.snapshot.lock().unwrap().clone();
        if let Some(api) = app.try_state::<crate::api::Api>() {
            if let Ok(db) = api.db.lock() {
                s.pending_count = queue::pending_count(&db);
            }
        }
        s
    }

    pub fn emit(&self, app: &AppHandle) {
        let state = self.get(app);
        let _ = app.emit("tracker-state", &state);
        crate::tray::rebuild(app, &state);
    }

    pub fn patch(&self, app: &AppHandle, f: impl FnOnce(&mut TrackerState)) {
        {
            let mut s = self.snapshot.lock().unwrap();
            f(&mut s);
            if let Some(api) = app.try_state::<crate::api::Api>() {
                if let Ok(db) = api.db.lock() {
                    s.pending_count = queue::pending_count(&db);
                }
            }
        }
        self.emit(app);
    }

    pub fn set_user(&self, app: &AppHandle, user: Option<TrackerUser>) {
        self.patch(app, |s| {
            let minutes = user
                .as_ref()
                .and_then(|u| u.idle_timeout_minutes)
                .filter(|m| *m > 0)
                .unwrap_or(DEFAULT_IDLE_TIMEOUT_MINUTES);
            s.user = user.clone();
            s.is_authenticated = user.is_some();
            s.idle_timeout_minutes = minutes;
        });
    }

    pub fn reset(&self, app: &AppHandle) {
        self.patch(app, |s| {
            let online = s.is_online;
            let monitoring_error = s.monitoring_error.clone();
            let last_sample = s.last_sample_at.clone();
            *s = TrackerState::default();
            s.is_online = online;
            s.monitoring_error = monitoring_error;
            s.last_sample_at = last_sample;
        });
    }
}

pub fn monitoring_should_run(state: &TrackerState) -> bool {
    state.is_authenticated && is_checked_in(&state.shift) && open_break_source(&state.shift).is_none()
}
