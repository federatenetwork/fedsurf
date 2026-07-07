//! Tab management. Rust is the source of truth: one child webview per tab
//! (label `tab-<id>`), only the active one visible. The chrome webview is a
//! render cache fed by `fedsurf://tab-*` events and drives everything through
//! the commands at the bottom of this file.

use crate::fed_protocol::canonical_fed_url;
use crate::AppState;
use serde::Serialize;
use std::collections::HashMap;
use tauri::webview::{NewWindowResponse, PageLoadEvent, WebviewBuilder};
use tauri::{AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, WebviewUrl};

pub const TOPBAR_HEIGHT: f64 = 52.0;
pub const HOME_URL: &str = "fed://home.fed";
pub const SEARCH_URL: &str = "fed://fed.busca";
const DEFAULT_SIDEBAR_PX: f64 = 240.0;

pub type TabId = u32;

#[derive(Clone, Default, Serialize)]
pub struct TabMeta {
    pub url: String,
    pub title: String,
    pub favicon: String,
    pub loading: bool,
}

#[derive(Clone, Serialize)]
pub struct TabInfo {
    pub id: TabId,
    #[serde(flatten)]
    pub meta: TabMeta,
}

#[derive(Clone, Serialize)]
pub struct TabsSnapshot {
    pub tabs: Vec<TabInfo>,
    pub active: Option<TabId>,
    pub platform: &'static str,
}

pub struct TabState {
    pub order: Vec<TabId>,
    pub meta: HashMap<TabId, TabMeta>,
    pub active: Option<TabId>,
    pub next_id: TabId,
    pub sidebar_px: f64,
}

impl Default for TabState {
    fn default() -> Self {
        Self {
            order: Vec::new(),
            meta: HashMap::new(),
            active: None,
            next_id: 1,
            sidebar_px: DEFAULT_SIDEBAR_PX,
        }
    }
}

fn label_of(id: TabId) -> String {
    format!("tab-{id}")
}

fn emit_chrome<S: Serialize + Clone>(app: &AppHandle, event: &str, payload: S) {
    app.emit_to("chrome", event, payload).ok();
}

