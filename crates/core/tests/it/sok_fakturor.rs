//! Sökningen efter fakturor, kunder och leverantörer (⌘K och sökfältet, story 76), genom `core`
//! mot en tillfällig databas.

use maya_core::aktor::Aktör;
use maya_core::atkomst::ÅtkomstFel;
use maya_core::foretag::FöretagsId;
use maya_core::kund::Kundstatus;
use maya_core::leverantor::Status;
use maya_core::sok::{self, Träffar};
use maya_core::{Databas, Klocka, foretag, kund, leverantor, testhjalp};
use maya_domain::foretag::NyttFöretag;

/// Exempel AB med fakturor och register:
/// - kundfaktura 1 till Bergström Bygg AB, 11 875,00 kr, och kreditfaktura 2 som krediterar den;
/// - den externa kundfakturan EXT-77 till Lindqvist Konsult, 12 500,00 kr;
/// - leverantörsfakturan LF-2026-17 från Kontorsvaror i Norr AB, 1 250,00 kr;
/// - den arkiverade kunden Gamla Kunden AB och den arkiverade leverantören Gamla Tryckeriet.
struct Exempel {
    anna: Aktör,
    exempel: FöretagsId,
    faktura: String,
    kredit: String,
}

async fn exempel(db: &Databas) -> Exempel {
    let (anna, exempel) = testhjalp::fakturaföretag(db).await;
    let klocka = Klocka::system();
    let f = testhjalp::utfärdad_kundfaktura(db, &anna, exempel, "Bergström Bygg AB", "2026-03-10")
        .await;
    let k = testhjalp::utfärdad_kreditfaktura(db, &anna, exempel, f.id).await;
    let lindqvist = testhjalp::kund(db, &anna, exempel, "Lindqvist Konsult").await;
    testhjalp::registrerad_extern_kundfaktura(db, &anna, exempel, lindqvist, "EXT-77").await;
    let norr = testhjalp::leverantör(db, &anna, exempel, "Kontorsvaror i Norr AB").await;
    testhjalp::registrerad_leverantörsfaktura(db, &anna, exempel, norr, "LF-2026-17", "").await;
    let gamla = testhjalp::kund(db, &anna, exempel, "Gamla Kunden AB").await;
    kund::arkivera(db, &klocka, &anna, exempel, gamla)
        .await
        .unwrap();
    let tryckeriet = testhjalp::leverantör(db, &anna, exempel, "Gamla Tryckeriet").await;
    leverantor::arkivera(db, &klocka, &anna, exempel, tryckeriet)
        .await
        .unwrap();
    Exempel {
        anna,
        exempel,
        faktura: f.nummer.to_string(),
        kredit: k.nummer.to_string(),
    }
}

async fn sök(db: &Databas, e: &Exempel, fråga: &str) -> Träffar {
    sok::sök(db, &e.anna, e.exempel, fråga).await.unwrap()
}

fn kundfakturor(t: &Träffar) -> Vec<String> {
    t.kundfakturor
        .iter()
        .map(|f| f.fakturanummer.clone())
        .collect()
}

fn leverantörsfakturor(t: &Träffar) -> Vec<String> {
    t.leverantörsfakturor
        .iter()
        .map(|f| f.fakturanummer.clone())
        .collect()
}

fn kunder(t: &Träffar) -> Vec<String> {
    t.kunder.iter().map(|k| k.namn.clone()).collect()
}

fn leverantörer(t: &Träffar) -> Vec<String> {
    t.leverantörer
        .iter()
        .map(|l| l.uppgifter.namn.clone())
        .collect()
}

#[tokio::test]
async fn fakturanumret_hittar_mayas_egna_externa_kreditfakturor_och_leverantörsfakturor() {
    let db = Databas::tillfällig().await;
    let e = exempel(&db).await;
    let t = sök(&db, &e, &e.faktura).await;
    assert_eq!(kundfakturor(&t), [e.faktura.as_str()]);
    assert!(t.leverantörsfakturor.is_empty());
    assert_eq!(
        kundfakturor(&sök(&db, &e, &e.kredit).await),
        [e.kredit.as_str()]
    );
    assert_eq!(kundfakturor(&sök(&db, &e, "ext-77").await), ["EXT-77"]);
    let t = sök(&db, &e, "LF-2026-17").await;
    assert_eq!(leverantörsfakturor(&t), ["LF-2026-17"]);
    assert!(t.kundfakturor.is_empty());
}

