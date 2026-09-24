use crate::api::Api;
use crate::input;
use crate::queue;
use crate::state::{monitoring_should_run, Hub};
use crate::types::PendingActivitySample;
use image::{imageops::FilterType, DynamicImage};
use rand::Rng;
use std::fs;
use std::io::Cursor;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tauri::{AppHandle, Manager};
use uuid::Uuid;
#[cfg(not(target_os = "macos"))]
use xcap::Monitor;

const WINDOW_MS: u128 = 10 * 60 * 1000;
const SCREENSHOT_WIDTH: u32 = 1280;
const SCREENSHOT_HEIGHT: u32 = 800;
const JPEG_QUALITY: u8 = 45;

static RUNNING: AtomicBool = AtomicBool::new(false);
static LOOP_SPAWNED: AtomicBool = AtomicBool::new(false);

struct ActiveWindow {
    window_start: i64,
    seconds_elapsed: u64,
    keyboard_seconds: u64,
    mouse_seconds: u64,
    combined_seconds: u64,
    image_path: Option<PathBuf>,
}

fn data_dir(app: &AppHandle) -> PathBuf {
    app.path()
        .app_data_dir()
        .unwrap_or_else(|_| std::env::temp_dir().join("leapsofts-erp"))
}

const SCREEN_PERMISSION: &str = "Screen Recording permission is required — grant it in System Settings → Privacy & Security → Screen Recording, then quit and reopen the app.";

fn capture_jpeg() -> Result<Vec<u8>, String> {
    let rgba = capture_rgba()?;
    let dynimg = DynamicImage::ImageRgba8(rgba);
    let resized = dynimg.resize(SCREENSHOT_WIDTH, SCREENSHOT_HEIGHT, FilterType::Triangle);
    let mut buf = Cursor::new(Vec::new());
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, JPEG_QUALITY);
    encoder
        .encode_image(&resized)
        .map_err(|e| e.to_string())?;
    Ok(buf.into_inner())
}

/// The display the user is actually working on. A secondary display that is
/// only showing the desktop still has the frontmost app in the menu bar, so
/// grabbing the first monitor looks like a home-screen visit.
///
/// Do not use `SCShareableContent::snapshot()`. In screencapturekit 8.0.1 that
/// indexes a buffer whose length is still 0, so the first capture panics and
/// the screenshot task never runs again.
#[cfg(target_os = "macos")]
fn capture_rgba() -> Result<image::RgbaImage, String> {
    use screencapturekit::screenshot_manager::{CGImageExt, SCScreenshotManager};
    use screencapturekit::shareable_content::SCShareableContent;
    use screencapturekit::stream::configuration::SCStreamConfiguration;
    use screencapturekit::stream::content_filter::SCContentFilter;

    ensure_screen_capture_access();
    let content = SCShareableContent::get().map_err(capture_error)?;
    let displays = content.displays();
    if displays.is_empty() {
        return Err(SCREEN_PERMISSION.to_string());
    }
    let windows = content.windows();
    let front_pid = active_win_pos_rs::get_active_window()
        .ok()
        .map(|win| win.process_id as i32)
        .filter(|pid| *pid > 0);
    let display = focused_display(&displays, &windows, front_pid).unwrap_or(&displays[0]);

    let filter = SCContentFilter::create()
        .with_display(display)
        .with_excluding_windows(&[])
        .build();
    let (width, height) = cap_capture_size(display.width().max(1), display.height().max(1));
    let config = SCStreamConfiguration::new()
        .with_width(width)
        .with_height(height)
        .with_scales_to_fit(true);

    let image = SCScreenshotManager::capture_image(&filter, &config).map_err(capture_error)?;
    let pixels = image.rgba_data().map_err(capture_error)?;
    let w = image.width() as u32;
    let h = image.height() as u32;
    image::RgbaImage::from_raw(w, h, pixels)
        .ok_or_else(|| "Screen capture returned an unexpected image".to_string())
}

#[cfg(target_os = "macos")]
fn ensure_screen_capture_access() {
    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGPreflightScreenCaptureAccess() -> bool;
        fn CGRequestScreenCaptureAccess() -> bool;
    }
    unsafe {
        if !CGPreflightScreenCaptureAccess() {
            CGRequestScreenCaptureAccess();
        }
    }
}

#[cfg(target_os = "macos")]
fn capture_error(err: impl std::fmt::Display) -> String {
    let msg = err.to_string();
    let lower = msg.to_ascii_lowercase();
    if lower.contains("permission") || lower.contains("not authorized") || lower.contains("tcc") {
        SCREEN_PERMISSION.to_string()
    } else {
        format!("Screen capture failed: {msg}")
    }
}

