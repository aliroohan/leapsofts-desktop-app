mod active_window;
mod api;
mod app_usage;
mod browser_url;
mod input;
mod queue;
mod screenshots;
mod session;
mod state;
mod tray;
mod types;
mod url_sanitize;

use api::Api;
use queue::open_db;
use state::Hub;
use tauri::Manager;
use tauri::{WebviewUrl, WebviewWindowBuilder};

const ERP_URL: &str = "https://erp.leapsofts.com/projects";

#[tauri::command]
fn get_state(app: tauri::AppHandle) -> types::TrackerState {
    app.state::<Hub>().get(&app)
}

#[derive(serde::Serialize)]
#[serde(tag = "kind")]
enum LoginResult {
    #[serde(rename = "authenticated")]
    Authenticated { state: types::TrackerState },
    #[serde(rename = "requires2FA")]
    Requires2FA {
        #[serde(rename = "tempToken")]
        temp_token: String,
    },
    #[serde(rename = "requires2FASetup")]
    Requires2FASetup,
}

#[tauri::command]
async fn login(app: tauri::AppHandle, email: String, password: String) -> Result<LoginResult, String> {
    match session::login(&app, email, password).await? {
        session::LoginStep::Done => Ok(LoginResult::Authenticated {
            state: app.state::<Hub>().get(&app),
        }),
        session::LoginStep::NeedsCode(temp_token) => Ok(LoginResult::Requires2FA { temp_token }),
        session::LoginStep::NeedsSetup => Ok(LoginResult::Requires2FASetup),
    }
}

#[tauri::command]
async fn verify_2fa(
    app: tauri::AppHandle,
    temp_token: String,
    code: String,
) -> Result<types::TrackerState, String> {
    session::verify_two_factor(&app, temp_token, code).await?;
    Ok(app.state::<Hub>().get(&app))
}

#[tauri::command]
async fn logout(app: tauri::AppHandle) -> Result<types::TrackerState, String> {
    session::logout(&app).await?;
    Ok(app.state::<Hub>().get(&app))
}

#[tauri::command]
async fn check_in(app: tauri::AppHandle) -> Result<types::TrackerState, String> {
    session::user_check_in(&app).await?;
    Ok(app.state::<Hub>().get(&app))
}

#[tauri::command]
async fn check_out(app: tauri::AppHandle) -> Result<types::TrackerState, String> {
    session::user_check_out(&app).await?;
    Ok(app.state::<Hub>().get(&app))
}

#[tauri::command]
async fn start_break(app: tauri::AppHandle) -> Result<types::TrackerState, String> {
    session::user_start_break(&app).await?;
    Ok(app.state::<Hub>().get(&app))
}

#[tauri::command]
async fn end_break(app: tauri::AppHandle) -> Result<types::TrackerState, String> {
    session::user_end_break(&app).await?;
    Ok(app.state::<Hub>().get(&app))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            get_state, login, verify_2fa, logout, check_in, check_out, start_break, end_break
        ])
        .setup(|app| {
            let _ = dotenvy::from_filename("../.env");
            let _ = dotenvy::dotenv();
            let handle = app.handle().clone();
            let data_dir = handle.path().app_data_dir().unwrap_or_else(|_| {
                std::env::temp_dir().join("leapsofts-erp")
            });
            std::fs::create_dir_all(&data_dir).ok();
            let conn = open_db(&data_dir).expect("sqlite");
            let api = Api::new(conn);
            api.load_token_bootstrap();
            app.manage(api);
            app.manage(Hub::new());

            WebviewWindowBuilder::new(app, "main", WebviewUrl::External(ERP_URL.parse().unwrap()))
                .title("Leapsofts ERP")
                .inner_size(1280.0, 800.0)
                .min_inner_size(800.0, 600.0)
                .build()?;

            WebviewWindowBuilder::new(app, "tracker", WebviewUrl::App("index.html".into()))
                .title("Leapsofts Time Tracker")
                .inner_size(400.0, 580.0)
                .min_inner_size(360.0, 480.0)
                .build()?;

            tray::setup(&handle)?;

            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                session::bootstrap_session(&handle).await;
            });

            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_millis(2000));
                loop {
                    interval.tick().await;
                    session::on_idle_tick_with_sleep_gap(&handle).await;
                }
            });

            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
