use dark_light::{Mode as SystemTheme, detect as detect_system_theme};
use std::path::PathBuf;
use tauri::utils::config::Color;
use tauri::webview::PageLoadEvent;
use tauri::{Theme, WebviewWindow};

use crate::{config::Config, core::handle, utils::resolve::window_script::build_window_initial_script};
use clash_verge_logging::{Type, logging, logging_error};

const DARK_BACKGROUND_COLOR: Color = Color(46, 48, 61, 255); // #2E303D
const LIGHT_BACKGROUND_COLOR: Color = Color(245, 245, 245, 255); // #F5F5F5
const DARK_BACKGROUND_HEX: &str = "#2E303D";
const LIGHT_BACKGROUND_HEX: &str = "#F5F5F5";

const DEFAULT_WIDTH: f64 = 940.0;
const DEFAULT_HEIGHT: f64 = 700.0;

const MINIMAL_WIDTH: f64 = 520.0;
const MINIMAL_HEIGHT: f64 = 520.0;

#[cfg(target_os = "linux")]
const DEFAULT_DECORATIONS: bool = false;
#[cfg(not(target_os = "linux"))]
const DEFAULT_DECORATIONS: bool = true;

const fn restored_window_size_is_too_small(width: u32, height: u32) -> bool {
    width < MINIMAL_WIDTH as u32 || height < MINIMAL_HEIGHT as u32
}

fn restore_default_size_if_needed(window: &WebviewWindow) {
    let Ok(size) = window.outer_size() else {
        return;
    };

    if !restored_window_size_is_too_small(size.width, size.height) {
        return;
    }

    logging_error!(
        Type::Window,
        window.set_size(tauri::LogicalSize::new(DEFAULT_WIDTH, DEFAULT_HEIGHT))
    );
    logging_error!(Type::Window, window.center());
}

pub async fn build_new_window() -> Result<WebviewWindow, String> {
    let app_handle = handle::Handle::app_handle();

    let config = Config::verge().await;
    let latest = config.latest_arc();
    let start_page = latest.start_page.as_deref().unwrap_or("/");
    let initial_theme_mode = match latest.theme_mode.as_deref() {
        Some("dark") => "dark",
        Some("light") => "light",
        _ => "system",
    };

    let resolved_theme = match initial_theme_mode {
        "dark" => Some(Theme::Dark),
        "light" => Some(Theme::Light),
        _ => None,
    };

    let prefers_dark_background = match resolved_theme {
        Some(Theme::Dark) => true,
        Some(Theme::Light) => false,
        _ => !matches!(detect_system_theme().ok(), Some(SystemTheme::Light)),
    };

    let background_color = if prefers_dark_background {
        DARK_BACKGROUND_COLOR
    } else {
        LIGHT_BACKGROUND_COLOR
    };

    let initial_script = build_window_initial_script(initial_theme_mode, DARK_BACKGROUND_HEX, LIGHT_BACKGROUND_HEX);

    #[cfg(debug_assertions)]
    eprintln!(
        "[window-debug] build_new_window start_page={} frontend=tauri-url",
        start_page
    );

    #[cfg(feature = "verge-dev")]
    let webview_url = if let Ok(dev_url) = std::env::var("CLASH_VERGE_DEV_FRONTEND_URL") {
        match tauri::Url::parse(&dev_url) {
            Ok(url) => {
                let is_http_or_https = url.scheme() == "http" || url.scheme() == "https";
                let is_local_host = matches!(url.host_str(), Some("localhost") | Some("127.0.0.1"));
                let is_expected_port = url.port() == Some(3000);
                let has_no_credentials = url.username().is_empty() && url.password().is_none();

                if is_http_or_https && is_local_host && is_expected_port && has_no_credentials {
                    tauri::WebviewUrl::External(url)
                } else {
                    eprintln!(
                        "[window-dev] Disallowed CLASH_VERGE_DEV_FRONTEND_URL: '{}' (must be http(s)://localhost:3000 or http(s)://127.0.0.1:3000 without credentials), falling back to app bundle",
                        dev_url
                    );
                    tauri::WebviewUrl::App(start_page.into())
                }
            }
            Err(parse_err) => {
                eprintln!(
                    "[window-dev] Failed to parse CLASH_VERGE_DEV_FRONTEND_URL '{}': {parse_err}, falling back to app bundle",
                    dev_url
                );
                tauri::WebviewUrl::App(start_page.into())
            }
        }
    } else {
        tauri::WebviewUrl::App(start_page.into())
    };

    #[cfg(not(feature = "verge-dev"))]
    let webview_url = tauri::WebviewUrl::App(start_page.into());

    let mut builder = tauri::WebviewWindowBuilder::new(
        app_handle,
        "main", /* the unique window label */
        webview_url,
    )
    .title("Clash Verge")
    .center()
    .decorations(DEFAULT_DECORATIONS)
    .fullscreen(false)
    .inner_size(DEFAULT_WIDTH, DEFAULT_HEIGHT)
    .min_inner_size(MINIMAL_WIDTH, MINIMAL_HEIGHT)
    .visible(false) // 等待主题色准备好后再展示，避免启动色差
    .initialization_script(&initial_script)
    .general_autofill_enabled(false) // 禁用自动填充
    .on_page_load(move |window, payload| {
        #[cfg(debug_assertions)]
        eprintln!(
            "[window-debug] page_load event={:?} label={} url={}",
            payload.event(),
            window.label(),
            payload.url()
        );
        logging!(
            info,
            Type::Window,
            "page load event={:?} label={} url={}",
            payload.event(),
            window.label(),
            payload.url()
        );
        if payload.event() != PageLoadEvent::Finished {
            return;
        }

        logging_error!(Type::Window, window.show());
        logging_error!(Type::Window, window.set_focus());
    });

    if let Some(theme) = resolved_theme {
        builder = builder.theme(Some(theme));
    }

    builder = builder.background_color(background_color);

    #[cfg(debug_assertions)]
    if let Some(data_directory) = std::env::var_os("CLASH_VERGE_DEV_WEBVIEW_DATA_DIR") {
        let data_directory = PathBuf::from(data_directory);
        logging!(
            info,
            Type::Window,
            "webview data directory override={}",
            data_directory.display()
        );
        builder = builder.data_directory(data_directory);
    }

    match builder.build() {
        Ok(window) => {
            #[cfg(debug_assertions)]
            eprintln!("[window-debug] builder.build succeeded label=main");
            logging!(
                info,
                Type::Window,
                "builder.build succeeded label=main start_page={}",
                start_page
            );
            logging_error!(Type::Window, window.set_background_color(Some(background_color)));
            restore_default_size_if_needed(&window);
            // A new page supersedes any reload marker left by the old window.
            #[cfg(target_os = "macos")]
            take_webview_needs_reload();
            Ok(window)
        }
        Err(e) => {
            #[cfg(debug_assertions)]
            eprintln!("[window-debug] builder.build failed label=main error={}", e);
            logging!(error, Type::Window, "builder.build failed label=main error={}", e);
            Err(e.to_string())
        }
    }
}