/// Run a closure on the main thread (webview creation is main-thread-only on
/// macOS) and await its result from an async command.
pub async fn on_main<T, F>(app: &AppHandle, f: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(&AppHandle) -> Result<T, String> + Send + 'static,
{
    let (tx, rx) = tokio::sync::oneshot::channel();
    let handle = app.clone();
    app.run_on_main_thread(move || {
        let _ = tx.send(f(&handle));
    })
    .map_err(|e| e.to_string())?;
    rx.await.map_err(|_| "main thread task dropped".to_string())?
}

// ---------------------------------------------------------------------------
// Core operations (call on the main thread)
// ---------------------------------------------------------------------------

pub fn create_tab_sync(
    app: &AppHandle,
    url: Option<String>,
    activate: bool,
) -> Result<TabId, String> {
    let url_str = canonical_fed_url(&url.unwrap_or_else(|| HOME_URL.to_string()));
    let parsed: tauri::Url = url_str.parse().map_err(|e| format!("invalid URL: {e}"))?;
    let window = app
        .get_window("main")
        .ok_or_else(|| "main window not found".to_string())?;

    let state = app.state::<AppState>();
    let (id, index, sidebar_px) = {
        let mut tabs = state.tabs.lock().unwrap();
        let id = tabs.next_id;
        tabs.next_id += 1;
        tabs.order.push(id);
        tabs.meta.insert(
            id,
            TabMeta {
                url: url_str.clone(),
                loading: true,
                ..Default::default()
            },
        );
        (id, tabs.order.len() - 1, tabs.sidebar_px)
    };

    let label = label_of(id);
    let nav_app = app.clone();
    let load_app = app.clone();
    let title_app = app.clone();
    let popup_app = app.clone();

    // The webview starts at about:blank and only navigates after the custom
    // user agent is applied — WKWebView races the first navigation otherwise,
    // and the first site would see the bare default UA (=> legacy pages).
    let blank: tauri::Url = "about:blank".parse().unwrap();
    #[allow(unused_mut)]
    let mut builder = WebviewBuilder::new(label, WebviewUrl::External(blank))
        .initialization_script(crate::ua::init_script())
        .use_https_scheme(true)
        .on_navigation(move |url| {
            if url.as_str() != "about:blank" {
                meta_update(
                    &nav_app,
                    id,
                    Some(url.to_string()),
                    None,
                    None,
                    Some(true),
                );
            }
            true
        })
        .on_page_load(move |_, payload| {
            if payload.url().as_str() == "about:blank" {
                return;
            }
            let done = matches!(payload.event(), PageLoadEvent::Finished);
            meta_update(
                &load_app,
                id,
                Some(payload.url().to_string()),
                None,
                None,
                Some(!done),
            );
        })
        .on_document_title_changed(move |_, title| {
            meta_update(&title_app, id, None, Some(title), None, None);
        })
        .on_new_window(move |url, _features| {
            // target=_blank / window.open become tabs; Fedsurf never spawns
            // OS windows for page content.
            let handle = popup_app.clone();
            let target = url.to_string();
            let _ = popup_app.run_on_main_thread(move || {
                let _ = create_tab_sync(&handle, Some(target), true);
            });
            NewWindowResponse::Deny
        });
    #[cfg(not(windows))]
    {
        builder = builder.user_agent(crate::ua::USER_AGENT);
    }

    let (pos, size) = content_rect(&window, sidebar_px);
    let webview = window.add_child(builder, pos, size).map_err(|e| {
        // Roll the phantom tab back out of the state so the sidebar never
        // shows a tab that has no webview behind it.
        let mut tabs = state.tabs.lock().unwrap();
        tabs.order.retain(|t| *t != id);
        tabs.meta.remove(&id);
        e.to_string()
    })?;
    apply_user_agent(&webview);
    #[cfg(target_os = "macos")]
    macos_container::wrap(&webview);
    webview.navigate(parsed).map_err(|e| e.to_string())?;

    #[derive(Clone, Serialize)]
    struct Created {
        id: TabId,
        url: String,
        index: usize,
        active: bool,
    }
    emit_chrome(
        app,
        "fedsurf://tab-created",
        Created {
            id,
            url: url_str,
            index,
            active: activate,
        },
    );

    if activate {
        activate_tab_sync(app, id)?;
    } else {
        set_tab_hidden(&webview, true);
    }
    Ok(id)
}

/// `WebviewBuilder::user_agent` is ignored for child (multiwebview) webviews
/// on macOS, so Google & friends see WKWebView's bare UA and serve their
/// legacy pages. Set `WKWebView.customUserAgent` directly instead.
#[cfg(target_os = "macos")]
fn apply_user_agent(webview: &tauri::Webview) {
    let ua = crate::ua::USER_AGENT;
    let label = webview.label().to_string();
    let result = webview.with_webview(move |pw| {
        let wk = pw.inner() as *mut objc2::runtime::AnyObject;
        let ns = objc2_foundation::NSString::from_str(ua);
        unsafe {
            let _: () = objc2::msg_send![wk, setCustomUserAgent: &*ns];
        }
        tracing::info!("customUserAgent applied to {label}");
    });
    if let Err(e) = result {
        tracing::warn!("with_webview failed: {e}");
    }
}

#[cfg(not(target_os = "macos"))]
fn apply_user_agent(_webview: &tauri::Webview) {}

pub fn activate_tab_sync(app: &AppHandle, id: TabId) -> Result<(), String> {
    let state = app.state::<AppState>();
    let prev = {
        let mut tabs = state.tabs.lock().unwrap();
        if !tabs.order.contains(&id) {
            return Err(format!("no tab {id}"));
        }
        let prev = tabs.active;
        tabs.active = Some(id);
        prev
    };
    if let Some(prev_id) = prev.filter(|p| *p != id) {
        if let Some(w) = app.get_webview(&label_of(prev_id)) {
            set_tab_hidden(&w, true);
        }
    }
    if let Some(w) = app.get_webview(&label_of(id)) {
        set_tab_hidden(&w, false);
        w.set_focus().ok();
    }
    sync_window_title(app);
    emit_chrome(app, "fedsurf://tab-activated", id);
    Ok(())
}

pub fn close_tab_sync(app: &AppHandle, id: TabId) -> Result<(), String> {
    let state = app.state::<AppState>();
    let next_active = {
        let mut tabs = state.tabs.lock().unwrap();
        let Some(pos) = tabs.order.iter().position(|t| *t == id) else {
            return Ok(()); // already gone; closing twice is not an error
        };
        if tabs.order.len() == 1 {
            // The last tab never closes — it goes home instead, so the window
            // always has a live page.
            drop(tabs);
            if let Some(w) = app.get_webview(&label_of(id)) {
                w.navigate(HOME_URL.parse().unwrap()).ok();
            }
            return Ok(());
        }
        tabs.order.remove(pos);
        tabs.meta.remove(&id);
        if tabs.active == Some(id) {
            tabs.active = None;
            // Prefer the right neighbor (browser convention), else the new last.
            Some(tabs.order[pos.min(tabs.order.len() - 1)])
        } else {
            None
        }
    };
    if let Some(w) = app.get_webview(&label_of(id)) {
        #[cfg(target_os = "macos")]
        macos_container::remove(&w);
        w.close().ok();
    }
    emit_chrome(app, "fedsurf://tab-closed", id);
    if let Some(next) = next_active {
        activate_tab_sync(app, next)?;
    }
    Ok(())
}

pub fn move_tab_sync(app: &AppHandle, id: TabId, to_index: usize) -> Result<(), String> {
    let state = app.state::<AppState>();
    let mut tabs = state.tabs.lock().unwrap();
    let Some(pos) = tabs.order.iter().position(|t| *t == id) else {
        return Err(format!("no tab {id}"));
    };
    tabs.order.remove(pos);
    let to = to_index.min(tabs.order.len());
    tabs.order.insert(to, id);
    Ok(())
}

pub fn set_sidebar_width_sync(app: &AppHandle, px: f64) {
    {
        let state = app.state::<AppState>();
        state.tabs.lock().unwrap().sidebar_px = px.max(0.0);
    }
    if let Some(window) = app.get_window("main") {
        layout_all(&window);
    }
}

/// Merge new metadata into a tab and broadcast the full row to the chrome.
/// Callable from any thread (events + the ipc scheme land off-main).
pub fn meta_update(
    app: &AppHandle,
    id: TabId,
    url: Option<String>,
    title: Option<String>,
    favicon: Option<String>,
    loading: Option<bool>,
) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let (info, is_active) = {
        let mut tabs = state.tabs.lock().unwrap();
        let active = tabs.active;
        let Some(meta) = tabs.meta.get_mut(&id) else {
            return;
        };
        if let Some(url) = url {
            let url = canonical_fed_url(&url);
            if url != meta.url {
                // Real navigation: the old page's title/favicon are stale.
                if !url.split('#').next().eq(&meta.url.split('#').next()) {
                    meta.title.clear();
                }
                meta.url = url;
            }
        }
        if let Some(title) = title {
            meta.title = title;
        }
        if let Some(favicon) = favicon {
            meta.favicon = favicon;
        }
        if let Some(loading) = loading {
            meta.loading = loading;
        }
        (
            TabInfo {
                id,
                meta: meta.clone(),
            },
            active == Some(id),
        )
    };
    if is_active {
        sync_window_title(app);
    }
    emit_chrome(app, "fedsurf://tab-updated", info);
}

