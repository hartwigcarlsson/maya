use maya_core::Databas;
use maya_core::installation::{self, RelyingParty};

fn rp(url: &str) -> RelyingParty {
    RelyingParty::från_publik_url(&url.parse().unwrap())
}

#[tokio::test]
async fn rp_id_och_origin_harleds_ur_den_publika_url_en() {
    let rp = rp("https://bokforing.example.se");

    assert_eq!(rp.rp_id, "bokforing.example.se");
    assert_eq!(rp.origin, "https://bokforing.example.se");
}

#[tokio::test]
async fn rp_id_sparas_vid_forsta_start_och_samma_godtas_sedan() {
    let db = Databas::tillfällig().await;

    installation::kontrollera_rp(&db, &rp("https://bokforing.example.se"))
        .await
        .unwrap();
    installation::kontrollera_rp(&db, &rp("https://bokforing.example.se"))
        .await
        .unwrap();
}

#[tokio::test]
async fn maya_vagrar_starta_nar_den_publika_url_en_ger_ett_annat_rp_id() {
    let db = Databas::tillfällig().await;
    installation::kontrollera_rp(&db, &rp("https://bokforing.example.se"))
        .await
        .unwrap();

    let err = installation::kontrollera_rp(&db, &rp("https://annan.example.se"))
        .await
        .unwrap_err();

    let text = err.to_string();
    assert!(text.contains("bokforing.example.se"), "{text}");
    assert!(text.contains("annan.example.se"), "{text}");
    assert!(text.contains("maya domain change"), "{text}");
}

#[tokio::test]
async fn maya_vagrar_starta_nar_origin_har_bytt_port() {
    let db = Databas::tillfällig().await;
    installation::kontrollera_rp(&db, &rp("http://localhost:3102"))
        .await
        .unwrap();

    let err = installation::kontrollera_rp(&db, &rp("http://localhost:3103")).await;

    assert!(err.is_err());
}
