//! Per-platform user agent + the script injected into every tab webview.
//!
//! The UA matters: WKWebView/WebKitGTK's bare default (no `Version/x Safari/x`
//! token) makes Google & friends serve their legacy no-JS pages. We present a
//! plain Safari UA instead. Windows keeps WebView2's Chromium default.

#[cfg(target_os = "macos")]
pub const USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.6 Safari/605.1.15";
#[cfg(target_os = "linux")]
pub const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.6 Safari/605.1.15";

/// Where the injected script phones home. Tab pages are remote origins, so the
/// tauri IPC bridge is unavailable; instead they POST to our custom scheme
/// (`register_asynchronous_uri_scheme_protocol("fedsurf-ipc", ...)`), and the
/// handler identifies the tab via `UriSchemeContext::webview_label()`.
/// Windows/WebView2 exposes custom schemes as `https://<scheme>.localhost`.
#[cfg(windows)]
const IPC_ENDPOINT: &str = "https://fedsurf-ipc.localhost/report";
#[cfg(not(windows))]
const IPC_ENDPOINT: &str = "fedsurf-ipc://localhost/report";

#[cfg(target_os = "macos")]
const IS_MAC: &str = "true";
#[cfg(not(target_os = "macos"))]
const IS_MAC: &str = "false";

/// Injected into every page a tab loads (any scheme):
///   1. Federate-styled scrollbar.
///   2. Reports url + best favicon to the fedsurf-ipc scheme (incl. SPA navs).
///   3. Captures browser shortcuts that pages would otherwise swallow.
pub fn init_script() -> String {
    TEMPLATE
        .replace("__ENDPOINT__", IPC_ENDPOINT)
        .replace("__IS_MAC__", IS_MAC)
}

const TEMPLATE: &str = r##"
(function () {
  if (window.__fedsurf) return;
  window.__fedsurf = true;

  /* ---- Federate scrollbar ------------------------------------------- */
  var css = [
    '::-webkit-scrollbar { width: 12px; height: 12px; }',
    '::-webkit-scrollbar-track { background: transparent; }',
    '::-webkit-scrollbar-thumb {',
    '  background: rgba(174, 122, 72, 0.55);',
    '  border-radius: 999px;',
    '  border: 3px solid transparent;',
    '  background-clip: padding-box;',
    '  min-height: 40px;',
    '}',
    '::-webkit-scrollbar-thumb:hover { background: rgba(174, 122, 72, 0.85); background-clip: padding-box; }',
    '::-webkit-scrollbar-thumb:active { background: #544329; background-clip: padding-box; }',
    '::-webkit-scrollbar-corner { background: transparent; }'
  ].join('\n');
  function addStyle() {
    if (document.getElementById('__fedsurf-scrollbar')) return;
    var s = document.createElement('style');
    s.id = '__fedsurf-scrollbar';
    s.textContent = css;
    (document.head || document.documentElement).appendChild(s);
  }
  addStyle();
  document.addEventListener('DOMContentLoaded', addStyle);

  /* ---- metadata reporting ------------------------------------------- */
  var ENDPOINT = '__ENDPOINT__';
  var IS_MAC = __IS_MAC__;
  function send(msg) {
    try {
      fetch(ENDPOINT, { method: 'POST', mode: 'no-cors', keepalive: true, body: JSON.stringify(msg) })
        .catch(function () {});
    } catch (e) {}
  }
  function favicon() {
    var best = '', bestScore = -1;
    var links = document.querySelectorAll('link[rel~="icon"], link[rel="shortcut icon"], link[rel="apple-touch-icon"]');
    for (var i = 0; i < links.length; i++) {
      var l = links[i];
      var href = l.getAttribute('href');
      if (!href) continue;
      var rel = (l.getAttribute('rel') || '').toLowerCase();
      var score = rel.indexOf('apple-touch') !== -1 ? 1 : 2;
      var size = parseInt((l.getAttribute('sizes') || ''), 10) || 0;
      if (size >= 24 && size <= 64) score += 2;
      else if (size > 64) score += 1;
      if (score > bestScore) {
        try { best = new URL(href, document.baseURI).href; bestScore = score; } catch (e) {}
      }
    }
    return best;
  }
  var last = '';
  function report() {
    var msg = { kind: 'meta', url: location.href, favicon: favicon() };
    var key = msg.url + '|' + msg.favicon;
    if (key === last) return;
    last = key;
    send(msg);
  }
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', report);
  else report();
  window.addEventListener('load', function () { setTimeout(report, 50); });
  ['pushState', 'replaceState'].forEach(function (m) {
    var orig = history[m];
    if (!orig) return;
    history[m] = function () {
      var r = orig.apply(this, arguments);
      setTimeout(report, 0);
      return r;
    };
  });
  window.addEventListener('popstate', function () { setTimeout(report, 0); });
  window.addEventListener('hashchange', function () { setTimeout(report, 0); });

  /* ---- browser shortcuts (fire even while the page has focus) --------
     macOS is excluded: the native menu handles accelerators there, and a
     preventDefault here would suppress it (WKWebView gives the page first
     crack at key equivalents). */
  if (IS_MAC) return;
  window.addEventListener('keydown', function (e) {
    var action = '';
    if (e.ctrlKey && !e.metaKey && !e.altKey && e.key === 'Tab') {
      action = e.shiftKey ? 'prev-tab' : 'next-tab';
    } else {
      var mod = IS_MAC ? (e.metaKey && !e.ctrlKey) : (e.ctrlKey && !e.metaKey);
      if (mod && !e.altKey && !e.shiftKey) {
        var k = e.key.toLowerCase();
        if (k === 't') action = 'new-tab';
        else if (k === 'w') action = 'close-tab';
        else if (k === 'l') action = 'focus-address';
        else if (k === 'b') action = 'toggle-sidebar';
        else if (k === 'r') action = 'reload';
        else if (k === '[') action = 'back';
        else if (k === ']') action = 'forward';
        else if (k >= '1' && k <= '9') action = 'tab-' + k;
      }
    }
    if (action) {
      e.preventDefault();
      send({ kind: 'shortcut', action: action });
    }
  }, true);
})();
"##;