#[cfg(target_os = "macos")]
fn focused_display<'a>(
    displays: &'a [screencapturekit::shareable_content::SCDisplay],
    windows: &'a [screencapturekit::shareable_content::SCWindow],
    front_pid: Option<i32>,
) -> Option<&'a screencapturekit::shareable_content::SCDisplay> {
    use screencapturekit::cg::CGRect;
    use screencapturekit::shareable_content::SCWindow;

    let area = |window: &SCWindow| {
        let frame = window.frame();
        (frame.size.width * frame.size.height).max(0.0)
    };
    let contains = |frame: &CGRect, x: f64, y: f64| {
        x >= frame.origin.x
            && y >= frame.origin.y
            && x < frame.origin.x + frame.size.width
            && y < frame.origin.y + frame.size.height
    };
    let usable: Vec<&SCWindow> = windows
        .iter()
        .filter(|w| {
            let frame = w.frame();
            w.is_on_screen()
                && w.window_layer() == 0
                && frame.size.width >= 200.0
                && frame.size.height >= 200.0
        })
        .collect();
    let focused = front_pid.and_then(|pid| {
        usable
            .iter()
            .copied()
            .filter(|w| {
                w.owning_application()
                    .is_some_and(|app| app.process_id() == pid)
            })
            .max_by(|a, b| area(a).total_cmp(&area(b)))
    });
    let window = focused.or_else(|| usable.into_iter().max_by(|a, b| area(a).total_cmp(&area(b))))?;
    let frame = window.frame();
    let cx = frame.origin.x + frame.size.width / 2.0;
    let cy = frame.origin.y + frame.size.height / 2.0;
    displays.iter().find(|d| contains(&d.frame(), cx, cy))
}

#[cfg(target_os = "macos")]
fn cap_capture_size(width: u32, height: u32) -> (u32, u32) {
    const MAX_EDGE: u32 = 1600;
    let longest = width.max(height);
    let (width, height) = if longest <= MAX_EDGE || longest == 0 {
        (width.max(1), height.max(1))
    } else if width >= height {
        let h = (u64::from(height) * u64::from(MAX_EDGE) / u64::from(width)).max(1) as u32;
        (MAX_EDGE, h)
    } else {
        let w = (u64::from(width) * u64::from(MAX_EDGE) / u64::from(height)).max(1) as u32;
        (w, MAX_EDGE)
    };
    (width.max(2) & !1, height.max(2) & !1)
}

#[cfg(not(target_os = "macos"))]
fn capture_rgba() -> Result<image::RgbaImage, String> {
    let monitors = Monitor::all().map_err(|e| e.to_string())?;
    let primary = monitors
        .into_iter()
        .next()
        .ok_or_else(|| SCREEN_PERMISSION.to_string())?;
    primary.capture_image().map_err(|_| SCREEN_PERMISSION.to_string())
}

fn aligned_slot_start(now_ms: i64) -> i64 {
    now_ms - (now_ms % WINDOW_MS as i64)
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn iso(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms)
        .unwrap_or_else(chrono::Utc::now)
        .to_rfc3339()
}