/// Defers recovery of a terminated hidden main webview until its next activation.
#[cfg(target_os = "macos")]
static WEBVIEW_NEEDS_RELOAD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[cfg(target_os = "macos")]
pub fn take_webview_needs_reload() -> bool {
    WEBVIEW_NEEDS_RELOAD.swap(false, std::sync::atomic::Ordering::SeqCst)
}

///
/// macOS may kill hidden WebContent under memory pressure. Clean orphaned Mihomo subscriptions,
/// reload visible webviews immediately, and defer a hidden main-window reload until activation.
/// Registering this callback replaces Tauri's default automatic reload.
#[cfg(target_os = "macos")]
pub fn on_web_content_process_terminated(webview: &tauri::Webview) {
    if handle::Handle::global().is_exiting() {
        return;
    }

    logging!(
        warn,
        Type::Window,
        "WebView 渲染进程已被系统终止（label={}），开始恢复",
        webview.label()
    );

    let window = webview.window();
    let is_user_visible = window.is_visible().unwrap_or(false) && !window.is_minimized().unwrap_or(false);

    // Only the main window has a path that consumes the deferred marker.
    let is_main_window = webview.label() == "main";
    let reload_now = is_user_visible || !is_main_window;

    if !reload_now {
        WEBVIEW_NEEDS_RELOAD.store(true, std::sync::atomic::Ordering::SeqCst);
        logging!(info, Type::Window, "窗口不可见，页面将在下次打开窗口时重载");
    }

    // Clean before reload so cleanup cannot remove subscriptions created by the new page.
    let webview = webview.clone();
    crate::process::AsyncHandler::spawn(move || async move {
        if let Err(err) = handle::Handle::mihomo().clear_all_ws_connections() {
            logging!(warn, Type::Window, "清理 Mihomo WebSocket 连接失败: {err}");
        } else {
            logging!(info, Type::Window, "已清理全部 Mihomo WebSocket 连接");
        }
        if reload_now {
            logging_error!(Type::Window, webview.reload());
        }
    });
}

/// Consumes the shared marker for native unminimize paths that bypass `activate_window`.
#[cfg(target_os = "macos")]
pub fn reload_main_window_if_needed() {
    if !take_webview_needs_reload() {
        return;
    }
    let Some(window) = crate::utils::window_manager::WindowManager::get_main_window() else {
        return;
    };
    logging!(info, Type::Window, "渲染进程曾被系统终止，窗口聚焦后重载页面");
    if let Err(e) = window.reload() {
        logging!(warn, Type::Window, "重载页面失败: {e}");
    }
}
