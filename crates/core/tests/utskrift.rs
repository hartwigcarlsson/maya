//! Utskrift som PDF: verifikatet och kontoplanen ur databasen, med sidfoten från klockan och
//! aktören. Mallarnas utseende prövas i `maya-pdf`; här syns att rätt data kommer med.

use maya_core::atkomst::ÅtkomstFel;
use maya_core::behandlingshistorik::Urval;
use maya_core::utskrift::{self, UtskriftFel};
use maya_core::{Databas, Klocka, kontoplan, testhjalp, underlag};
use maya_domain::kontoplan::Kontonummer;
use maya_pdf::testhjalp::{innehåller, sidor, är_pdfa_2b};

fn klocka() -> Klocka {
    Klocka::fast("2026-10-18T12:34:00Z".parse().unwrap())
}

#[tokio::test]
async fn verifikatet_skrivs_ut_med_rader_historik_och_sidfot() {
    let db = Databas::tillfällig().await;
    let (anna, företag) = testhjalp::företag(&db).await;
    let v = testhjalp::bokför(
        &db,
        &anna,
        företag,
        "2026-03-14",
        &[(1930, 1_250_000, 0), (3001, 0, 1_250_000)],
    )
    .await;

    let pdf = utskrift::verifikat(&db, &klocka(), &anna, företag, v.id)
        .await
        .unwrap()
        .expect("verifikatet finns");

    assert_eq!(pdf.filnamn, "V1.pdf");
    är_pdfa_2b(&pdf.innehåll);
    let text = sidor(&pdf.innehåll).concat();
    for väntat in [
        "Exempel AB",
        "556036-0793",
        "V1",
        "Testverifikat 2026-03-14",
        "period mars 2026",
        "1930",
        "Företagskonto",
        "3001",
        "12 500,00",
        "Inga underlag",
        "verifikat bokfört",
        // Sidfoten: klockans tid i Stockholm och användarens visningsnamn.
        "Utskrivet 2026-10-18 14:34 av anna@example.se",
        "Sida 1 av 1",
    ] {
        assert!(innehåller(&text, väntat), "saknar {väntat:?} i {text}");
    }
}

#[tokio::test]
async fn verifikatets_underlag_och_kompletteringar_står_i_förteckningen() {
    let db = Databas::tillfällig().await;
    let (anna, företag) = testhjalp::företag(&db).await;
    let kvitto = underlag::ladda_upp(
        &db,
        &klocka(),
        &anna,
        företag,
        "kvitto-mars.pdf",
        "",
        b"%PDF-1.7\nkvitto\n",
    )
    .await
    .unwrap();
    let v = testhjalp::bokför(
        &db,
        &anna,
        företag,
        "2026-03-14",
        &[(1930, 1_250_000, 0), (3001, 0, 1_250_000)],
    )
    .await;
    underlag::komplettera(&db, &klocka(), &anna, företag, v.id, kvitto.id)
        .await
        .unwrap();

    let pdf = utskrift::verifikat(&db, &klocka(), &anna, företag, v.id)
        .await
        .unwrap()
        .expect("verifikatet finns");

    är_pdfa_2b(&pdf.innehåll);
    let text = sidor(&pdf.innehåll).concat();
    for väntat in [
        "kvitto-mars.pdf",
        &kvitto.sha256[..16],
        "Komplettering 2026-10-18 14:34 av anna@example.se",
    ] {
        assert!(innehåller(&text, väntat), "saknar {väntat:?} i {text}");
    }
    assert!(!innehåller(&text, "Inga underlag"), "{text}");
}

#[tokio::test]
async fn samma_tid_och_data_ger_samma_pdf() {
    let db = Databas::tillfällig().await;
    let (anna, företag) = testhjalp::företag(&db).await;
    let v = testhjalp::bokför(
        &db,
        &anna,
        företag,
        "2026-03-14",
        &[(1930, 100, 0), (3001, 0, 100)],
    )
    .await;

    let a = utskrift::verifikat(&db, &klocka(), &anna, företag, v.id).await;
    let b = utskrift::verifikat(&db, &klocka(), &anna, företag, v.id).await;

    assert_eq!(a.unwrap().unwrap(), b.unwrap().unwrap());
}

