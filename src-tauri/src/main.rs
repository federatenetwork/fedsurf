//! Fedsurf: a browser for the Federate Network.
//!
//! Three schemes, one window:
//!   fed://   -> resolved in-process by `federate-resolution` (root zone ->
//!               TLD -> domain record -> manifest -> blocks, every layer
//!               signature/hash verified before a single byte is rendered)
//!   https:// -> native webview networking
//!   http://  -> native webview networking
//!
//! One window, many webviews: "chrome" (toolbar + tab sidebar, app assets)
//! fills the window; one child webview per tab (`tab-<id>`) covers the content
//! area, only the active one visible. See `tabs.rs`.
//!
//! Fedsurf is also the OS handler for fed:// links (deep-link plugin): a
//! fed:// URL clicked anywhere opens here, in the running instance.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod fed_protocol;
mod ipc;
mod menu;
mod tabs;
mod ua;

use federate_client::NodeClient;
use federate_resolution::Resolver;
use std::sync::{Arc, Mutex};
use tauri::{window::WindowBuilder, LogicalPosition, LogicalSize, Manager, WebviewUrl};
use tauri_plugin_deep_link::DeepLinkExt;

const START_WIDTH: f64 = 1200.0;
const START_HEIGHT: f64 = 800.0;

pub struct AppState {
    pub resolver: Arc<Resolver>,
    pub tabs: Mutex<tabs::TabState>,
}

fn main() {
    tracing_subscriber::fmt().init();

    tauri::Builder::default()
        // Must be first: routes a second launch (e.g. the OS invoking us for a
        // fed:// link on Windows/Linux) into this instance. With the
        // "deep-link" feature the URL args are forwarded to the deep-link
        // plugin automatically; we just surface the window.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(window) = app.get_window("main") {
                window.unminimize().ok();
                window.set_focus().ok();
            }
        }))
        .plugin(tauri_plugin_deep_link::init())
        .register_asynchronous_uri_scheme_protocol("fed", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            let url = request.uri().to_string();
            tauri::async_runtime::spawn(async move {
                let response = match app.try_state::<AppState>() {
                    Some(state) => fed_protocol::serve_fed(state.resolver.clone(), &url).await,
                    None => fed_protocol::error_page(
                        500,
                        "Fedsurf is still starting",
                        "Try again in a moment.",
                    ),
                };
                responder.respond(response);
            });
        })
        .register_asynchronous_uri_scheme_protocol("fedsurf-ipc", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            let label = ctx.webview_label().to_string();
            let body = request.body().to_vec();
            let is_post = request.method() == tauri::http::Method::POST;
            responder.respond(ipc::cors_response());
            if is_post {
                tauri::async_runtime::spawn(async move {
                    ipc::handle(&app, &label, &body);
                });
            }
        })
        .invoke_handler(tauri::generate_handler![
            tabs::frontend_ready,
            tabs::create_tab,
            tabs::close_tab,
            tabs::activate_tab,
            tabs::move_tab,
            tabs::set_sidebar_width,
            tabs::navigate,
            tabs::go_back,
            tabs::go_forward,
            tabs::reload,
            tabs::go_home,
            tabs::frontend_log
        ])
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let bootstrap = std::env::var("FEDSURF_BOOTSTRAP")
                .unwrap_or_else(|_| federate_core::DEFAULT_BOOTSTRAP_URL.to_string());
            let root_key = std::env::var("FEDSURF_ROOT_KEY").ok();
            let resolver = Resolver::new(NodeClient::new(&bootstrap), &data_dir, root_key)?;
            app.manage(AppState {
                resolver: Arc::new(resolver),
                tabs: Mutex::new(tabs::TabState::default()),
            });

            menu::install(app)?;

            let mut builder = WindowBuilder::new(app, "main")
                .title("Fedsurf")
                .inner_size(START_WIDTH, START_HEIGHT)
                .min_inner_size(480.0, 320.0)
                .background_color(tabs::SURFACE_COLOR);
            #[cfg(target_os = "macos")]
            {
                // Traffic lights float over the toolbar; the toolbar owns the
                // full top edge (the CSS pads left to clear the buttons).
                builder = builder
                    .title_bar_style(tauri::TitleBarStyle::Overlay)
                    .hidden_title(true);
            }
            let window = builder.build()?;

            // Chrome fills the window (topbar + sidebar + background); tab
            // webviews are stacked over its content area, added later.
            let chrome =
                tauri::webview::WebviewBuilder::new("chrome", WebviewUrl::App("index.html".into()))
                    .background_color(tabs::SURFACE_COLOR);
            window.add_child(
                chrome,
                LogicalPosition::new(0.0, 0.0),
                LogicalSize::new(START_WIDTH, START_HEIGHT),
            )?;

            // fed:// links from the OS. In dev on Windows/Linux the scheme is
            // registered at runtime; installed bundles register it via the
            // installer (NSIS registry / .desktop MimeType / Info.plist).
            #[cfg(any(windows, target_os = "linux"))]
            {
                if let Err(e) = app.deep_link().register("fed") {
                    tracing::warn!("could not register fed:// at runtime: {e}");
                }
            }
            let start_urls: Vec<String> = app
                .deep_link()
                .get_current()
                .ok()
                .flatten()
                .unwrap_or_default()
                .iter()
                .map(ToString::to_string)
                .collect();
            let initial_url = start_urls.first().cloned();
            // on_open_url may replay the launch URLs; swallow that one echo so
            // the initial tab isn't duplicated.
            let startup_echo = Arc::new(Mutex::new(if start_urls.is_empty() {
                None
            } else {
                Some(start_urls)
            }));
            let handle = app.handle().clone();
            app.deep_link().on_open_url(move |event| {
                let urls: Vec<String> = event.urls().iter().map(ToString::to_string).collect();
                if urls.is_empty() {
                    return;
                }
                if let Some(initial) = startup_echo.lock().unwrap().take() {
                    if initial == urls {
                        return;
                    }
                }
                // Defer out of the Apple Event callback: creating a webview
                // inside it panics in tao's did_finish_launching path, and a
                // panic there cannot unwind — the whole app aborts. Hop
                // through the async runtime so the work lands on the main
                // thread on a clean event-loop tick.
                let app = handle.clone();
                tauri::async_runtime::spawn(async move {
                    let inner = app.clone();
                    let _ = app.run_on_main_thread(move || {
                        if let Some(window) = inner.get_window("main") {
                            window.unminimize().ok();
                            window.set_focus().ok();
                        }
                        for url in urls {
                            if let Err(e) = tabs::create_tab_sync(&inner, Some(url), true) {
                                tracing::warn!("deep link open failed: {e}");
                            }
                        }
                    });
                });
            });

            tabs::create_tab_sync(&app.handle().clone(), initial_url, true)
                .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
            tabs::layout_all(&window);

            let win = window.clone();
            window.on_window_event(move |event| {
                if matches!(
                    event,
                    tauri::WindowEvent::Resized(_) | tauri::WindowEvent::ScaleFactorChanged { .. }
                ) {
                    tabs::layout_all(&win);
                }
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Fedsurf");
}