fn sync_window_title(app: &AppHandle) {
    let state = app.state::<AppState>();
    let title = {
        let tabs = state.tabs.lock().unwrap();
        tabs.active
            .and_then(|id| tabs.meta.get(&id))
            .map(|m| m.title.clone())
            .unwrap_or_default()
    };
    if let Some(window) = app.get_window("main") {
        let full = if title.is_empty() {
            "Fedsurf".to_string()
        } else {
            format!("{title} — Fedsurf")
        };
        window.set_title(&full).ok();
    }
}

fn snapshot(app: &AppHandle) -> TabsSnapshot {
    let state = app.state::<AppState>();
    let tabs = state.tabs.lock().unwrap();
    TabsSnapshot {
        tabs: tabs
            .order
            .iter()
            .filter_map(|id| {
                tabs.meta.get(id).map(|meta| TabInfo {
                    id: *id,
                    meta: meta.clone(),
                })
            })
            .collect(),
        active: tabs.active,
        platform: std::env::consts::OS,
    }
}

fn active_label(app: &AppHandle) -> Option<String> {
    let state = app.try_state::<AppState>()?;
    let active = state.tabs.lock().unwrap().active?;
    Some(label_of(active))
}

// ---------------------------------------------------------------------------
// Layout
// ---------------------------------------------------------------------------

fn content_rect(
    window: &tauri::Window,
    sidebar_px: f64,
) -> (LogicalPosition<f64>, LogicalSize<f64>) {
    let scale = window.scale_factor().unwrap_or(1.0);
    let inner: LogicalSize<f64> = window
        .inner_size()
        .map(|s| s.to_logical(scale))
        .unwrap_or_else(|_| LogicalSize::new(1200.0, 800.0));
    (
        LogicalPosition::new(sidebar_px, TOPBAR_HEIGHT),
        LogicalSize::new(
            (inner.width - sidebar_px).max(0.0),
            (inner.height - TOPBAR_HEIGHT).max(0.0),
        ),
    )
}

