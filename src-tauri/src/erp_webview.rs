use tauri::{AppHandle, Manager, Runtime, WebviewUrl, WebviewWindowBuilder};
use tauri::webview::NewWindowResponse;
use tauri_plugin_opener::OpenerExt;
use url::Url;

/// ERP uses many `target="_blank"` links and `window.open()` calls. On macOS the webview often
/// ignores those popup requests, while right-click → Open Link works because it navigates in-place.
/// This script turns blank/external opens into a top-level navigation so `on_navigation` can open
/// the system browser (or allow in-app navigation for same-origin URLs).
const LINK_CLICK_BRIDGE_SCRIPT: &str = r##"
(function () {
  if (window.__leapsoftsLinkBridge) return;
  window.__leapsoftsLinkBridge = true;

  const ERP_HOST = "__ERP_HOST__";

  function resolveUrl(raw) {
    try {
      return new URL(raw, window.location.href);
    } catch {
      return null;
    }
  }

  function shouldBridge(url, target) {
    if (url.protocol === "mailto:" || url.protocol === "tel:" || url.protocol === "sms:") {
      return true;
    }
    if (target === "_blank") return true;
    return url.host !== ERP_HOST && url.host !== window.location.hostname;
  }

  function openViaNavigation(url) {
    window.location.href = url.href;
  }

  document.addEventListener(
    "click",
    function (event) {
      if (event.defaultPrevented || event.button !== 0) return;
      if (event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;

      const anchor = event.target && event.target.closest && event.target.closest("a[href]");
      if (!anchor || anchor.hasAttribute("download")) return;

      const href = anchor.getAttribute("href");
      if (!href || href.startsWith("#") || href.startsWith("javascript:")) return;

      const url = resolveUrl(href);
      if (!url) return;

      const target = (anchor.getAttribute("target") || "").toLowerCase();
      if (!shouldBridge(url, target)) return;

      event.preventDefault();
      event.stopImmediatePropagation();
      openViaNavigation(url);
    },
    true
  );

  const originalOpen = window.open.bind(window);
  window.open = function (url, target, features) {
    if (typeof url === "string" && url && url !== "about:blank") {
      const resolved = resolveUrl(url);
      if (resolved) {
        openViaNavigation(resolved);
        return null;
      }
    }
    return originalOpen(url, target, features);
  };
})();
"##;

fn link_click_bridge_script() -> String {
    let erp_host = erp_base_url()
        .host_str()
        .unwrap_or("erp.leapsofts.com")
        .to_string();
    LINK_CLICK_BRIDGE_SCRIPT.replace("__ERP_HOST__", &erp_host)
}

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
        // Tauri/Wry capture OS file drops unless disabled; the ERP chat UI relies on HTML5 drop.
        .disable_drag_drop_handler()
        .initialization_script(link_click_bridge_script())
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
