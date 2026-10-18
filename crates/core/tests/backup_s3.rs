//! Backup till S3-kompatibel lagring: en S3-tjänst i processen (path-style, PUT/GET/DELETE och
//! ListObjectsV2) som kräver SigV4-rubrikerna och kontrollerar kroppens SHA-256.
//! Signaturens beräkning prövas mot AWS:s exempel i `backup::s3`.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use axum::body::Bytes;
use axum::http::{HeaderMap, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use maya_core::aktor::Aktör;
use maya_core::backup::{self, Backupmål};
use maya_core::config::Config;
use maya_core::kontrollsumma::Kontrollsummor;
use maya_core::{Databas, Klocka, testhjalp, underlag};
use sha2::{Digest, Sha256};

type Lager = Arc<Mutex<BTreeMap<String, Vec<u8>>>>;

const BUCKET: &str = "maya-backup";
const NYCKEL_ID: &str = "testnyckel";

/// Startar tjänsten och ger dess adress och lagret.
async fn s3_tjänst() -> (String, Lager) {
    let lager: Lager = Arc::default();
    let l = lager.clone();
    let app = axum::Router::new().fallback(move |metod, uri, rubriker, kropp| {
        let l = l.clone();
        async move { hantera(l, metod, uri, rubriker, kropp) }
    });
    let lyssnare = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let adress = lyssnare.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(lyssnare, app).await.unwrap() });
    (format!("http://{adress}"), lager)
}

fn hantera(lager: Lager, metod: Method, uri: Uri, rubriker: HeaderMap, kropp: Bytes) -> Response {
    let rubrik = |namn: &str| {
        rubriker
            .get(namn)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
    };
    if !rubrik("authorization").starts_with(&format!("AWS4-HMAC-SHA256 Credential={NYCKEL_ID}/"))
        || rubrik("x-amz-date").len() != 16
    {
        return (
            StatusCode::FORBIDDEN,
            "<Error><Code>AccessDenied</Code></Error>",
        )
            .into_response();
    }
    if rubrik("x-amz-content-sha256") != hex::encode(Sha256::digest(&kropp)) {
        return (
            StatusCode::BAD_REQUEST,
            "<Error><Code>XAmzContentSHA256Mismatch</Code></Error>",
        )
            .into_response();
    }
    let väg = uri.path().trim_start_matches('/');
    let (bucket, nyckel) = väg.split_once('/').unwrap_or((väg, ""));
    if bucket != BUCKET {
        return (
            StatusCode::NOT_FOUND,
            "<Error><Code>NoSuchBucket</Code></Error>",
        )
            .into_response();
    }
    let mut lager = lager.lock().unwrap();
    match (metod, nyckel) {
        (Method::GET, "") => {
            let fråga = uri.query().unwrap_or("");
            assert!(fråga.contains("list-type=2"), "{fråga}");
            let prefix = fråga
                .split('&')
                .find_map(|p| p.strip_prefix("prefix="))
                .unwrap_or("");
            let mut xml = String::from("<ListBucketResult><IsTruncated>false</IsTruncated>");
            for nyckel in lager.keys().filter(|k| k.starts_with(prefix)) {
                xml.push_str(&format!("<Contents><Key>{nyckel}</Key></Contents>"));
            }
            xml.push_str("</ListBucketResult>");
            xml.into_response()
        }
        (Method::GET, nyckel) => match lager.get(nyckel) {
            Some(innehåll) => innehåll.clone().into_response(),
            None => (
                StatusCode::NOT_FOUND,
                "<Error><Code>NoSuchKey</Code></Error>",
            )
                .into_response(),
        },
        (Method::PUT, nyckel) => {
            lager.insert(nyckel.to_string(), kropp.to_vec());
            StatusCode::OK.into_response()
        }
        (Method::DELETE, nyckel) => {
            lager.remove(nyckel);
            StatusCode::NO_CONTENT.into_response()
        }
        _ => StatusCode::METHOD_NOT_ALLOWED.into_response(),
    }
}

