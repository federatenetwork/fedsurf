//! The `fedsurf-ipc://` scheme: how tab pages (remote origins, no tauri IPC
//! bridge) talk back to the browser. The injected script (see `ua.rs`) POSTs
//! JSON here; `UriSchemeContext::webview_label()` tells us which tab sent it.
//!
//! Only two message kinds exist, both low-privilege by design (a hostile page
//! can at worst rearrange tabs — never touch the filesystem or other origins):
//!   meta     { url, favicon }  -> update that tab's sidebar entry
//!   shortcut { action }        -> keyboard shortcut forwarded from the page

use serde::Deserialize;
use tauri::AppHandle;

#[derive(Deserialize)]
struct IpcMsg {
    kind: String,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    favicon: Option<String>,
    #[serde(default)]
    action: Option<String>,
}

pub fn handle(app: &AppHandle, label: &str, body: &[u8]) {
    let Some(id) = label
        .strip_prefix("tab-")
        .and_then(|s| s.parse::<u32>().ok())
    else {
        return;
    };
    let Ok(msg) = serde_json::from_slice::<IpcMsg>(body) else {
        return;
    };
    match msg.kind.as_str() {
        "meta" => crate::tabs::meta_update(
            app,
            id,
            msg.url,
            None,
            Some(msg.favicon.unwrap_or_default()),
            None,
        ),
        "shortcut" => {
            if let Some(action) = msg.action {
                let handle = app.clone();
                let _ = app.run_on_main_thread(move || {
                    crate::tabs::dispatch_action(&handle, &action);
                });
            }
        }
        _ => {}
    }
}

/// Every response carries permissive CORS headers so `fetch()` from any page
/// origin succeeds (the request body is one-way telemetry; nothing sensitive
/// ever flows back).
pub fn cors_response() -> tauri::http::Response<Vec<u8>> {
    tauri::http::Response::builder()
        .status(200)
        .header("Access-Control-Allow-Origin", "*")
        .header("Access-Control-Allow-Methods", "POST, OPTIONS")
        .header("Access-Control-Allow-Headers", "*")
        .header("Content-Type", "text/plain")
        .body(b"ok".to_vec())
        .unwrap()
}
