use crate::queue;
use crate::types::{AppUsageSegment, Shift, TrackerUser};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, COOKIE, SET_COOKIE};
use reqwest::multipart;
use rusqlite::Connection;
use serde_json::Value;
use std::sync::Mutex;
use std::time::Duration;

pub const DEFAULT_API_BASE: &str =
    "https://leapsofts-erp-backend-f9122a2e0426.herokuapp.com/api/v1";

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("{0}")]
    Message(String),
    #[error("{0}")]
    Network(String),
}

impl ApiError {
    pub fn is_network(&self) -> bool {
        matches!(self, ApiError::Network(_))
    }
}

pub fn api_base() -> String {
    std::env::var("VITE_API_URL")
        .or_else(|_| std::env::var("API_BASE"))
        .ok()
        .or_else(|| option_env!("VITE_API_URL").map(str::to_string))
        .or_else(|| option_env!("API_BASE").map(str::to_string))
        .unwrap_or_else(|| DEFAULT_API_BASE.to_string())
        .trim_end_matches('/')
        .to_string()
}

pub enum PasswordLogin {
    Session(TrackerUser),
    NeedsCode(String),
    NeedsSetup,
}

pub struct Api {
    client: reqwest::Client,
    pub db: Mutex<Connection>,
}

impl Api {
    pub fn new(conn: Connection) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("http client");
        Self {
            client,
            db: Mutex::new(conn),
        }
    }

    fn token(&self) -> Option<String> {
        let db = self.db.lock().ok()?;
        queue::kv_get(&db, "access_token")
    }

    fn refresh_cookie(&self) -> Option<String> {
        let db = self.db.lock().ok()?;
        queue::kv_get(&db, "refresh_cookie")
    }

    pub fn save_token(&self, token: &str) {
        if let Ok(db) = self.db.lock() {
            queue::kv_set(&db, "access_token", token);
        }
        let _ = keyring::Entry::new("com.leapsofts.erp", "access_token")
            .and_then(|e| e.set_password(token));
    }

    pub fn load_token_bootstrap(&self) {
        if self.token().is_some() {
            return;
        }
        if let Ok(entry) = keyring::Entry::new("com.leapsofts.erp", "access_token") {
            if let Ok(pw) = entry.get_password() {
                if !pw.is_empty() {
                    self.save_token(&pw);
                }
            }
        }
    }

    pub fn clear_session(&self) {
        if let Ok(db) = self.db.lock() {
            queue::kv_del(&db, "access_token");
            queue::kv_del(&db, "refresh_cookie");
        }
        let _ = keyring::Entry::new("com.leapsofts.erp", "access_token").and_then(|e| e.delete_credential());
    }

    fn store_refresh_from_headers(&self, headers: &HeaderMap) {
        for value in headers.get_all(SET_COOKIE) {
            if let Ok(s) = value.to_str() {
                if let Some(cookie) = parse_refresh_cookie(s) {
                    if let Ok(db) = self.db.lock() {
                        queue::kv_set(&db, "refresh_cookie", &cookie);
                    }
                }
            }
        }
    }

    async fn refresh_access_token(&self) -> Option<String> {
        let cookie = self.refresh_cookie()?;
        let url = format!("{}/auth/refresh", api_base());
        let mut headers = HeaderMap::new();
        if let Ok(v) = HeaderValue::from_str(&format!("refreshToken={cookie}")) {
            headers.insert(COOKIE, v);
        }
        let res = self.client.post(url).headers(headers).send().await.ok()?;
        self.store_refresh_from_headers(res.headers());
        if !res.status().is_success() {
            return None;
        }
        let body: Value = res.json().await.ok()?;
        let token = body
            .get("data")
            .and_then(|d| d.get("accessToken"))
            .and_then(|t| t.as_str())
            .map(|s| s.to_string())?;
        self.save_token(&token);
        Some(token)
    }

    async fn send_json(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
        retry: bool,
    ) -> Result<Value, ApiError> {
        let url = format!("{}{path}", api_base());
        let mut req = self.client.request(method.clone(), &url);
        req = req.header("X-Leapsofts-Client", "tracker");
        if let Some(token) = self.token() {
            req = req.header(AUTHORIZATION, format!("Bearer {token}"));
        }
        if let Some(cookie) = self.refresh_cookie() {
            req = req.header(COOKIE, format!("refreshToken={cookie}"));
        }
        if let Some(b) = body.clone() {
            req = req.json(&b);
        }
        let res = req.send().await.map_err(|e| ApiError::Network(e.to_string()))?;
        self.store_refresh_from_headers(res.headers());
        let status = res.status();
        if status.as_u16() == 401 && retry && !path.contains("/auth/login") {
            if self.refresh_access_token().await.is_some() {
                return Box::pin(self.send_json(method, path, body, false)).await;
            }
        }
        let text = res.text().await.unwrap_or_default();
        let parsed: Value = serde_json::from_str(&text).unwrap_or_else(|_| {
            serde_json::json!({ "message": text })
        });
        if !status.is_success() {
            let message = parsed
                .pointer("/error/message")
                .or_else(|| parsed.get("message"))
                .and_then(|v| v.as_str())
                .unwrap_or("Request failed")
                .to_string();
            return Err(ApiError::Message(message));
        }
        Ok(parsed.get("data").cloned().unwrap_or(parsed))
    }

    pub async fn login(&self, email: &str, password: &str) -> Result<PasswordLogin, ApiError> {
        let data = self
            .send_json(
                reqwest::Method::POST,
                "/auth/login",
                Some(serde_json::json!({ "email": email, "password": password })),
                false,
            )
            .await?;
        if data.get("requires2FA").and_then(|v| v.as_bool()) == Some(true) {
            let temp_token = data
                .get("tempToken")
                .and_then(|t| t.as_str())
                .filter(|t| !t.is_empty())
                .ok_or_else(|| ApiError::Message("Two-factor challenge is missing a token".into()))?;
            return Ok(PasswordLogin::NeedsCode(temp_token.to_string()));
        }
        if data.get("requires2FASetup").and_then(|v| v.as_bool()) == Some(true) {
            return Ok(PasswordLogin::NeedsSetup);
        }
        Ok(PasswordLogin::Session(self.session_user(data)?))
    }

    pub async fn verify_two_factor(&self, temp_token: &str, code: &str) -> Result<TrackerUser, ApiError> {
        let data = self
            .send_json(
                reqwest::Method::POST,
                "/auth/2fa/verify-login",
                Some(serde_json::json!({ "tempToken": temp_token, "code": code })),
                false,
            )
            .await?;
        self.session_user(data)
    }

    fn session_user(&self, data: Value) -> Result<TrackerUser, ApiError> {
        let token = data
            .get("accessToken")
            .and_then(|t| t.as_str())
            .filter(|t| !t.is_empty())
            .ok_or_else(|| ApiError::Message("Sign-in did not return a session".into()))?;
        self.save_token(token);
        let user = data.get("user").cloned().unwrap_or(Value::Null);
        Ok(TrackerUser::from_raw(user))
    }

    pub async fn fetch_me(&self) -> Result<TrackerUser, ApiError> {
        let data = self.send_json(reqwest::Method::GET, "/users/me", None, true).await?;
        Ok(TrackerUser::from_raw(data))
    }

    pub async fn fetch_today_shift(&self) -> Result<Option<Shift>, ApiError> {
        match self.send_json(reqwest::Method::GET, "/shifts/today", None, true).await {
            Ok(Value::Null) => Ok(None),
            Ok(v) => Ok(serde_json::from_value(v).ok()),
            Err(e) if e.to_string().contains("404") || e.to_string().contains("400") => Ok(None),
            Err(e) => Err(e),
        }
    }

    pub async fn check_in(&self) -> Result<Shift, ApiError> {
        let data = self
            .send_json(reqwest::Method::POST, "/shifts/check-in", None, true)
            .await?;
        serde_json::from_value(data).map_err(|e| ApiError::Message(e.to_string()))
    }

    pub async fn check_out(&self) -> Result<Shift, ApiError> {
        let data = self
            .send_json(reqwest::Method::POST, "/shifts/check-out", None, true)
            .await?;
        serde_json::from_value(data).map_err(|e| ApiError::Message(e.to_string()))
    }

    pub async fn start_break(&self, source: &str) -> Result<Shift, ApiError> {
        let data = self
            .send_json(
                reqwest::Method::POST,
                "/shifts/break/start",
                Some(serde_json::json!({ "source": source })),
                true,
            )
            .await?;
        serde_json::from_value(data).map_err(|e| ApiError::Message(e.to_string()))
    }

    pub async fn end_break(&self) -> Result<Shift, ApiError> {
        let data = self
            .send_json(reqwest::Method::POST, "/shifts/break/end", None, true)
            .await?;
        serde_json::from_value(data).map_err(|e| ApiError::Message(e.to_string()))
    }

    pub async fn send_heartbeat(&self, beats: &[String], stopping: bool) -> Result<Shift, ApiError> {
        let data = self
            .send_json(
                reqwest::Method::POST,
                "/shifts/heartbeat",
                Some(serde_json::json!({ "beats": beats, "stopping": stopping })),
                true,
            )
            .await?;
        serde_json::from_value(data).map_err(|e| ApiError::Message(e.to_string()))
    }

    pub async fn record_break(&self, start: &str, end: &str, source: &str) -> Result<Shift, ApiError> {
        let data = self
            .send_json(
                reqwest::Method::POST,
                "/shifts/break/record",
                Some(serde_json::json!({
                    "startTime": start,
                    "endTime": end,
                    "source": source
                })),
                true,
            )
            .await?;
        serde_json::from_value(data).map_err(|e| ApiError::Message(e.to_string()))
    }

    pub async fn logout_remote(&self) {
        let _ = self
            .send_json(reqwest::Method::POST, "/auth/logout", None, true)
            .await;
        self.clear_session();
    }

    pub async fn upload_activity_sample(
        &self,
        window_start: &str,
        captured_at: &str,
        keyboard_pct: i32,
        mouse_pct: i32,
        combined_pct: i32,
        jpeg: Vec<u8>,
    ) -> Result<(), ApiError> {
        let mut attempt = 0;
        loop {
            let url = format!("{}/shifts/activity-samples", api_base());
            let part = multipart::Part::bytes(jpeg.clone())
                .file_name("sample.jpg")
                .mime_str("image/jpeg")
                .unwrap_or_else(|_| multipart::Part::bytes(jpeg.clone()));
            let form = multipart::Form::new()
                .text("windowStart", window_start.to_string())
                .text("capturedAt", captured_at.to_string())
                .text("keyboardPct", keyboard_pct.to_string())
                .text("mousePct", mouse_pct.to_string())
                .text("combinedPct", combined_pct.to_string())
                .part("image", part);
            let mut req = self.client.post(&url).multipart(form);
            req = req.header("X-Leapsofts-Client", "tracker");
            if let Some(token) = self.token() {
                req = req.header(AUTHORIZATION, format!("Bearer {token}"));
            }
            let res = req.send().await.map_err(|e| ApiError::Network(e.to_string()))?;
            let status = res.status();
            if status.as_u16() == 401 && attempt == 0 {
                attempt += 1;
                if self.refresh_access_token().await.is_some() {
                    continue;
                }
            }
            if !status.is_success() {
                let text = res.text().await.unwrap_or_default();
                let parsed: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
                let message = parsed
                    .pointer("/error/message")
                    .or_else(|| parsed.get("message"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("Upload failed")
                    .to_string();
                return Err(ApiError::Message(message));
            }
            return Ok(());
        }
    }

    pub async fn upload_app_usage(&self, segments: &[AppUsageSegment]) -> Result<(), ApiError> {
        if segments.is_empty() {
            return Ok(());
        }
        self.send_json(
            reqwest::Method::POST,
            "/shifts/app-usage",
            Some(serde_json::json!({ "segments": segments })),
            true,
        )
        .await?;
        Ok(())
    }
}

fn parse_refresh_cookie(set_cookie: &str) -> Option<String> {
    for part in set_cookie.split(';') {
        let part = part.trim();
        if let Some(rest) = part.strip_prefix("refreshToken=") {
            if !rest.is_empty() {
                return Some(rest.to_string());
            }
        }
    }
    None
}

pub fn is_online() -> bool {
    use std::net::ToSocketAddrs;
    let host = api_base()
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .split('/')
        .next()
        .unwrap_or("leapsofts-erp-backend-f9122a2e0426.herokuapp.com")
        .split(':')
        .next()
        .unwrap_or("leapsofts-erp-backend-f9122a2e0426.herokuapp.com")
        .to_string();
    let port = if api_base().starts_with("http://") { 80 } else { 443 };
    let Ok(mut addrs) = format!("{host}:{port}").to_socket_addrs() else {
        return false;
    };
    let Some(addr) = addrs.next() else {
        return false;
    };
    std::net::TcpStream::connect_timeout(&addr, Duration::from_secs(2)).is_ok()
}