#[tokio::test]
async fn ett_okänt_verifikat_ger_ingen_pdf() {
    let db = Databas::tillfällig().await;
    let (anna, företag) = testhjalp::företag(&db).await;

    let pdf = utskrift::verifikat(&db, &klocka(), &anna, företag, "999".parse().unwrap())
        .await
        .unwrap();

    assert!(pdf.is_none());
}

#[tokio::test]
async fn kontoplanen_skrivs_ut_med_alla_konton_och_namnbyten() {
    let db = Databas::tillfällig().await;
    let (anna, företag) = testhjalp::företag(&db).await;
    let nr = Kontonummer::new(1930).unwrap();
    kontoplan::byt_namn(&db, &klocka(), &anna, företag, nr, "Bankkonto SEB")
        .await
        .unwrap();

    let pdf = utskrift::kontoplan(&db, &klocka(), &anna, företag)
        .await
        .unwrap();

    assert_eq!(pdf.filnamn, "kontoplan.pdf");
    är_pdfa_2b(&pdf.innehåll);
    let sidor = sidor(&pdf.innehåll);
    let antal = sidor.len();
    assert!(antal > 10, "BAS 2026 har över tusen konton: {antal} sidor");
    let text = sidor.concat();
    for väntat in [
        "Kontoplan",
        "Exempel AB",
        "BAS 2026",
        "1930 Bankkonto SEB",
        "8999 Årets resultat",
        "Inaktiv",
        &format!("Sida {antal} av {antal}"),
    ] {
        assert!(innehåller(&text, väntat), "saknar {väntat:?}");
    }
}

#[tokio::test]
async fn bara_den_med_åtkomst_kan_skriva_ut() {
    let db = Databas::tillfällig().await;
    let (anna, företag) = testhjalp::företag(&db).await;
    let v = testhjalp::bokför(
        &db,
        &anna,
        företag,
        "2026-03-14",
        &[(1930, 100, 0), (3001, 0, 100)],
    )
    .await;
    let bertil = testhjalp::användare(&db, "bertil@example.se").await;

    assert!(matches!(
        utskrift::verifikat(&db, &klocka(), &bertil, företag, v.id).await,
        Err(UtskriftFel::Åtkomst(ÅtkomstFel::IngenÅtkomst))
    ));
    assert!(matches!(
        utskrift::kontoplan(&db, &klocka(), &bertil, företag).await,
        Err(UtskriftFel::Åtkomst(ÅtkomstFel::IngenÅtkomst))
    ));
}

#[tokio::test]
async fn behandlingshistoriken_skrivs_ut_med_urvalet() {
    let db = Databas::tillfällig().await;
    let (anna, företag) = testhjalp::företag(&db).await;
    let nr = Kontonummer::new(1930).unwrap();
    kontoplan::byt_namn(&db, &klocka(), &anna, företag, nr, "Bankkonto SEB")
        .await
        .unwrap();
    testhjalp::bokför(
        &db,
        &anna,
        företag,
        "2026-03-14",
        &[(1930, 1_250_000, 0), (3001, 0, 1_250_000)],
    )
    .await;
    let urval = Urval {
        typ: Some("konto namnbytt".into()),
        från: Some(jiff::civil::date(2026, 10, 1)),
        ..Urval::default()
    };

    let pdf = utskrift::behandlingshistorik(&db, &klocka(), &anna, företag, &urval)
        .await
        .unwrap();

    assert_eq!(pdf.filnamn, "behandlingshistorik_5560360793_20261018.pdf");
    är_pdfa_2b(&pdf.innehåll);
    let text = sidor(&pdf.innehåll).concat();
    for väntat in [
        "Behandlingshistorik",
        "Exempel AB",
        "Urval: typ konto namnbytt · från 2026-10-01",
        "2026-10-18 14:34:00",
        "anna@example.se (webb)",
        "konto namnbytt",
        "1930",
        "Namn ändrat från ”Företagskonto” till ”Bankkonto SEB”",
        "1 post",
        "Utskrivet 2026-10-18 14:34 av anna@example.se",
    ] {
        assert!(innehåller(&text, väntat), "{väntat}: {text}");
    }
    assert!(!innehåller(&text, "verifikat bokfört"), "{text}");
}
