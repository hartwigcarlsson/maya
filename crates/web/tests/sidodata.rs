//! Sidornas data hämtas med `Resource` och server functions (`sidodata.rs`). En sådan sida får
//! inte hänga när en annan router byggs samtidigt i processen, som när testerna körs parallellt:
//! `generate_route_list` stänger av alla resurser i processen medan den renderar appen.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use maya_core::Databas;
use maya_core::config::Config;
use tower::ServiceExt;

fn config() -> Config {
    Config::from_env(
        [
            ("MAYA_PUBLIC_URL", "https://bokforing.example.se"),
            ("MAYA_BACKUP_DIR", "/backup"),
        ]
        .map(|(k, v)| (k.to_string(), v.to_string())),
    )
    .unwrap()
}

#[tokio::test]
async fn en_sida_med_resource_hanger_inte_medan_andra_routrar_byggs() {
    let db = Databas::tillfällig().await;
    maya_core::testhjalp::användare(&db, "anna@example.se").await;
    let app = maya_web::router(Arc::new(config()), (*db).clone());

    let klart = Arc::new(AtomicBool::new(false));
    let byggare: Vec<_> = (0..4)
        .map(|_| {
            let klart = klart.clone();
            let db = (*db).clone();
            std::thread::spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap();
                runtime.block_on(async {
                    while !klart.load(Ordering::Relaxed) {
                        drop(maya_web::router(Arc::new(config()), db.clone()));
                        tokio::task::yield_now().await;
                    }
                });
            })
        })
        .collect();

    for i in 0..50 {
        let svar = tokio::time::timeout(Duration::from_secs(5), async {
            let response = app
                .clone()
                .oneshot(
                    Request::get("/passkeys")
                        .header(header::HOST, "bokforing.example.se")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            let status = response.status();
            let body = response.into_body().collect().await.unwrap().to_bytes();
            (status, String::from_utf8_lossy(&body).into_owned())
        })
        .await;
        let Ok((status, body)) = svar else {
            klart.store(true, Ordering::Relaxed);
            panic!("begäran {i} hängde");
        };
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("Fortsätt med passkey"), "{body}");
    }
    klart.store(true, Ordering::Relaxed);
    for b in byggare {
        b.join().unwrap();
    }
}
