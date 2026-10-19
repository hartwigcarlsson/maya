use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "tests/fixtures/site"]
struct Fixtures;

/// `shasum -a 256 tests/fixtures/site/pkg/maya.css`
const ETAG: &str = "\"6431abd38866d230ea26466a973107e9df8d10b3456746f632a6998ac35930cb\"";

#[test]
fn en_asset_far_etag_ur_innehallets_hash_och_no_cache() {
    let response = maya_web::serve_embedded::<Fixtures>("/pkg/maya.css", &HeaderMap::new());

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::ETAG], ETAG);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-cache");
    assert_eq!(response.headers()[header::CONTENT_TYPE], "text/css");
}

#[test]
fn en_oforandrad_asset_ger_304() {
    let mut headers = HeaderMap::new();
    headers.insert(header::IF_NONE_MATCH, HeaderValue::from_static(ETAG));

    let response = maya_web::serve_embedded::<Fixtures>("/pkg/maya.css", &headers);

    assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
}

#[test]
fn en_asset_som_saknas_ger_404() {
    let response = maya_web::serve_embedded::<Fixtures>("/pkg/finns-inte.js", &HeaderMap::new());

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
