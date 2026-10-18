use std::net::IpAddr;

use axum::http::{HeaderMap, HeaderValue};
use maya_core::config::Config;

fn config() -> Config {
    Config::from_env(
        [
            ("MAYA_PUBLIC_URL", "https://bokforing.example.se"),
            ("MAYA_BACKUP_DIR", "/backup"),
            ("MAYA_TRUSTED_PROXY", "172.31.80.2"),
        ]
        .map(|(k, v)| (k.to_string(), v.to_string())),
    )
    .unwrap()
}

fn ip(s: &str) -> IpAddr {
    s.parse().unwrap()
}

fn forwarded_for(value: &'static str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert("x-forwarded-for", HeaderValue::from_static(value));
    headers
}

#[test]
fn x_forwarded_for_fran_den_konfigurerade_proxyn_ger_klientens_adress() {
    let headers = forwarded_for("203.0.113.7");

    assert_eq!(
        maya_web::client_ip(ip("172.31.80.2"), &headers, &config()),
        ip("203.0.113.7")
    );
}

#[test]
fn x_forwarded_for_fran_nagon_annan_ignoreras() {
    let headers = forwarded_for("203.0.113.7");

    assert_eq!(
        maya_web::client_ip(ip("198.51.100.9"), &headers, &config()),
        ip("198.51.100.9")
    );
}

#[test]
fn en_forfalskad_adress_forst_i_kedjan_ignoreras() {
    // Klienten skickade själv "10.0.0.1"; proxyn lade till klientens verkliga adress sist.
    let headers = forwarded_for("10.0.0.1, 203.0.113.7");

    assert_eq!(
        maya_web::client_ip(ip("172.31.80.2"), &headers, &config()),
        ip("203.0.113.7")
    );
}