#[tokio::test]
async fn kundens_namn_hittar_kunden_och_dess_fakturor() {
    let db = Databas::tillfällig().await;
    let e = exempel(&db).await;
    let t = sök(&db, &e, "bergström").await;
    assert_eq!(kundfakturor(&t), [e.kredit.as_str(), e.faktura.as_str()]);
    assert_eq!(kunder(&t), ["Bergström Bygg AB"]);
    assert!(t.leverantörsfakturor.is_empty() && t.leverantörer.is_empty());
    let t = sök(&db, &e, "LINDQVIST").await;
    assert_eq!(kundfakturor(&t), ["EXT-77"]);
    assert_eq!(kunder(&t), ["Lindqvist Konsult"]);
}

#[tokio::test]
async fn leverantörens_namn_hittar_leverantören_och_dess_fakturor() {
    let db = Databas::tillfällig().await;
    let e = exempel(&db).await;
    let t = sök(&db, &e, "kontorsvaror norr").await;
    assert_eq!(leverantörsfakturor(&t), ["LF-2026-17"]);
    assert_eq!(leverantörer(&t), ["Kontorsvaror i Norr AB"]);
    assert!(t.kundfakturor.is_empty() && t.kunder.is_empty());
}

#[tokio::test]
async fn beloppet_hittar_fakturorna_också_en_kreditfaktura() {
    let db = Databas::tillfällig().await;
    let e = exempel(&db).await;
    let t = sök(&db, &e, "11 875").await;
    assert_eq!(kundfakturor(&t), [e.kredit.as_str(), e.faktura.as_str()]);
    assert_eq!(kundfakturor(&sök(&db, &e, "12500,00").await), ["EXT-77"]);
    let t = sök(&db, &e, "1 250").await;
    assert_eq!(leverantörsfakturor(&t), ["LF-2026-17"]);
    assert!(t.kundfakturor.is_empty());
}

#[tokio::test]
async fn arkiverade_kunder_och_leverantörer_hittas_och_är_märkta() {
    let db = Databas::tillfällig().await;
    let e = exempel(&db).await;
    let t = sök(&db, &e, "gamla").await;
    assert_eq!(kunder(&t), ["Gamla Kunden AB"]);
    assert_eq!(t.kunder[0].status, Kundstatus::Arkiverad);
    assert_eq!(leverantörer(&t), ["Gamla Tryckeriet"]);
    assert_eq!(t.leverantörer[0].status, Status::Arkiverad);
    let t = sök(&db, &e, "bergström").await;
    assert_eq!(t.kunder[0].status, Kundstatus::Aktiv);
}

#[tokio::test]
async fn sökningen_hittar_aldrig_fakturor_eller_register_i_andra_företag() {
    let db = Databas::tillfällig().await;
    let e = exempel(&db).await;
    let annat = foretag::lägg_upp(
        &db,
        &Klocka::system(),
        &e.anna,
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
    for fråga in [
        e.faktura.as_str(),
        "EXT-77",
        "LF-2026-17",
        "bergström",
        "lindqvist",
        "norr",
        "gamla",
        "11 875",
        "1 250",
    ] {
        let t = sok::sök(&db, &e.anna, annat, fråga).await.unwrap();
        assert!(
            t.kundfakturor.is_empty()
                && t.leverantörsfakturor.is_empty()
                && t.kunder.is_empty()
                && t.leverantörer.is_empty(),
            "{fråga}"
        );
    }
}

#[tokio::test]
async fn den_som_saknar_åtkomst_kan_inte_söka_och_en_tom_fråga_träffar_inget() {
    let db = Databas::tillfällig().await;
    let e = exempel(&db).await;
    let bertil = testhjalp::användare(&db, "bertil@example.se").await;
    for fråga in ["bergström", ""] {
        assert!(matches!(
            sok::sök(&db, &bertil, e.exempel, fråga).await,
            Err(ÅtkomstFel::IngenÅtkomst)
        ));
    }
    let t = sök(&db, &e, "  ").await;
    assert!(
        t.verifikat.is_empty()
            && t.kundfakturor.is_empty()
            && t.leverantörsfakturor.is_empty()
            && t.kunder.is_empty()
            && t.leverantörer.is_empty()
    );
}