/// Keep every webview glued to the window: chrome fills it (topbar + sidebar
/// + background), tab webviews cover the content area. Children are positioned
/// in outer-window coordinates on macOS, so this must rerun on resize/rescale.
pub fn layout_all(window: &tauri::Window) {
    let scale = window.scale_factor().unwrap_or(1.0);
    let Ok(inner) = window.inner_size() else {
        return;
    };
    let inner: LogicalSize<f64> = inner.to_logical(scale);
    let sidebar_px = window
        .try_state::<AppState>()
        .map(|s| s.tabs.lock().unwrap().sidebar_px)
        .unwrap_or(DEFAULT_SIDEBAR_PX);

    if let Some(chrome) = window.get_webview("chrome") {
        chrome.set_position(LogicalPosition::new(0.0, 0.0)).ok();
        chrome
            .set_size(LogicalSize::new(inner.width, inner.height))
            .ok();
    }
    let (pos, size) = content_rect(window, sidebar_px);
    for webview in window.webviews() {
        if webview.label().starts_with("tab-") {
            // On macOS the webview lives inside a container view (see
            // macos_container) — position that instead; tauri's own
            // set_position would misplace the webview within it.
            #[cfg(target_os = "macos")]
            macos_container::set_frame(&webview, pos.x, pos.y, size.width, size.height);
            #[cfg(not(target_os = "macos"))]
            {
                webview.set_position(pos).ok();
                webview.set_size(size).ok();
            }
        }
    }
}

/// macOS: every tab's WKWebView lives inside its own container NSView sized
/// to the content area. WebKit's attached Web Inspector lays itself out
/// relative to the inspected webview's superview — when that superview is the
/// whole window contentView the inspector hijacks the window (covers the
/// toolbar/sidebar); inside a per-tab container it docks within the content
/// area like it should.
#[cfg(target_os = "macos")]
mod macos_container {
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    use objc2_foundation::{NSPoint, NSRect, NSSize};

    /// Move a freshly created tab webview out of the window contentView and
    /// into a container NSView occupying the same frame.
    pub fn wrap(webview: &tauri::Webview) {
        let _ = webview.with_webview(|pw| unsafe {
            let v = pw.inner() as *mut AnyObject;
            let parent: *mut AnyObject = msg_send![&*v, superview];
            if parent.is_null() {
                return;
            }
            let window: *mut AnyObject = msg_send![&*v, window];
            let content: *mut AnyObject = msg_send![&*window, contentView];
            if parent != content {
                return; // already wrapped
            }
            let frame: NSRect = msg_send![&*v, frame];
            let container: *mut AnyObject = msg_send![class!(NSView), alloc];
            let container: *mut AnyObject = msg_send![container, initWithFrame: frame];
            let _: () = msg_send![&*content, addSubview: &*container];
            let _: () = msg_send![&*v, removeFromSuperview];
            let bounds: NSRect = msg_send![&*container, bounds];
            let _: () = msg_send![&*v, setFrame: bounds];
            // width + height sizable so the webview tracks the container
            let _: () = msg_send![&*v, setAutoresizingMask: 18usize];
            let _: () = msg_send![&*container, addSubview: &*v];
            // superview holds the only reference we need
            let _: () = msg_send![&*container, release];
        });
    }

    /// Position the container in window coordinates (x/y from the top-left,
    /// logical points).
    pub fn set_frame(webview: &tauri::Webview, x: f64, y: f64, w: f64, h: f64) {
        let _ = webview.with_webview(move |pw| unsafe {
            let v = pw.inner() as *mut AnyObject;
            let container: *mut AnyObject = msg_send![&*v, superview];
            if container.is_null() {
                return;
            }
            let window: *mut AnyObject = msg_send![&*v, window];
            let content: *mut AnyObject = msg_send![&*window, contentView];
            if container == content {
                return; // not wrapped; nothing to do
            }
            let parent: *mut AnyObject = msg_send![&*container, superview];
            if parent.is_null() {
                return;
            }
            let flipped: bool = msg_send![&*parent, isFlipped];
            let pb: NSRect = msg_send![&*parent, bounds];
            let oy = if flipped { y } else { pb.size.height - y - h };
            let frame = NSRect::new(NSPoint::new(x, oy), NSSize::new(w, h));
            let current: NSRect = msg_send![&*container, frame];
            if (current.origin.x - frame.origin.x).abs() < 0.5
                && (current.origin.y - frame.origin.y).abs() < 0.5
                && (current.size.width - frame.size.width).abs() < 0.5
                && (current.size.height - frame.size.height).abs() < 0.5
            {
                return; // unchanged — don't force a relayout/repaint
            }
            let _: () = msg_send![&*container, setFrame: frame];
        });
    }

