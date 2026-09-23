#[cfg(target_os = "macos")]
use std::io::Read;
#[cfg(target_os = "macos")]
use std::process::{Command, Stdio};
#[cfg(target_os = "macos")]
use std::time::{Duration, Instant};

#[cfg(target_os = "macos")]
enum BrowserKind {
    SafariLike(&'static str),
    ChromeLike(&'static str),
}

#[cfg(target_os = "macos")]
fn norm(s: &str) -> String {
    s.to_lowercase().replace(".app", "")
}

#[cfg(target_os = "macos")]
fn classify(app: &str, exec: &str) -> Option<BrowserKind> {
    let a = norm(app);
    let e = norm(exec);
    let hay = format!("{a} {e}");

    if a == "safari" || e == "safari" || hay.contains("safari") {
        return Some(BrowserKind::SafariLike("Safari"));
    }
    if a == "orion" || e == "orion" {
        return Some(BrowserKind::SafariLike("Orion"));
    }
    if hay.contains("chromium") {
        return Some(BrowserKind::ChromeLike("Chromium"));
    }
    if hay.contains("google chrome") || a == "chrome" || e == "google chrome" {
        return Some(BrowserKind::ChromeLike("Google Chrome"));
    }
    if hay.contains("brave") {
        return Some(BrowserKind::ChromeLike("Brave Browser"));
    }
    if hay.contains("microsoft edge") || a == "edge" || e.contains("msedge") {
        return Some(BrowserKind::ChromeLike("Microsoft Edge"));
    }
    if a == "arc" || e == "arc" {
        return Some(BrowserKind::ChromeLike("Arc"));
    }
    if a == "vivaldi" || e == "vivaldi" {
        return Some(BrowserKind::ChromeLike("Vivaldi"));
    }
    if a == "opera" || e == "opera" || hay.contains("opera") {
        return Some(BrowserKind::ChromeLike("Opera"));
    }
    if a == "dia" || e == "dia" {
        return Some(BrowserKind::ChromeLike("Dia"));
    }
    None
}

#[cfg(target_os = "macos")]
fn osascript(script: &str) -> Option<String> {
    let mut child = Command::new("osascript")
        .arg("-e")
        .arg(script)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    return None;
                }
                let mut buf = String::new();
                stdout.read_to_string(&mut buf).ok()?;
                let text = buf.trim().to_string();
                if text.is_empty() || text.eq_ignore_ascii_case("missing value") {
                    return None;
                }
                return Some(text);
            }
            Ok(None) if start.elapsed() > Duration::from_millis(1500) => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(_) => return None,
        }
    }
}

/// Best-effort current tab URL for the focused browser. macOS uses Automation (AppleScript).
pub fn current_browser_url(app: &str, exec: &str) -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        let script = match classify(app, exec)? {
            BrowserKind::SafariLike(name) => format!(
                r#"tell application "{name}" to get URL of current tab of front window"#
            ),
            BrowserKind::ChromeLike(name) => format!(
                r#"tell application "{name}" to get URL of active tab of front window"#
            ),
        };
        osascript(&script)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, exec);
        None
    }
}
