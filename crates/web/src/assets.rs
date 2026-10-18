//! Inbäddade assets: `ETag` ur innehållets SHA-256 och `Cache-Control: no-cache`, så att
//! webbläsaren alltid frågar och får `304` när filen inte har ändrats.

use axum::http::{HeaderMap, HeaderValue, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use rust_embed::RustEmbed;

/// Sajten som cargo-leptos bygger (`/pkg/…` och filerna i `public`), inbäddad vid kompileringen.
#[derive(RustEmbed)]
#[folder = "$LEPTOS_SITE_ROOT"]
#[allow_missing = true]
struct Site;

pub(crate) async fn serve_site(uri: Uri, headers: HeaderMap) -> Response {
    serve_embedded::<Site>(uri.path(), &headers)
}

/// Svarar med filen `path` ur `E`. En förkomprimerad `.br` eller `.gz` väljs när klienten
/// tar emot den.
pub fn serve_embedded<E: RustEmbed>(path: &str, headers: &HeaderMap) -> Response {
    let path = path.trim_start_matches('/');
    let Some(original) = E::get(path) else {
        return (StatusCode::NOT_FOUND, "Sidan finns inte\n").into_response();
    };
    let accepts = headers
        .get(header::ACCEPT_ENCODING)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    let (file, encoding) = [("br", ".br"), ("gzip", ".gz")]
        .into_iter()
        .filter(|(encoding, _)| accepts.split(',').any(|a| a.trim().starts_with(encoding)))
        .find_map(|(encoding, suffix)| {
            E::get(&format!("{path}{suffix}")).map(|f| (f, Some(encoding)))
        })
        .unwrap_or((original.clone(), None));

    let etag = format!("\"{}\"", hex(&file.metadata.sha256_hash()));
    let mut response = if headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.split(',').any(|tag| tag.trim() == etag))
    {
        StatusCode::NOT_MODIFIED.into_response()
    } else {
        file.data.into_owned().into_response()
    };

    let h = response.headers_mut();
    if let Ok(etag) = HeaderValue::from_str(&etag) {
        h.insert(header::ETAG, etag);
    }
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    h.insert(header::VARY, HeaderValue::from_static("accept-encoding"));
    if let Ok(mime) = HeaderValue::from_str(original.metadata.mimetype()) {
        h.insert(header::CONTENT_TYPE, mime);
    }
    if let Some(encoding) = encoding {
        h.insert(header::CONTENT_ENCODING, HeaderValue::from_static(encoding));
    }
    response
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
