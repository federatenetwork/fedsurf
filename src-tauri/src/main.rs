//! Fedsurf: a browser for the Federate Network.
//!
//! Three schemes, one window:
//!   fed://   -> resolved in-process by `federate-resolution` (root zone ->
//!               TLD -> domain record -> manifest -> blocks, every layer
//!               signature/hash verified before a single byte is rendered)
//!   https:// -> native webview networking
//!   http://  -> native webview networking
//!
//! Two webviews in one window: "chrome" (toolbar UI, app assets) on top and
//! "content" (the page being browsed) below it.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use federate_client::NodeClient;
use federate_resolution::{Resolved, Resolver};
use federate_uri::FederateUri;
use std::sync::Arc;
use tauri::{
    webview::PageLoadEvent, window::WindowBuilder, Emitter, LogicalPosition, LogicalSize, Manager,
    WebviewUrl,
};

const TOPBAR_HEIGHT: f64 = 52.0;

/// Injected into every page the content webview loads (any scheme), so the
/// whole browser scrolls with the same Federate-styled scrollbar.
const SCROLLBAR_SCRIPT: &str = r#"
(function () {
  var css = [
    '::-webkit-scrollbar { width: 12px; height: 12px; }',
    '::-webkit-scrollbar-track { background: transparent; }',
    '::-webkit-scrollbar-thumb {',
    '  background: rgba(174, 122, 72, 0.55);', /* bronze */
    '  border-radius: 999px;',
    '  border: 3px solid transparent;',
    '  background-clip: padding-box;',
    '  min-height: 40px;',
    '}',
    '::-webkit-scrollbar-thumb:hover { background: rgba(174, 122, 72, 0.85); background-clip: padding-box; }',
    '::-webkit-scrollbar-thumb:active { background: #544329; background-clip: padding-box; }', /* umber */
    '::-webkit-scrollbar-corner { background: transparent; }'
  ].join('\n');
  var style = document.createElement('style');
  style.id = '__fedsurf-scrollbar';
  style.textContent = css;
  (document.head || document.documentElement).appendChild(style);
})();
"#;
const START_WIDTH: f64 = 1200.0;
const START_HEIGHT: f64 = 800.0;
const HOME_URL: &str = "fed://home.fed";
const SEARCH_URL: &str = "fed://fed.busca";

struct AppState {
    resolver: Arc<Resolver>,
}

