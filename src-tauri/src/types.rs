use serde::{Deserialize, Serialize};

pub const DEFAULT_IDLE_TIMEOUT_MINUTES: u32 = 5;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum BreakSource {
    Manual,
    Idle,
    Sleep,
    Offline,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShiftBreak {
    pub start_time: String,
    pub end_time: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<BreakSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Shift {
    #[serde(rename = "_id", default)]
    pub id: String,
    #[serde(default)]
    pub user_id: String,
    #[serde(default)]
    pub date: String,
    pub check_in_time: Option<String>,
    pub check_out_time: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_minutes: Option<f64>,
    #[serde(default)]
    pub breaks: Vec<ShiftBreak>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_break_minutes: Option<f64>,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_active: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackerUser {
    #[serde(rename = "_id", default)]
    pub id: String,
    #[serde(default)]
    pub email: String,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub idle_timeout_minutes: Option<u32>,
    pub monitor_screenshots: Option<bool>,
    pub monitor_app_usage: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackingSummary {
    pub app: Option<String>,
    pub domain: Option<String>,
    pub pending_segments: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackerState {
    pub is_authenticated: bool,
    pub user: Option<TrackerUser>,
    pub shift: Option<Shift>,
    pub idle_seconds: u64,
    pub idle_timeout_minutes: u32,
    pub is_online: bool,
    pub pending_count: u32,
    pub last_error: Option<String>,
    pub monitoring_active: bool,
    pub monitoring_error: Option<String>,
    pub last_sample_at: Option<String>,
    pub tracking_summary: Option<TrackingSummary>,
}

impl Default for TrackerState {
    fn default() -> Self {
        Self {
            is_authenticated: false,
            user: None,
            shift: None,
            idle_seconds: 0,
            idle_timeout_minutes: DEFAULT_IDLE_TIMEOUT_MINUTES,
            is_online: true,
            pending_count: 0,
            last_error: None,
            monitoring_active: false,
            monitoring_error: None,
            last_sample_at: None,
            tracking_summary: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingBreak {
    pub id: String,
    pub start_time: String,
    pub end_time: Option<String>,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingActivitySample {
    pub id: String,
    pub window_start: String,
    pub captured_at: String,
    pub keyboard_pct: i32,
    pub mouse_pct: i32,
    pub combined_pct: i32,
    pub image_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppUsageSegment {
    pub client_id: String,
    pub app: String,
    pub exec_name: String,
    pub title: String,
    pub url: Option<String>,
    pub domain: Option<String>,
    pub started_at: String,
    pub ended_at: String,
    pub duration_sec: u64,
    pub active_sec: u64,
    pub platform: String,
}

impl TrackerUser {
    pub fn from_raw(v: serde_json::Value) -> Self {
        let id = v
            .get("_id")
            .or_else(|| v.get("id"))
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        Self {
            id,
            email: v.get("email").and_then(|x| x.as_str()).unwrap_or("").to_string(),
            first_name: v.get("firstName").and_then(|x| x.as_str()).map(|s| s.to_string()),
            last_name: v.get("lastName").and_then(|x| x.as_str()).map(|s| s.to_string()),
            idle_timeout_minutes: v
                .get("idleTimeoutMinutes")
                .and_then(|x| x.as_u64())
                .map(|n| n as u32),
            monitor_screenshots: v.get("monitorScreenshots").and_then(|x| x.as_bool()),
            monitor_app_usage: v.get("monitorAppUsage").and_then(|x| x.as_bool()),
        }
    }

    pub fn wants_screenshots(&self) -> bool {
        self.monitor_screenshots != Some(false)
    }

    pub fn wants_app_usage(&self) -> bool {
        self.monitor_app_usage != Some(false)
    }
}

pub fn open_break_source(shift: &Option<Shift>) -> Option<String> {
    let shift = shift.as_ref()?;
    let last = shift.breaks.last()?;
    if last.end_time.is_some() {
        return None;
    }
    Some(
        last.source
            .as_ref()
            .map(|s| match s {
                BreakSource::Manual => "manual",
                BreakSource::Idle => "idle",
                BreakSource::Sleep => "sleep",
                BreakSource::Offline => "offline",
            })
            .unwrap_or("manual")
            .to_string(),
    )
}

pub fn is_checked_in(shift: &Option<Shift>) -> bool {
    match shift {
        Some(s) => s.status == "checked_in" && s.check_in_time.is_some(),
        None => false,
    }
}