fn mål(endpoint: &str, hemlighet_fil: &std::path::Path) -> Backupmål {
    std::fs::write(hemlighet_fil, "hemlig\n").unwrap();
    let config = Config::from_env(
        [
            ("MAYA_PUBLIC_URL", "https://bokforing.example.se"),
            ("MAYA_BACKUP_S3_ENDPOINT", endpoint),
            ("MAYA_BACKUP_S3_BUCKET", BUCKET),
            ("MAYA_BACKUP_S3_ACCESS_KEY_ID", NYCKEL_ID),
            (
                "MAYA_BACKUP_S3_SECRET_ACCESS_KEY_FILE",
                hemlighet_fil.to_str().unwrap(),
            ),
        ]
        .map(|(k, v)| (k.to_string(), v.to_string())),
    )
    .unwrap();
    Backupmål::från(&config.backup)
}

#[tokio::test(flavor = "multi_thread")]
async fn backup_till_s3_verifieras_gallras_och_återställs_med_samma_kontrollsummor() {
    let db = Databas::tillfällig().await;
    let (anna, företag) = testhjalp::företag(&db).await;
    testhjalp::bokför(
        &db,
        &anna,
        företag,
        "2026-03-14",
        &[(1930, 12_500, 0), (3001, 0, 12_500)],
    )
    .await;
    underlag::ladda_upp(
        &db,
        &Klocka::system(),
        &anna,
        företag,
        "kvitto.txt",
        "text/plain",
        b"kvitto 1",
    )
    .await
    .unwrap();
    let före = Kontrollsummor::räkna(&db).await.unwrap();

    let (endpoint, lager) = s3_tjänst().await;
    let (arbete, tom) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mål = mål(&endpoint, &arbete.path().join("hemlighet"));

    // Åtta nätter i rad: den första gallras.
    for dag in 10..=17 {
        let klocka = Klocka::fast(format!("2026-10-{dag}T01:00:00Z").parse().unwrap());
        let resultat = backup::ta_backup(&db, &klocka, &Aktör::maya(), &mål, arbete.path())
            .await
            .unwrap();
        assert!(
            resultat.verifiering.lyckades(),
            "{:?}",
            resultat.verifiering.problem
        );
    }
    let nycklar: Vec<String> = lager.lock().unwrap().keys().cloned().collect();
    assert_eq!(nycklar.len(), 14, "{nycklar:?}");
    assert!(
        !nycklar.iter().any(|n| n.starts_with("maya-20261010")),
        "{nycklar:?}"
    );
    assert!(nycklar.contains(&"maya-20261017T010000Z.json".to_string()));

    // Verifieringen ur S3 upptäcker en ändrad fil.
    let senaste = "maya-20261017T010000Z";
    let verifiering = backup::verifiera(
        &db,
        &Klocka::system(),
        &Aktör::cli(),
        &mål,
        None,
        arbete.path(),
    )
    .await
    .unwrap();
    assert_eq!(verifiering.backup.namn, senaste);
    assert!(verifiering.lyckades(), "{:?}", verifiering.problem);

    let fil = backup::hämta(&mål, senaste, tom.path()).await.unwrap();
    let återställd = Databas::öppna(&fil).await.unwrap();
    assert_eq!(Kontrollsummor::räkna(&återställd).await.unwrap(), före);

    lager
        .lock()
        .unwrap()
        .get_mut(&format!("{senaste}.db"))
        .unwrap()
        .push(0);
    let verifiering = backup::verifiera(
        &db,
        &Klocka::system(),
        &Aktör::cli(),
        &mål,
        None,
        arbete.path(),
    )
    .await
    .unwrap();
    assert!(
        verifiering
            .problem
            .contains(&"databasfilens SHA-256 stämmer inte med manifestet".to_string()),
        "{:?}",
        verifiering.problem
    );
    // Arbetskatalogen innehåller bara hemligheten.
    assert_eq!(std::fs::read_dir(arbete.path()).unwrap().count(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn fel_från_s3_sparas_som_misslyckad_backup() {
    let db = Databas::tillfällig().await;
    let (endpoint, _) = s3_tjänst().await;
    let arbete = tempfile::tempdir().unwrap();
    let mål = mål(
        &format!("{endpoint}/fel-prefix"),
        &arbete.path().join("hemlighet"),
    );
    let fel = backup::ta_backup(&db, &Klocka::system(), &Aktör::maya(), &mål, arbete.path()).await;
    assert!(fel.is_err());
    let status = backup::status(&db).await.unwrap();
    let senaste = status.senaste_backup.unwrap();
    assert!(!senaste.lyckades);
    assert!(senaste.meddelande.contains("404"), "{}", senaste.meddelande);
}
