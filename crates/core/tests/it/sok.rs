//! Sökningen efter verifikat (⌘K och sökfältet), genom `core` mot en tillfällig databas.

use maya_core::aktor::Aktör;
use maya_core::atkomst::ÅtkomstFel;
use maya_core::foretag::FöretagsId;
use maya_core::verifikat;
use maya_core::{Databas, Klocka, foretag, testhjalp};
use maya_domain::foretag::NyttFöretag;
use maya_domain::verifikat::{NyRad, NyttVerifikat};

fn rad(konto: &str, debet: &str, kredit: &str, text: &str) -> NyRad {
    NyRad {
        konto: konto.into(),
        debet: debet.into(),
        kredit: kredit.into(),
        text: text.into(),
    }
}

async fn bokför(
    db: &Databas,
    aktör: &Aktör,
    företag: FöretagsId,
    datum: &str,
    beskrivning: &str,
    rader: Vec<NyRad>,
) {
    verifikat::bokför(
        db,
        &Klocka::system(),
        aktör,
        företag,
        &NyttVerifikat {
            datum: datum.into(),
            beskrivning: beskrivning.into(),
            rader,
        },
    )
    .await
    .unwrap();
}

/// Anna med två företag: Exempel AB med tre verifikat och Annat AB med ett, som liknar
/// Exempels V1 i allt.
async fn två_företag(db: &Databas) -> (Aktör, FöretagsId, FöretagsId) {
    let (anna, exempel) = testhjalp::företag(db).await;
    let annat = foretag::lägg_upp(
        db,
        &Klocka::system(),
        &anna,
        &NyttFöretag {
            orgnr: "559900-0006".into(),
            namn: "Annat AB".into(),
            första_år_start: "2026-01-01".into(),
            första_år_slut: "2026-12-31".into(),
            bokföringsmetod: "faktura".into(),
            momsperiod: "kvartal".into(),
        },
    )
    .await
    .unwrap();
    bokför(
        db,
        &anna,
        exempel,
        "2026-03-14",
        "Kaffe till kontoret",
        vec![rad("5460", "250", "", ""), rad("1930", "", "250", "")],
    )
    .await;
    bokför(
        db,
        &anna,
        exempel,
        "2026-04-02",
        "Försäljning konsulttimmar",
        vec![
            rad("1510", "12 500", "", "Kund Bergström"),
            rad("3001", "", "10 000", ""),
            rad("2611", "", "2 500", ""),
        ],
    )
    .await;
    bokför(
        db,
        &anna,
        exempel,
        "2026-04-20",
        "Hyra april",
        vec![rad("5010", "8 000", "", ""), rad("1930", "", "8 000", "")],
    )
    .await;
    bokför(
        db,
        &anna,
        annat,
        "2026-03-14",
        "Kaffe till kontoret",
        vec![rad("5460", "250", "", ""), rad("1930", "", "250", "")],
    )
    .await;
    (anna, exempel, annat)
}

async fn sök(db: &Databas, aktör: &Aktör, företag: FöretagsId, fråga: &str) -> Vec<String> {
    verifikat::sök(db, aktör, företag, fråga)
        .await
        .unwrap()
        .into_iter()
        .map(|v| v.nummer.expect("bokfört").to_string())
        .collect()
}

#[tokio::test]
async fn verifikationsnumret_hittar_verifikatet_med_eller_utan_v() {
    let db = Databas::tillfällig().await;
    let (anna, exempel, _) = två_företag(&db).await;
    assert_eq!(sök(&db, &anna, exempel, "V2").await, ["V2"]);
    assert_eq!(sök(&db, &anna, exempel, "v3").await, ["V3"]);
    assert_eq!(sök(&db, &anna, exempel, "2").await, ["V2"]);
}

#[tokio::test]
async fn texten_hittar_verifikatet_på_beskrivning_eller_radtext_utan_hänsyn_till_versaler() {
    let db = Databas::tillfällig().await;
    let (anna, exempel, _) = två_företag(&db).await;
    assert_eq!(sök(&db, &anna, exempel, "hyra").await, ["V3"]);
    assert_eq!(sök(&db, &anna, exempel, "FÖRSÄLJNING").await, ["V2"]);
    assert_eq!(sök(&db, &anna, exempel, "bergström").await, ["V2"]);
    assert_eq!(sök(&db, &anna, exempel, "kaffe kontoret").await, ["V1"]);
    assert!(sök(&db, &anna, exempel, "kaffe hyra").await.is_empty());
}

#[tokio::test]
async fn beloppet_hittar_verifikatet_på_summan_eller_en_rad() {
    let db = Databas::tillfällig().await;
    let (anna, exempel, _) = två_företag(&db).await;
    assert_eq!(sök(&db, &anna, exempel, "12 500").await, ["V2"]);
    assert_eq!(sök(&db, &anna, exempel, "12500,00").await, ["V2"]);
    assert_eq!(sök(&db, &anna, exempel, "2 500").await, ["V2"]);
    assert_eq!(sök(&db, &anna, exempel, "8000").await, ["V3"]);
}

#[tokio::test]
async fn datumet_eller_månaden_hittar_verifikaten() {
    let db = Databas::tillfällig().await;
    let (anna, exempel, _) = två_företag(&db).await;
    assert_eq!(sök(&db, &anna, exempel, "2026-03-14").await, ["V1"]);
    assert_eq!(sök(&db, &anna, exempel, "2026-04").await, ["V3", "V2"]);
}

#[tokio::test]
async fn sökningen_hittar_aldrig_verifikat_i_andra_företag() {
    let db = Databas::tillfällig().await;
    let (anna, exempel, annat) = två_företag(&db).await;
    let exempels = verifikat::lista(&db, &anna, exempel, verifikat::Urval::Alla)
        .await
        .unwrap();
    for fråga in [
        "V1",
        "1",
        "kaffe",
        "250",
        "2026-03-14",
        "2026-04",
        "hyra",
        "12 500",
    ] {
        let träffar = verifikat::sök(&db, &anna, annat, fråga).await.unwrap();
        assert!(
            träffar
                .iter()
                .all(|v| exempels.iter().all(|e| e.id != v.id)),
            "{fråga}"
        );
    }
    assert_eq!(sök(&db, &anna, annat, "kaffe").await, ["V1"]);
    assert!(sök(&db, &anna, annat, "hyra").await.is_empty());
}

#[tokio::test]
async fn den_som_saknar_åtkomst_till_företaget_kan_inte_söka_i_det() {
    let db = Databas::tillfällig().await;
    let (_, exempel, _) = två_företag(&db).await;
    let bertil = testhjalp::användare(&db, "bertil@example.se").await;
    for fråga in ["kaffe", ""] {
        assert!(matches!(
            verifikat::sök(&db, &bertil, exempel, fråga).await,
            Err(ÅtkomstFel::IngenÅtkomst)
        ));
    }
}

#[tokio::test]
async fn en_tom_fråga_träffar_inget() {
    let db = Databas::tillfällig().await;
    let (anna, exempel, _) = två_företag(&db).await;
    assert!(sök(&db, &anna, exempel, "  ").await.is_empty());
}