pub fn start(app: AppHandle) {
    let hook_ok = input::ensure_started();
    if let Some(hub) = app.try_state::<Hub>() {
        hub.patch(&app, |s| {
            s.monitoring_active = hook_ok;
            if hook_ok {
                s.monitoring_error = None;
            } else {
                s.monitoring_error = Some(
                    "Input monitoring unavailable — grant Accessibility/Input Monitoring permission in System Settings, then check in again."
                        .into(),
                );
            }
        });
    }
    RUNNING.store(true, Ordering::SeqCst);
    if LOOP_SPAWNED.swap(true, Ordering::SeqCst) {
        return;
    }
    let flush_app = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            if !RUNNING.load(Ordering::SeqCst) {
                tokio::time::sleep(Duration::from_secs(1)).await;
                continue;
            }
            let should = app
                .try_state::<Hub>()
                .map(|h| monitoring_should_run(&h.get(&app)) && h.get(&app).user.as_ref().map(|u| u.wants_screenshots()).unwrap_or(true))
                .unwrap_or(false);
            if !should {
                tokio::time::sleep(Duration::from_secs(1)).await;
                continue;
            }

            let now = now_ms();
            let window_start = aligned_slot_start(now);
            let ms_into = now % WINDOW_MS as i64;
            let ms_until_end = if ms_into == 0 { WINDOW_MS as i64 } else { WINDOW_MS as i64 - ms_into };
            let remaining_sec = (ms_until_end / 1000).max(1) as u64;
            let offset_sec = if remaining_sec <= 1 {
                0
            } else {
                rand::thread_rng().gen_range(0..remaining_sec)
            };

            let mut win = ActiveWindow {
                window_start,
                seconds_elapsed: 0,
                keyboard_seconds: 0,
                mouse_seconds: 0,
                combined_seconds: 0,
                image_path: None,
            };

            let capture_at = tokio::time::Instant::now() + Duration::from_secs(offset_sec);
            let end_at = tokio::time::Instant::now() + Duration::from_millis(ms_until_end as u64);
            let mut captured = false;

            while tokio::time::Instant::now() < end_at {
                if !RUNNING.load(Ordering::SeqCst) {
                    break;
                }
                let should = app
                    .try_state::<Hub>()
                    .map(|h| monitoring_should_run(&h.get(&app)))
                    .unwrap_or(false);
                if !should {
                    break;
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
                let (k, m) = input::last_tick();
                win.seconds_elapsed += 1;
                if k {
                    win.keyboard_seconds += 1;
                }
                if m {
                    win.mouse_seconds += 1;
                }
                if k || m {
                    win.combined_seconds += 1;
                }
                if !captured && tokio::time::Instant::now() >= capture_at {
                    captured = true;
                    // Blocking ScreenCaptureKit wait must not run on the async worker.
                    // A panic there used to abort this task permanently.
                    let shot = tokio::task::spawn_blocking(capture_jpeg).await;
                    match shot {
                        Ok(Ok(jpeg)) => {
                            let dir = queue::image_dir(&data_dir(&app));
                            let path = dir.join(format!("{}-{}.jpg", win.window_start, rand::thread_rng().gen_range(0..1_000_000)));
                            if fs::write(&path, jpeg).is_ok() {
                                win.image_path = Some(path);
                            }
                            if let Some(hub) = app.try_state::<Hub>() {
                                hub.patch(&app, |s| s.monitoring_error = None);
                            }
                        }
                        Ok(Err(msg)) => {
                            if let Some(hub) = app.try_state::<Hub>() {
                                hub.patch(&app, |s| s.monitoring_error = Some(msg));
                            }
                        }
                        Err(_) => {
                            if let Some(hub) = app.try_state::<Hub>() {
                                hub.patch(&app, |s| {
                                    s.monitoring_error = Some("Screen capture failed".into());
                                });
                            }
                        }
                    }
                }
            }

            enqueue_window(&app, &win);
            let _ = flush_pending(&app).await;
            let _ = crate::app_usage::flush_pending(&app).await;
        }
    });

    tauri::async_runtime::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(60)).await;
                if !RUNNING.load(Ordering::SeqCst) {
                    continue;
                }
                let _ = flush_pending(&flush_app).await;
                let _ = crate::app_usage::flush_pending(&flush_app).await;
            }
        });
}

pub fn stop() {
    RUNNING.store(false, Ordering::SeqCst);
}

fn enqueue_window(app: &AppHandle, win: &ActiveWindow) {
    let Some(path) = &win.image_path else { return };
    if win.seconds_elapsed == 0 {
        return;
    }
    let total = win.seconds_elapsed as f64;
    let sample = PendingActivitySample {
        id: Uuid::new_v4().to_string(),
        window_start: iso(win.window_start),
        captured_at: chrono::Utc::now().to_rfc3339(),
        keyboard_pct: ((win.keyboard_seconds as f64 / total) * 100.0).round() as i32,
        mouse_pct: ((win.mouse_seconds as f64 / total) * 100.0).round() as i32,
        combined_pct: ((win.combined_seconds as f64 / total) * 100.0).round() as i32,
        image_path: path.to_string_lossy().to_string(),
    };
    if let Some(api) = app.try_state::<Api>() {
        if let Ok(db) = api.db.lock() {
            queue::enqueue_sample(&db, &sample);
        }
    }
    if let Some(hub) = app.try_state::<Hub>() {
        hub.patch(app, |s| s.last_sample_at = Some(sample.captured_at.clone()));
    }
}

pub async fn flush_pending(app: &AppHandle) -> Result<(), String> {
    let Some(api) = app.try_state::<Api>() else {
        return Ok(());
    };
    let pending = {
        let db = api.db.lock().map_err(|e| e.to_string())?;
        queue::list_samples(&db)
    };
    for item in pending {
        let jpeg = match fs::read(&item.image_path) {
            Ok(b) => b,
            Err(_) => {
                if let Ok(db) = api.db.lock() {
                    queue::remove_sample(&db, &item.id, &item.image_path);
                }
                continue;
            }
        };
        match api
            .upload_activity_sample(
                &item.window_start,
                &item.captured_at,
                item.keyboard_pct,
                item.mouse_pct,
                item.combined_pct,
                jpeg,
            )
            .await
        {
            Ok(()) => {
                if let Ok(db) = api.db.lock() {
                    queue::remove_sample(&db, &item.id, &item.image_path);
                }
                if let Some(hub) = app.try_state::<Hub>() {
                    hub.patch(app, |s| s.last_error = None);
                }
            }
            Err(e) if e.is_network() => {
                if let Some(hub) = app.try_state::<Hub>() {
                    hub.patch(app, |s| {
                        s.is_online = false;
                    });
                }
                return Ok(());
            }
            Err(_) => {
                if let Ok(db) = api.db.lock() {
                    queue::remove_sample(&db, &item.id, &item.image_path);
                }
            }
        }
    }
    Ok(())
}