fn main() {
    tracing_subscriber::fmt().init();

    tauri::Builder::default()
        .register_asynchronous_uri_scheme_protocol("fed", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            let url = request.uri().to_string();
            tauri::async_runtime::spawn(async move {
                let response = match app.try_state::<AppState>() {
                    Some(state) => serve_fed(state.resolver.clone(), &url).await,
                    None => error_page(500, "Fedsurf is still starting", "Try again in a moment."),
                };
                responder.respond(response);
            });
        })
        .invoke_handler(tauri::generate_handler![
            navigate, go_back, go_forward, reload, go_home
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
            });

            let mut builder = WindowBuilder::new(app, "main")
                .title("Fedsurf")
                .inner_size(START_WIDTH, START_HEIGHT)
                .min_inner_size(480.0, 320.0);
            #[cfg(target_os = "macos")]
            {
                // Traffic lights float over the toolbar; the toolbar owns the
                // full top edge (the CSS pads left to clear the buttons).
                builder = builder
                    .title_bar_style(tauri::TitleBarStyle::Overlay)
                    .hidden_title(true);
            }
            let window = builder.build()?;

            let chrome = tauri::webview::WebviewBuilder::new(
                "chrome",
                WebviewUrl::App("index.html".into()),
            );
            window.add_child(
                chrome,
                LogicalPosition::new(0.0, 0.0),
                LogicalSize::new(START_WIDTH, TOPBAR_HEIGHT),
            )?;

            let nav_handle = app.handle().clone();
            let load_handle = app.handle().clone();
            let content = tauri::webview::WebviewBuilder::new(
                "content",
                WebviewUrl::External(HOME_URL.parse()?),
            )
            .initialization_script(SCROLLBAR_SCRIPT)
            .on_navigation(move |url| {
                nav_handle
                    .emit_to("chrome", "fedsurf://navigation-started", url.to_string())
                    .ok();
                true
            })
            .on_page_load(move |_webview, payload| {
                if matches!(payload.event(), PageLoadEvent::Finished) {
                    load_handle
                        .emit_to("chrome", "fedsurf://url-changed", payload.url().to_string())
                        .ok();
                }
            });
            window.add_child(
                content,
                LogicalPosition::new(0.0, TOPBAR_HEIGHT),
                LogicalSize::new(START_WIDTH, START_HEIGHT - TOPBAR_HEIGHT),
            )?;

            // Keep the two webviews glued to the window: children are
            // positioned in outer-window coordinates on macOS, so everything
            // shifts down by the title-bar height or the toolbar gets cut.
            layout_webviews(&window);
            let win = window.clone();
            window.on_window_event(move |event| {
                if matches!(
                    event,
                    tauri::WindowEvent::Resized(_) | tauri::WindowEvent::ScaleFactorChanged { .. }
                ) {
                    layout_webviews(&win);
                }
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Fedsurf");
}

/// Position the chrome + content webviews inside the window. The title bar is
/// an overlay (macOS), so the toolbar starts at the very top edge.
fn layout_webviews(window: &tauri::Window) {
    let scale = window.scale_factor().unwrap_or(1.0);
    let Ok(inner) = window.inner_size() else {
        return;
    };
    let inner: LogicalSize<f64> = inner.to_logical(scale);
    if let Some(chrome) = window.get_webview("chrome") {
        chrome.set_position(LogicalPosition::new(0.0, 0.0)).ok();
        chrome
            .set_size(LogicalSize::new(inner.width, TOPBAR_HEIGHT))
            .ok();
    }
    if let Some(content) = window.get_webview("content") {
        content
            .set_position(LogicalPosition::new(0.0, TOPBAR_HEIGHT))
            .ok();
        content
            .set_size(LogicalSize::new(
                inner.width,
                (inner.height - TOPBAR_HEIGHT).max(0.0),
            ))
            .ok();
    }
}

// ---------------------------------------------------------------------------
// Toolbar commands
// ---------------------------------------------------------------------------

/// Turn address-bar input into a URL and load it. Returns the URL actually
/// navigated to so the toolbar can echo it back.
#[tauri::command]
async fn navigate(app: tauri::AppHandle, input: String) -> Result<String, String> {
    let url = normalize_input(&app, input.trim()).await?;
    let content = app
        .get_webview("content")
        .ok_or_else(|| "content webview not found".to_string())?;
    content
        .navigate(url.parse().map_err(|e| format!("invalid URL: {e}"))?)
        .map_err(|e| e.to_string())?;
    Ok(url)
}

#[tauri::command]
fn go_back(app: tauri::AppHandle) {
    if let Some(content) = app.get_webview("content") {
        content.eval("history.back()").ok();
    }
}

#[tauri::command]
fn go_forward(app: tauri::AppHandle) {
    if let Some(content) = app.get_webview("content") {
        content.eval("history.forward()").ok();
    }
}

#[tauri::command]
fn reload(app: tauri::AppHandle) {
    if let Some(content) = app.get_webview("content") {
        content.eval("location.reload()").ok();
    }
}

#[tauri::command]
async fn go_home(app: tauri::AppHandle) -> Result<String, String> {
    navigate(app, HOME_URL.to_string()).await
}

/// Address-bar smarts:
///   - explicit scheme (fed/http/https) passes through
///   - bare word or spaces -> Federate search
///   - `label.tld` where the TLD exists in the verified Federate root -> fed://
///   - anything else with a dot -> https://
async fn normalize_input(app: &tauri::AppHandle, input: &str) -> Result<String, String> {
    if input.is_empty() {
        return Err("empty address".into());
    }
    for scheme in ["fed://", "http://", "https://"] {
        if input.starts_with(scheme) {
            return Ok(input.to_string());
        }
    }
    let host = input
        .split(['/', '?', '#'])
        .next()
        .unwrap_or(input)
        .to_ascii_lowercase();
    let looks_like_host = host.contains('.') && !host.contains(' ');
    if looks_like_host {
        if let Ok(domain) = federate_naming::FederateDomain::parse(&host) {
            let state = app.state::<AppState>();
            if let Ok(root) = state.resolver.root().await {
                if root.lookup_tld(&domain.tld).is_some() {
                    return Ok(format!("fed://{input}"));
                }
            }
        }
        return Ok(format!("https://{input}"));
    }
    Ok(format!("{SEARCH_URL}/?q={}", percent_encode(input)))
}

fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for byte in s.bytes() {
        match byte {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

// ---------------------------------------------------------------------------
// fed:// protocol
// ---------------------------------------------------------------------------

async fn serve_fed(resolver: Arc<Resolver>, raw_url: &str) -> tauri::http::Response<Vec<u8>> {
    let uri = match FederateUri::parse(raw_url) {
        Ok(uri) => uri,
        Err(e) => {
            return error_page(
                400,
                "Not a valid Federate address",
                &format!("{raw_url}<br><br>{e}"),
            )
        }
    };
    match resolver.resolve_uri(&uri).await {
        Ok(Resolved::Content {
            bytes, mime, hash, ..
        }) => tauri::http::Response::builder()
            .status(200)
            .header("Content-Type", mime)
            .header("ETag", format!("\"{hash}\""))
            .header("X-Federate-Verified", "signature-chain+content-hash")
            .body(bytes)
            .unwrap(),
        Ok(Resolved::NotFederate { host }) => error_page(
            400,
            "Not a Federate name",
            &format!("<code>{host}</code> is not a valid Federate domain."),
        ),
        Ok(Resolved::TldNotFound { tld }) => error_page(
            404,
            "Unknown TLD",
            &format!("<code>.{tld}</code> is not in the Federate root registry."),
        ),
        Ok(Resolved::TldUnavailable { tld, status }) => error_page(
            451,
            "TLD unavailable",
            &format!("<code>.{tld}</code> exists but is not resolvable (status: {status})."),
        ),
        Ok(Resolved::DelegatedUnavailable { domain, tld, reason }) => error_page(
            502,
            "Registry unreachable",
            &format!(
                "<code>{domain}</code> lives under the delegated TLD <code>.{tld}</code>, whose registry cannot be reached right now.<br><br>{reason}"
            ),
        ),
        Ok(Resolved::DomainNotFound { domain }) => error_page(
            404,
            "Domain not registered",
            &format!("<code>{domain}</code> is not registered on the Federate Network."),
        ),
        Ok(Resolved::DomainUnavailable { domain, status }) => error_page(
            451,
            "Domain unavailable",
            &format!("<code>{domain}</code> exists but is not active (status: {status})."),
        ),
        Ok(Resolved::PathNotFound { domain, path }) => error_page(
            404,
            "Page not found",
            &format!("<code>{domain}</code> publishes no file at <code>{path}</code>."),
        ),
        Ok(Resolved::SecurityFailure { domain, layer, reason }) => security_page(&domain, &layer, &reason),
        Err(e) => error_page(
            502,
            "Could not reach the Federate Network",
            &format!("{e}<br><br>Check your connection, or set <code>FEDSURF_BOOTSTRAP</code> to a reachable node."),
        ),
    }
}

fn error_page(status: u16, title: &str, detail: &str) -> tauri::http::Response<Vec<u8>> {
    let body = page_html(title, detail, false);
    tauri::http::Response::builder()
        .status(status)
        .header("Content-Type", "text/html; charset=utf-8")
        .body(body.into_bytes())
        .unwrap()
}

/// Verification failures get a distinct, unmissable page: content is NEVER
/// rendered when any signature or hash in the chain fails.
fn security_page(domain: &str, layer: &str, reason: &str) -> tauri::http::Response<Vec<u8>> {
    let detail = format!(
        "Verification failed for <code>{domain}</code> at the <strong>{layer}</strong> layer.<br><br>{reason}<br><br>Fedsurf refused to render this content. This can mean tampering somewhere between you and the publisher."
    );
    let body = page_html("Content failed verification", &detail, true);
    tauri::http::Response::builder()
        .status(502)
        .header("Content-Type", "text/html; charset=utf-8")
        .body(body.into_bytes())
        .unwrap()
}

/// Browser-UI fonts, embedded so error/security pages render them without any
/// asset origin (they are served straight from the fed:// handler).
fn font_css() -> &'static str {
    static CSS: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    CSS.get_or_init(|| {
        let face = |family: &str, style: &str, weight: &str, bytes: &[u8]| {
            format!(
                "@font-face{{font-family:'{family}';font-style:{style};font-weight:{weight};\
                 src:url(data:font/woff2;base64,{}) format('woff2')}}",
                base64_encode(bytes)
            )
        };
        [
            face(
                "Averia Serif Libre",
                "normal",
                "400",
                include_bytes!("../../ui/fonts/averia-serif-libre-400.woff2"),
            ),
            face(
                "Averia Serif Libre",
                "normal",
                "700",
                include_bytes!("../../ui/fonts/averia-serif-libre-700.woff2"),
            ),
            face(
                "Lilex",
                "normal",
                "100 700",
                include_bytes!("../../ui/fonts/lilex-var.woff2"),
            ),
        ]
        .join("\n")
    })
}

fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = u32::from_be_bytes([0, b[0], b[1], b[2]]);
        out.push(TABLE[(n >> 18) as usize & 63] as char);
        out.push(TABLE[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { TABLE[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { TABLE[n as usize & 63] as char } else { '=' });
    }
    out
}

fn page_html(title: &str, detail: &str, danger: bool) -> String {
    // Federate Network palette: terracotta for security failures, bronze otherwise.
    let accent = if danger { "#C86439" } else { "#AE7A48" };
    let fonts = font_css();
    format!(
        r#"<!doctype html><html><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<style>
{fonts}
  :root {{
    --bg: #FBF6F0; --fg: #21211F; --muted: #544329; --border: #E8DEC5;
    --surface: #E8DEC5; --accent: {accent}; }}
  * {{ margin: 0; box-sizing: border-box; }}
  body {{ background: var(--bg); color: var(--fg); min-height: 100dvh;
    display: grid; place-items: center; padding: 24px;
    font: 15px/1.6 'Averia Serif Libre', Georgia, serif; }}
  main {{ max-width: 560px; width: 100%; }}
  .rule {{ width: 40px; height: 3px; background: var(--accent); margin-bottom: 20px; }}
  h1 {{ font-size: 24px; font-weight: 700; margin-bottom: 12px; }}
  p {{ color: var(--muted); overflow-wrap: break-word; }}
  code {{ font: 440 13px/1.4 'Lilex', ui-monospace, Menlo, monospace;
    background: color-mix(in srgb, var(--surface) 65%, var(--bg));
    border-radius: 4px; padding: 1px 5px; }}
  footer {{ margin-top: 28px; padding-top: 14px; border-top: 1px solid var(--border);
    font: 500 11px/1 'Lilex', ui-monospace, monospace;
    color: var(--muted); letter-spacing: 0.08em; }}
</style></head><body><main>
<div class="rule"></div>
<h1>{title}</h1>
<p>{detail}</p>
<footer>FEDSURF &middot; FEDERATE NETWORK</footer>
</main></body></html>"#
    )
}
