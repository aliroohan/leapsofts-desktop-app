use crate::browser_url::current_browser_url;
use crate::url_sanitize::sanitize_url;

pub struct ActiveWindowSnapshot {
    pub app: String,
    pub exec_name: String,
    pub title: String,
    pub raw_url: Option<String>,
}

pub fn current_platform() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else {
        "linux"
    }
}

pub fn get_active_window() -> Result<Option<ActiveWindowSnapshot>, String> {
    match active_win_pos_rs::get_active_window() {
        Ok(win) => {
            let app = win.app_name.trim().to_string();
            let exec = win
                .process_path
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            if app.is_empty() && exec.is_empty() {
                return Ok(None);
            }
            let title = win.title.trim().to_string();
            let app_name = if app.is_empty() { exec.clone() } else { app.clone() };
            let raw_url = current_browser_url(&app_name, &exec).or_else(|| infer_url(&title));
            Ok(Some(ActiveWindowSnapshot {
                app: if app.is_empty() { exec.clone() } else { app },
                exec_name: exec,
                title,
                raw_url,
            }))
        }
        Err(_) => Err("Window tracking unavailable".into()),
    }
}

fn infer_url(title: &str) -> Option<String> {
    for part in title.split([' ', '|', '—', '–', '-', '\n']) {
        let part = part.trim().trim_end_matches([',', ';', ')', ']']);
        if part.starts_with("http://") || part.starts_with("https://") {
            return Some(part.to_string());
        }
        if part.starts_with("www.") && part.contains('.') {
            return Some(format!("https://{part}"));
        }
    }
    None
}

pub fn sanitized_from_snapshot(snap: &ActiveWindowSnapshot) -> (Option<String>, Option<String>) {
    match sanitize_url(snap.raw_url.as_deref()) {
        Some(s) => (Some(s.url), Some(s.domain)),
        None => (None, None),
    }
}
