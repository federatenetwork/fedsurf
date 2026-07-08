//! The fed:// protocol handler: resolves Federate URIs in-process (root zone
//! -> TLD -> domain record -> manifest -> blocks, every layer signature/hash
//! verified before a single byte is rendered) and renders the branded
//! error/security pages when resolution fails.

use federate_resolution::{Resolved, Resolver};
use federate_uri::FederateUri;
use std::sync::Arc;

/// WebView2 (and Android) surface custom schemes as `https://<scheme>.localhost/...`.
/// Map those request URLs back to canonical `fed://` form so parsing, state and
/// the address bar all speak one dialect on every platform.
pub fn canonical_fed_url(raw: &str) -> String {
    for prefix in ["https://fed.localhost/", "http://fed.localhost/"] {
        if let Some(rest) = raw.strip_prefix(prefix) {
            return format!("fed://{rest}");
        }
    }
    raw.to_string()
}

pub async fn serve_fed(resolver: Arc<Resolver>, raw_url: &str) -> tauri::http::Response<Vec<u8>> {
    let raw_url = canonical_fed_url(raw_url);
    let uri = match FederateUri::parse(&raw_url) {
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

pub fn error_page(status: u16, title: &str, detail: &str) -> tauri::http::Response<Vec<u8>> {
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
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = u32::from_be_bytes([0, b[0], b[1], b[2]]);
        out.push(TABLE[(n >> 18) as usize & 63] as char);
        out.push(TABLE[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            TABLE[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[n as usize & 63] as char
        } else {
            '='
        });
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