    /// Hide/show the whole container (hiding just the webview would leave an
    /// attached inspector of a background tab on screen).
    pub fn set_hidden(webview: &tauri::Webview, hidden: bool) {
        let _ = webview.with_webview(move |pw| unsafe {
            let v = pw.inner() as *mut AnyObject;
            let container: *mut AnyObject = msg_send![&*v, superview];
            if container.is_null() {
                return;
            }
            let window: *mut AnyObject = msg_send![&*v, window];
            let content: *mut AnyObject = msg_send![&*window, contentView];
            if container == content {
                return;
            }
            let _: () = msg_send![&*container, setHidden: hidden];
        });
    }

    /// Remove the container from the view hierarchy (tab closing). An empty
    /// NSView left behind would still hit-test and eat clicks.
    pub fn remove(webview: &tauri::Webview) {
        let _ = webview.with_webview(|pw| unsafe {
            let v = pw.inner() as *mut AnyObject;
            let container: *mut AnyObject = msg_send![&*v, superview];
            if container.is_null() {
                return;
            }
            let window: *mut AnyObject = msg_send![&*v, window];
            let content: *mut AnyObject = msg_send![&*window, contentView];
            if container == content {
                return;
            }
            let _: () = msg_send![&*container, removeFromSuperview];
        });
    }
}

fn set_tab_hidden(webview: &tauri::Webview, hidden: bool) {
    #[cfg(target_os = "macos")]
    macos_container::set_hidden(webview, hidden);
    if hidden {
        webview.hide().ok();
    } else {
        webview.show().ok();
    }
}

// ---------------------------------------------------------------------------
// Shortcut / menu dispatch (menu events + `fedsurf-ipc` shortcut reports).
// Runs on the main thread; menu item ids double as action names.
// ---------------------------------------------------------------------------

pub fn dispatch_action(app: &AppHandle, action: &str) {
    let result: Result<(), String> = match action {
        "new-tab" => create_tab_sync(app, None, true).map(|_| focus_address(app)),
        "close-tab" => match current_active(app) {
            Some(id) => close_tab_sync(app, id),
            None => Ok(()),
        },
        "next-tab" => cycle_tab(app, 1),
        "prev-tab" => cycle_tab(app, -1),
        "toggle-sidebar" => {
            emit_chrome(app, "fedsurf://toggle-sidebar", ());
            Ok(())
        }
        "focus-address" => {
            focus_address(app);
            Ok(())
        }
        "back" => eval_active(app, "history.back()"),
        "forward" => eval_active(app, "history.forward()"),
        "reload" => {
            if let Some(w) = active_label(app).and_then(|l| app.get_webview(&l)) {
                w.reload().ok();
            }
            Ok(())
        }
        "devtools" => {
            if let Some(w) = active_label(app).and_then(|l| app.get_webview(&l)) {
                if w.is_devtools_open() {
                    w.close_devtools();
                } else {
                    w.open_devtools();
                }
            }
            Ok(())
        }
        other => {
            if let Some(n) = other.strip_prefix("tab-").and_then(|s| s.parse::<usize>().ok()) {
                jump_to_index(app, n)
            } else {
                Ok(()) // predefined menu items (copy/paste/quit/...) land here
            }
        }
    };
    if let Err(e) = result {
        tracing::warn!("action {action} failed: {e}");
    }
}

fn current_active(app: &AppHandle) -> Option<TabId> {
    app.try_state::<AppState>()?.tabs.lock().unwrap().active
}

fn cycle_tab(app: &AppHandle, delta: isize) -> Result<(), String> {
    let next = {
        let state = app.state::<AppState>();
        let tabs = state.tabs.lock().unwrap();
        if tabs.order.is_empty() {
            return Ok(());
        }
        let len = tabs.order.len() as isize;
        let pos = tabs
            .active
            .and_then(|a| tabs.order.iter().position(|t| *t == a))
            .unwrap_or(0) as isize;
        tabs.order[((pos + delta % len + len) % len) as usize]
    };
    activate_tab_sync(app, next)
}

