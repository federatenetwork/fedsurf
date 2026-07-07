# Fedsurf

A desktop browser for the [Federate Network](https://federate.network) that speaks three schemes:

| Scheme | How it's handled |
|---|---|
| `fed://` | Resolved **in-process** by the `federate-resolution` engine — root zone → TLD record → domain record → manifest → content blocks, every layer signature/hash verified before rendering. No local daemon, no hosts-file edits. |
| `https://` | Native webview networking (system WebKit/WebView2). |
| `http://` | Native webview networking. |

Fedsurf is also the **OS-level handler for `fed://` links**: once installed, a `fed://` URL clicked in any app (or typed into Chrome) opens in the running Fedsurf window as a new tab.

## Architecture

Tauri v2 app, one window, many webviews:

```
┌────────────────────────────────────────────────┐
│ toolbar: back · forward · reload · home · [url]│  chrome webview (ui/) fills the
├──────────┬─────────────────────────────────────┤  whole window: toolbar + tab
│ sidebar  │ tab-1  ─ visible                    │  sidebar + background
│  ▪ tab-1 │ tab-2  ─ hidden                     │
│  ▪ tab-2 │ …one native child webview per tab,  │  only the active tab's webview
│  + new   │  overlaid on the content area       │  is shown; the rest are hidden
│  ⊟ fold  │                                     │
└──────────┴─────────────────────────────────────┘
```

- **Tabs** (`src-tauri/src/tabs.rs`): Rust owns the tab list; one child webview per tab (`tab-<id>`), lazily created, destroyed on close. The chrome UI (`ui/js/*.js`, vanilla ES modules, no build step) is a render cache fed by `fedsurf://tab-*` events. The sidebar is collapsible: 240px (favicon + title) ⇄ 52px rail (favicons only), persisted across sessions, drag-to-reorder, middle-click close, tooltips on the rail.
- **fed:// protocol** (`src-tauri/src/fed_protocol.rs`): registered via Tauri's asynchronous custom URI scheme protocol. The handler parses the URL with `federate-uri`, resolves it with `federate-resolution::Resolver`, and answers with verified bytes + correct MIME type. Verification failures render a distinct security interstitial and content is never shown.
- **fed:// deep link** (`tauri-plugin-deep-link` + `tauri-plugin-single-instance`): installed bundles register the scheme with the OS (Info.plist on macOS, registry via NSIS on Windows, `.desktop` `MimeType=x-scheme-handler/fed` on deb/rpm). A second launch is routed into the running instance; URLs open as new tabs.
- **User agent** (`src-tauri/src/ua.rs`): WKWebView/WebKitGTK's bare default UA (no `Version/x Safari/x` token) makes Google & friends serve their 2005-era legacy pages. Fedsurf presents a plain Safari UA; on macOS this must be set via `WKWebView.customUserAgent` directly because `WebviewBuilder::user_agent` is ignored for child webviews.
- **Page ⇄ browser channel** (`src-tauri/src/ipc.rs`): tab pages are remote origins with no Tauri IPC bridge, so an injected script reports url/favicon (and, on Windows/Linux, keyboard shortcuts) by POSTing to the `fedsurf-ipc://` custom scheme; the handler identifies the tab via its webview label.
- **Address bar smarts**: explicit schemes pass through; `label.tld` is checked against the **verified Federate root zone** — known Federate TLDs go to `fed://`, everything else to `https://`; non-host input searches `fed://fed.busca`.
- Verified blocks/manifests/root zones are cached on disk (app data dir), so previously visited fed sites work offline.

Federate crates are consumed as path dependencies from a sibling checkout: `../federatenetwork`.

## Keyboard shortcuts

| | |
|---|---|
| New tab | ⌘/Ctrl+T |
| Close tab (last tab goes home instead) | ⌘/Ctrl+W |
| Next / previous tab | Ctrl+Tab / Ctrl+Shift+Tab |
| Jump to tab 1–8, last tab | ⌘/Ctrl+1–8, ⌘/Ctrl+9 |
| Focus address bar | ⌘/Ctrl+L |
| Toggle sidebar | ⌘/Ctrl+B |
| Back / forward | ⌘/Ctrl+[ / ⌘/Ctrl+] |
| Reload | ⌘/Ctrl+R |

On macOS these are native menu accelerators (they work regardless of focus). On Windows/Linux they are captured by the chrome webview and by a script injected into every page.

## Run (dev)

```sh
cd src-tauri
cargo run
```

Environment:

- `FEDSURF_BOOTSTRAP` — bootstrap node URL (default `https://federate.network`). For local dev against a dev node: `FEDSURF_BOOTSTRAP=http://127.0.0.1:9000 cargo run`
- `FEDSURF_ROOT_KEY` — pin the Federate Root public key explicitly (recommended). Without it the key is pinned on first use (TOFU) and persisted.

In dev, `fed://` OS registration is attempted at runtime on Windows/Linux; on macOS it only works from an installed bundle.

## Build & install (fed:// handler)

```sh
npx @tauri-apps/cli build
```

Bundles for the host platform land in `src-tauri/target/release/bundle/`:

- **macOS**: `.app` + `.dmg` — drag to /Applications; `CFBundleURLTypes` registers `fed://` on first launch.
- **Windows 10/11**: NSIS installer — writes the `fed` protocol registry keys.
- **Linux** (Ubuntu/Mint/Debian → `.deb`; Fedora → `.rpm`; Arch/CachyOS → `.AppImage` or build from source): deb/rpm register `x-scheme-handler/fed` via the `.desktop` file. For the AppImage, install the desktop file manually or run the binary once (runtime registration).

## What "building a browser from scratch" actually means

A browser is: rendering engine + networking + navigation/history + chrome UI + security model. A rendering engine alone (HTML/CSS/JS/layout/media) is tens of millions of lines — Chromium ≈ 30M, Gecko similar; even Ladybird (from-scratch engine, hundreds of contributors) took years to render mainstream sites. Nobody rebuilds that layer; every "new browser" (Arc, Brave, Edge, Orion) embeds an engine and owns everything around it.

Fedsurf's split:

- **rendered by the engine** (system webview): HTML/CSS/JS execution, layout, media
- **owned by Fedsurf**: URL scheme handling (`fed://`), name resolution + cryptographic verification, tab model, navigation model, chrome UI, caching policy

## Platform notes

- macOS/Linux: custom scheme URLs appear natively as `fed://…` in the webview.
- Windows: WebView2 surfaces custom schemes as `https://fed.localhost/…`; Fedsurf maps these back to canonical `fed://` form in the resolver and the address bar (`fed_protocol::canonical_fed_url`).
- Strict-CSP pages can block the injected favicon/shortcut reporter (`fedsurf-ipc`); Fedsurf falls back to `origin/favicon.ico` + letter tiles, and on macOS shortcuts never depend on it.
