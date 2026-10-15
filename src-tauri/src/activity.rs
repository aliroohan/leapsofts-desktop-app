//! Desktop activity capture. On Android these calls do nothing:
//! the phone app is attendance only (check-in, break, meeting, sync).

use tauri::AppHandle;

pub fn start_screenshots(app: AppHandle) {
    #[cfg(desktop)]
    crate::screenshots::start(app);
    #[cfg(not(desktop))]
    let _ = app;
}

pub fn stop_screenshots() {
    #[cfg(desktop)]
    crate::screenshots::stop();
}

pub async fn flush_screenshots(app: &AppHandle) {
    #[cfg(desktop)]
    {
        let _ = crate::screenshots::flush_pending(app).await;
    }
    #[cfg(not(desktop))]
    let _ = app;
}

pub fn start_usage(app: AppHandle) {
    #[cfg(desktop)]
    crate::app_usage::start(app);
    #[cfg(not(desktop))]
    let _ = app;
}

pub fn stop_usage(app: &AppHandle) {
    #[cfg(desktop)]
    crate::app_usage::stop(app);
    #[cfg(not(desktop))]
    let _ = app;
}

pub fn close_usage(app: &AppHandle) {
    #[cfg(desktop)]
    crate::app_usage::close_open_usage_segment(app);
    #[cfg(not(desktop))]
    let _ = app;
}

pub async fn flush_usage(app: &AppHandle) {
    #[cfg(desktop)]
    {
        let _ = crate::app_usage::flush_pending(app).await;
    }
    #[cfg(not(desktop))]
    let _ = app;
}

#[cfg_attr(not(desktop), allow(dead_code))]
pub fn system_idle_seconds() -> u64 {
    #[cfg(desktop)]
    {
        user_idle::UserIdle::get_time()
            .map(|t| t.as_seconds())
            .unwrap_or(0)
    }
    #[cfg(not(desktop))]
    {
        0
    }
}
