# Fedsurf

A desktop browser for the [Federate Network](https://federate.network) that speaks three schemes:

| Scheme | How it's handled |
|---|---|
| `fed://` | Resolved **in-process** by the `federate-resolution` engine — root zone → TLD record → domain record → manifest → content blocks, every layer signature/hash verified before rendering. No local daemon, no hosts-file edits. |
| `https://` | Native webview networking (system WebKit/WebView2). |
| `http://` | Native webview networking. |

## Architecture

Tauri v2 app, one window, two webviews:

```
┌─────────────────────────────────────────────┐
│ chrome webview (ui/index.html)              │  toolbar: back/forward/reload/home,
│ back · forward · reload · home · [address]  │  address bar with scheme badge
├─────────────────────────────────────────────┤
│ content webview                             │  the page being browsed;
│                                             │  fed:// served by the custom
│                                             │  URI scheme handler in Rust
└─────────────────────────────────────────────┘
```

- `fed://` is registered via Tauri's asynchronous custom URI scheme protocol. The handler parses the URL with `federate-uri`, resolves it with `federate-resolution::Resolver`, and answers with verified bytes + correct MIME type. Verification failures render a distinct security interstitial and content is never shown.
- Address bar smarts: explicit schemes pass through; `label.tld` is checked against the **verified Federate root zone** — known Federate TLDs go to `fed://`, everything else to `https://`; non-host input searches `fed://fed.busca`.
- Verified blocks/manifests/root zones are cached on disk (app data dir), so previously visited fed sites work offline.

Federate crates are consumed as path dependencies from a sibling checkout: `../federatenetwork`.

## Run

```sh
cd src-tauri
cargo run
```

Environment:

- `FEDSURF_BOOTSTRAP` — bootstrap node URL (default `https://federate.network`). For local dev against a dev node: `FEDSURF_BOOTSTRAP=http://127.0.0.1:9000 cargo run`
- `FEDSURF_ROOT_KEY` — pin the Federate Root public key explicitly (recommended). Without it the key is pinned on first use (TOFU) and persisted.

## What "building a browser from scratch" actually means

A browser is: rendering engine + networking + navigation/history + chrome UI + security model. A rendering engine alone (HTML/CSS/JS/layout/media) is tens of millions of lines — Chromium ≈ 30M, Gecko similar; even Ladybird (from-scratch engine, hundreds of contributors) took years to render mainstream sites. Nobody rebuilds that layer; every "new browser" (Arc, Brave, Edge, Orion) embeds an engine and owns everything around it.

Fedsurf's split:

- **rendered by the engine** (system webview): HTML/CSS/JS execution, layout, media
- **owned by Fedsurf**: URL scheme handling (`fed://`), name resolution + cryptographic verification, navigation model, chrome UI, caching policy

## Platform notes

- macOS/Linux: custom scheme URLs appear natively as `fed://…` in the webview.
- Windows: WebView2 maps custom schemes to `http://fed.<host>/…` internally; needs a small mapping shim (not yet wired).