/// Cmd/Ctrl+1..8 jump to that tab; 9 jumps to the last (browser convention).
fn jump_to_index(app: &AppHandle, n: usize) -> Result<(), String> {
    let target = {
        let state = app.state::<AppState>();
        let tabs = state.tabs.lock().unwrap();
        if tabs.order.is_empty() {
            return Ok(());
        }
        if n >= 9 {
            *tabs.order.last().unwrap()
        } else {
            match tabs.order.get(n.saturating_sub(1)) {
                Some(id) => *id,
                None => return Ok(()),
            }
        }
    };
    activate_tab_sync(app, target)
}

fn focus_address(app: &AppHandle) {
    if let Some(chrome) = app.get_webview("chrome") {
        chrome.set_focus().ok();
    }
    emit_chrome(app, "fedsurf://focus-address", ());
}

fn eval_active(app: &AppHandle, js: &str) -> Result<(), String> {
    if let Some(w) = active_label(app).and_then(|l| app.get_webview(&l)) {
        w.eval(js).map_err(|e| e.to_string())?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Address-bar smarts
// ---------------------------------------------------------------------------

/// Turn address-bar input into a URL:
///   - explicit scheme (fed/http/https) passes through
///   - bare word or spaces -> Federate search
///   - `label.tld` where the TLD exists in the verified Federate root -> fed://
///   - anything else with a dot -> https://
async fn normalize_input(app: &AppHandle, input: &str) -> Result<String, String> {
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
// Commands (chrome webview -> Rust)
// ---------------------------------------------------------------------------

/// Chrome finished booting: it reports its persisted sidebar width and gets
/// the authoritative tab list back (tabs may have been created before its
/// event listeners existed — deep links, the initial tab).
#[tauri::command]
pub async fn frontend_ready(app: AppHandle, sidebar_px: f64) -> Result<TabsSnapshot, String> {
    on_main(&app, move |app| {
        set_sidebar_width_sync(app, sidebar_px);
        Ok(snapshot(app))
    })
    .await
}

#[tauri::command]
pub async fn create_tab(app: AppHandle, url: Option<String>) -> Result<TabId, String> {
    on_main(&app, move |app| create_tab_sync(app, url, true)).await
}

#[tauri::command]
pub async fn close_tab(app: AppHandle, id: TabId) -> Result<(), String> {
    on_main(&app, move |app| close_tab_sync(app, id)).await
}

#[tauri::command]
pub async fn activate_tab(app: AppHandle, id: TabId) -> Result<(), String> {
    on_main(&app, move |app| activate_tab_sync(app, id)).await
}

#[tauri::command]
pub async fn move_tab(app: AppHandle, id: TabId, to_index: usize) -> Result<(), String> {
    on_main(&app, move |app| move_tab_sync(app, id, to_index)).await
}

#[tauri::command]
pub async fn set_sidebar_width(app: AppHandle, px: f64) -> Result<(), String> {
    on_main(&app, move |app| {
        set_sidebar_width_sync(app, px);
        Ok(())
    })
    .await
}

/// Navigate the active tab. Returns the URL actually loaded so the toolbar can
/// echo it back.
#[tauri::command]
pub async fn navigate(app: AppHandle, input: String) -> Result<String, String> {
    let url = normalize_input(&app, input.trim()).await?;
    let parsed: tauri::Url = url.parse().map_err(|e| format!("invalid URL: {e}"))?;
    let label = active_label(&app).ok_or_else(|| "no active tab".to_string())?;
    let webview = app
        .get_webview(&label)
        .ok_or_else(|| "tab webview not found".to_string())?;
    webview.navigate(parsed).map_err(|e| e.to_string())?;
    Ok(url)
}

#[tauri::command]
pub fn go_back(app: AppHandle) {
    eval_active(&app, "history.back()").ok();
}

#[tauri::command]
pub fn go_forward(app: AppHandle) {
    eval_active(&app, "history.forward()").ok();
}

#[tauri::command]
pub fn reload(app: AppHandle) {
    if let Some(w) = active_label(&app).and_then(|l| app.get_webview(&l)) {
        w.reload().ok();
    }
}

#[tauri::command]
pub async fn go_home(app: AppHandle) -> Result<String, String> {
    navigate(app, HOME_URL.to_string()).await
}

/// Chrome-side diagnostics land in the app log (also catches JS errors that
/// would otherwise vanish inside the webview).
#[tauri::command]
pub fn frontend_log(msg: String) {
    tracing::info!("[chrome] {msg}");
}
