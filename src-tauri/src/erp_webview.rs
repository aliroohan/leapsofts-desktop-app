use tauri::{AppHandle, Manager, Runtime, WebviewUrl, WebviewWindowBuilder};
use tauri::webview::NewWindowResponse;
use tauri_plugin_opener::OpenerExt;
use url::Url;

pub fn erp_base_url() -> Url {
    super::ERP_URL.parse().expect("ERP_URL must be a valid URL")
}

fn is_erp_http_url(url: &Url) -> bool {
    if !matches!(url.scheme(), "http" | "https") {
        return false;
    }
    let base = erp_base_url();
    url.scheme() == base.scheme() && url.host_str() == base.host_str()
}

fn open_in_system_browser<R: Runtime>(app: &AppHandle<R>, url: &Url) {
    if let Err(err) = app.opener().open_url(url.as_str(), None::<&str>) {
        eprintln!("failed to open URL in browser: {url} ({err})");
    }
}

fn allow_in_webview_navigation<R: Runtime>(app: &AppHandle<R>, url: &Url) -> bool {
    match url.scheme() {
        "about" | "blob" | "data" => true,
        "mailto" | "tel" | "sms" => {
            open_in_system_browser(app, url);
            false
        }
        "http" | "https" => {
            if is_erp_http_url(url) {
                true
            } else {
                open_in_system_browser(app, url);
                false
            }
        }
        _ => false,
    }
}

pub fn configure_erp_webview<'a, R: Runtime, M: Manager<R>>(
    app: &AppHandle<R>,
    builder: WebviewWindowBuilder<'a, R, M>,
) -> WebviewWindowBuilder<'a, R, M> {
    let app_for_nav = app.clone();
    let app_for_window = app.clone();

    builder
        .on_navigation(move |url| allow_in_webview_navigation(&app_for_nav, url))
        .on_new_window(move |url, features| {
            if is_erp_http_url(&url) {
                let label = format!("erp-{}", uuid::Uuid::new_v4());
                match configure_erp_webview(
                    &app_for_window,
                    WebviewWindowBuilder::new(
                        &app_for_window,
                        &label,
                        WebviewUrl::External(url.clone()),
                    )
                    .title("Leapsofts ERP")
                    .inner_size(1280.0, 800.0)
                    .min_inner_size(800.0, 600.0)
                    .window_features(features),
                )
                .build()
                {
                    Ok(window) => NewWindowResponse::Create { window },
                    Err(err) => {
                        eprintln!("failed to open ERP window: {url} ({err})");
                        open_in_system_browser(&app_for_window, &url);
                        NewWindowResponse::Deny
                    }
                }
            } else {
                open_in_system_browser(&app_for_window, &url);
                NewWindowResponse::Deny
            }
        })
}
