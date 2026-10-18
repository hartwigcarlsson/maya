//! Företagets inställningar: uppgifter, byte av bokföringsmetod och momsperiod, första årets
//! slutdatum och radering, genom `core` mot en tillfällig databas.

use jiff::civil::date;
use maya_core::atkomst::ÅtkomstFel;
use maya_core::foretag::{self, FöretagsFel};
use maya_core::{Databas, Klocka, behandlingshistorik, installationslogg, testhjalp, verifikat};
use maya_domain::foretag::{Bokföringsmetod, Byte, Momsperiod, NyttFöretag, Uppgifter};
use maya_domain::verifikat::{NyRad, NyttVerifikat};

const NU: &str = "2026-10-18T08:00:00Z";

fn klocka() -> Klocka {
    Klocka::fast(NU.parse().unwrap())
}

fn uppgifter() -> Uppgifter {
    Uppgifter {
        namn: "Exempel AB".into(),
        ..Uppgifter::default()
    }
}

#[tokio::test]
async fn ändrade_uppgifter_sparas_och_loggas_med_det_som_ändrades() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;

    let nya = Uppgifter {
        namn: "Exempel Handel AB".into(),
        gatuadress: "Storgatan 1".into(),
        postnummer: "11122".into(),
        postort: "Stockholm".into(),
        epost: "info@exempel.se".into(),
        telefon: "08-123 45 67".into(),
        säte: "Stockholm".into(),
        momsregnr: "se556036079301".into(),
        f_skatt: true,
    };
    foretag::ändra_uppgifter(&db, &klocka(), &anna, id, &nya)
        .await
        .unwrap();

    let företaget = foretag::företaget(&db, &anna, id).await.unwrap();
    assert_eq!(företaget.namn, "Exempel Handel AB");
    let u = &företaget.uppgifter;
    assert_eq!(u.postnummer, "111 22");
    assert_eq!(u.momsregnr, "SE556036079301");
    assert!(u.f_skatt);
    assert_eq!(u.epost, "info@exempel.se");

    let historik = behandlingshistorik::läs(&db, &anna, id).await.unwrap();
    let post = historik.last().unwrap();
    assert_eq!(post.typ, "företag ändrat");
    assert_eq!(post.objekt, "556036-0793");
    assert_eq!(post.aktör, anna);
    assert_eq!(post.tid.to_string(), NU);
    assert_eq!(
        post.beskrivning["namn"],
        serde_json::json!({ "från": "Exempel AB", "till": "Exempel Handel AB" })
    );
    assert_eq!(
        post.beskrivning["gatuadress"],
        serde_json::json!({ "från": "", "till": "Storgatan 1" })
    );
    assert_eq!(
        post.beskrivning["f_skatt"],
        serde_json::json!({ "från": false, "till": true })
    );
}

#[tokio::test]
async fn oförändrade_uppgifter_loggas_inte() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    let före = behandlingshistorik::läs(&db, &anna, id)
        .await
        .unwrap()
        .len();

    foretag::ändra_uppgifter(&db, &klocka(), &anna, id, &uppgifter())
        .await
        .unwrap();

    assert_eq!(
        behandlingshistorik::läs(&db, &anna, id)
            .await
            .unwrap()
            .len(),
        före
    );
}

#[tokio::test]
async fn uppgifter_utan_namn_eller_med_fel_momsregnr_nekas_med_koder() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;

    let fel = foretag::ändra_uppgifter(
        &db,
        &klocka(),
        &anna,
        id,
        &Uppgifter {
            namn: " ".into(),
            postnummer: "1234".into(),
            momsregnr: "SE5560360793".into(),
            ..Uppgifter::default()
        },
    )
    .await
    .unwrap_err();

    let FöretagsFel::Ogiltigt(fel) = fel else {
        panic!("väntade Ogiltigt, fick {fel:?}");
    };
    let koder: Vec<_> = fel.iter().map(|f| f.kod).collect();
    assert_eq!(
        koder,
        ["namn_saknas", "postnummer_ogiltigt", "momsregnr_ogiltigt"]
    );
}

#[tokio::test]
async fn den_utan_åtkomst_kan_inte_ändra_uppgifterna() {
    let db = Databas::tillfällig().await;
    let (_, id) = testhjalp::företag(&db).await;
    let bertil = testhjalp::användare(&db, "bertil@example.se").await;

    let fel = foretag::ändra_uppgifter(&db, &klocka(), &bertil, id, &uppgifter())
        .await
        .unwrap_err();

    assert!(matches!(
        fel,
        FöretagsFel::Åtkomst(ÅtkomstFel::IngenÅtkomst)
    ));
}

fn byte(gäller_från: &str, val: &str) -> Byte {
    Byte {
        gäller_från: gäller_från.into(),
        val: val.into(),
    }
}

fn koder(fel: FöretagsFel) -> Vec<&'static str> {
    match fel {
        FöretagsFel::Ogiltigt(fel) => fel.iter().map(|f| f.kod).collect(),
        fel => panic!("väntade Ogiltigt, fick {fel:?}"),
    }
}

#[tokio::test]
async fn bokföringsmetoden_byts_från_början_av_nästa_räkenskapsår_och_bytet_loggas() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;

    foretag::byt_bokföringsmetod(&db, &klocka(), &anna, id, &byte("2027-01-01", "kontant"))
        .await
        .unwrap();

    let företaget = foretag::företaget(&db, &anna, id).await.unwrap();
    assert_eq!(
        företaget.bokföringsmetoder,
        [
            (date(2026, 1, 1), Bokföringsmetod::Faktura),
            (date(2027, 1, 1), Bokföringsmetod::Kontant)
        ]
    );
    let post = behandlingshistorik::läs(&db, &anna, id)
        .await
        .unwrap()
        .pop()
        .unwrap();
    assert_eq!(post.typ, "bokföringsmetod bytt");
    assert_eq!(post.aktör, anna);
    assert_eq!(
        post.beskrivning,
        serde_json::json!({ "gäller_från": "2027-01-01", "från": "faktura", "till": "kontant" })
    );
}

#[tokio::test]
async fn bokföringsmetoden_byts_inte_mitt_i_ett_år_eller_från_det_första_året() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;

    for (datum, kod) in [
        ("2026-07-01", "inte_arets_start"),
        ("2028-01-01", "inte_arets_start"),
        ("2026-01-01", "fore_senaste_bytet"),
        ("2027-13-01", "datum_ogiltigt"),
    ] {
        let fel = foretag::byt_bokföringsmetod(&db, &klocka(), &anna, id, &byte(datum, "kontant"))
            .await
            .unwrap_err();
        assert_eq!(koder(fel), [kod], "{datum}");
    }
    let fel =
        foretag::byt_bokföringsmetod(&db, &klocka(), &anna, id, &byte("2027-01-01", "faktura"))
            .await
            .unwrap_err();
    assert_eq!(koder(fel), ["samma_som_nu"]);
    assert_eq!(
        foretag::företaget(&db, &anna, id)
            .await
            .unwrap()
            .bokföringsmetoder
            .len(),
        1
    );
}

#[tokio::test]
async fn bokföringsmetoden_byts_inte_från_ett_år_som_redan_har_bokförda_verifikat() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    testhjalp::bokför(
        &db,
        &anna,
        id,
        "2027-02-03",
        &[(1930, 10_000, 0), (3001, 0, 10_000)],
    )
    .await;

    let fel =
        foretag::byt_bokföringsmetod(&db, &klocka(), &anna, id, &byte("2027-01-01", "kontant"))
            .await
            .unwrap_err();

    assert_eq!(koder(fel), ["bokfort_efter"]);
}

#[tokio::test]
async fn momsperioden_byts_från_början_av_en_momsperiod_och_bytet_loggas() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;

    let fel = foretag::byt_momsperiod(&db, &klocka(), &anna, id, &byte("2027-02-01", "manad"))
        .await
        .unwrap_err();
    assert_eq!(koder(fel), ["inte_momsperiodens_start"]);

    foretag::byt_momsperiod(&db, &klocka(), &anna, id, &byte("2027-04-01", "manad"))
        .await
        .unwrap();

    let företaget = foretag::företaget(&db, &anna, id).await.unwrap();
    assert_eq!(
        företaget.momsperioder,
        [
            (date(2026, 1, 1), Momsperiod::Kvartal),
            (date(2027, 4, 1), Momsperiod::Månad)
        ]
    );
    let post = behandlingshistorik::läs(&db, &anna, id)
        .await
        .unwrap()
        .pop()
        .unwrap();
    assert_eq!(post.typ, "momsperiod bytt");
    assert_eq!(
        post.beskrivning,
        serde_json::json!({ "gäller_från": "2027-04-01", "från": "kvartal", "till": "manad" })
    );
}

#[tokio::test]
async fn helårsmoms_byts_bara_från_början_av_ett_räkenskapsår() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;

    let fel = foretag::byt_momsperiod(&db, &klocka(), &anna, id, &byte("2027-04-01", "helar"))
        .await
        .unwrap_err();
    assert_eq!(koder(fel), ["inte_momsperiodens_start"]);

    foretag::byt_momsperiod(&db, &klocka(), &anna, id, &byte("2027-01-01", "helar"))
        .await
        .unwrap();
}

#[tokio::test]
async fn första_räkenskapsårets_slut_kan_flyttas_inom_reglerna_och_ändringen_loggas() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;

    foretag::ändra_första_årets_slut(&db, &klocka(), &anna, id, "2027-06-30")
        .await
        .unwrap();

    let år = foretag::företaget(&db, &anna, id)
        .await
        .unwrap()
        .räkenskapsår;
    assert_eq!(år.len(), 1);
    assert_eq!(
        (år[0].start, år[0].slut),
        (date(2026, 1, 1), date(2027, 6, 30))
    );
    assert_eq!(år[0].perioder.len(), 18);
    assert_eq!(år[0].perioder[17].start, date(2027, 6, 1));
    let post = behandlingshistorik::läs(&db, &anna, id)
        .await
        .unwrap()
        .pop()
        .unwrap();
    assert_eq!(post.typ, "räkenskapsår ändrat");
    assert_eq!(post.objekt, "2026-01-01");
    assert_eq!(
        post.beskrivning,
        serde_json::json!({
            "start": "2026-01-01",
            "slut": { "från": "2026-12-31", "till": "2027-06-30" },
        })
    );
}

#[tokio::test]
async fn första_räkenskapsårets_slut_följer_samma_regler_som_vid_uppläggningen() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;

    for (slut, kod) in [
        ("2027-07-31", "ar_for_langt"),
        ("2026-11-15", "ar_slut_manadsslut"),
        ("2025-12-31", "ar_slut_fore_start"),
        ("31/12", "datum_ogiltigt"),
    ] {
        let fel = foretag::ändra_första_årets_slut(&db, &klocka(), &anna, id, slut)
            .await
            .unwrap_err();
        assert_eq!(koder(fel), [kod], "{slut}");
    }
}

#[tokio::test]
async fn första_räkenskapsåret_kortas_inte_så_att_ett_verifikat_hamnar_utanför() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    testhjalp::bokför(
        &db,
        &anna,
        id,
        "2026-11-10",
        &[(1930, 10_000, 0), (3001, 0, 10_000)],
    )
    .await;

    let fel = foretag::ändra_första_årets_slut(&db, &klocka(), &anna, id, "2026-10-31")
        .await
        .unwrap_err();
    assert_eq!(koder(fel), ["verifikat_efter_slut"]);

    foretag::ändra_första_årets_slut(&db, &klocka(), &anna, id, "2026-11-30")
        .await
        .unwrap();
}

#[tokio::test]
async fn första_räkenskapsårets_slut_ändras_inte_när_ett_senare_år_eller_byte_finns() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    foretag::byt_momsperiod(&db, &klocka(), &anna, id, &byte("2027-01-01", "manad"))
        .await
        .unwrap();

    let fel = foretag::ändra_första_årets_slut(&db, &klocka(), &anna, id, "2027-03-31")
        .await
        .unwrap_err();
    assert_eq!(koder(fel), ["byte_efter_aret"]);

    testhjalp::bokför(
        &db,
        &anna,
        id,
        "2027-01-10",
        &[(1930, 10_000, 0), (3001, 0, 10_000)],
    )
    .await;
    let fel = foretag::ändra_första_årets_slut(&db, &klocka(), &anna, id, "2027-03-31")
        .await
        .unwrap_err();
    assert_eq!(koder(fel), ["senare_ar_finns"]);
}

#[tokio::test]
async fn ett_företag_utan_räkenskapsinformation_raderas_och_raderingen_står_i_installationsloggen()
{
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    foretag::ändra_uppgifter(
        &db,
        &klocka(),
        &anna,
        id,
        &Uppgifter {
            gatuadress: "Storgatan 1".into(),
            ..uppgifter()
        },
    )
    .await
    .unwrap();
    foretag::ändra_första_årets_slut(&db, &klocka(), &anna, id, "2027-03-31")
        .await
        .unwrap();

    foretag::radera(&db, &klocka(), &anna, id).await.unwrap();

    assert!(foretag::mina_företag(&db, &anna).await.unwrap().is_empty());
    assert!(matches!(
        foretag::företaget(&db, &anna, id).await,
        Err(ÅtkomstFel::IngenÅtkomst)
    ));
    let post = installationslogg::läs(&db).await.unwrap().pop().unwrap();
    assert_eq!(post.typ, "företag raderat");
    assert_eq!(post.objekt, "556036-0793");
    assert_eq!(post.aktör, anna);
    assert_eq!(post.tid.to_string(), NU);
    assert_eq!(
        post.beskrivning,
        serde_json::json!({ "orgnr": "556036-0793", "namn": "Exempel AB" })
    );

    // Organisationsnumret är ledigt igen.
    let igen = NyttFöretag {
        orgnr: "556036-0793".into(),
        namn: "Exempel AB".into(),
        första_år_start: "2026-01-01".into(),
        första_år_slut: "2026-12-31".into(),
        bokföringsmetod: "faktura".into(),
        momsperiod: "kvartal".into(),
    };
    foretag::lägg_upp(&db, &klocka(), &anna, &igen)
        .await
        .unwrap();
}

#[tokio::test]
async fn radering_nekas_när_företaget_har_bokförda_verifikat() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    testhjalp::bokför(
        &db,
        &anna,
        id,
        "2026-03-14",
        &[(1930, 10_000, 0), (3001, 0, 10_000)],
    )
    .await;
    let loggen = installationslogg::läs(&db).await.unwrap().len();

    let fel = foretag::radera(&db, &klocka(), &anna, id)
        .await
        .unwrap_err();

    assert!(
        matches!(fel, FöretagsFel::HarRäkenskapsinformation),
        "{fel:?}"
    );
    assert_eq!(foretag::mina_företag(&db, &anna).await.unwrap().len(), 1);
    assert_eq!(installationslogg::läs(&db).await.unwrap().len(), loggen);
}

#[tokio::test]
async fn den_utan_åtkomst_kan_inte_radera_företaget() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    let bertil = testhjalp::användare(&db, "bertil@example.se").await;

    let fel = foretag::radera(&db, &klocka(), &bertil, id)
        .await
        .unwrap_err();

    assert!(matches!(
        fel,
        FöretagsFel::Åtkomst(ÅtkomstFel::IngenÅtkomst)
    ));
    assert_eq!(foretag::mina_företag(&db, &anna).await.unwrap().len(), 1);
}

#[tokio::test]
async fn ett_företag_med_bara_utkast_raderas_med_utkasten() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    let utkast = NyttVerifikat {
        datum: "2026-11-30".into(),
        beskrivning: "Halvfärdigt".into(),
        rader: vec![NyRad {
            konto: "1930".into(),
            debet: "100".into(),
            ..NyRad::default()
        }],
    };
    verifikat::spara_utkast(&db, &klocka(), &anna, id, None, &utkast)
        .await
        .unwrap();

    foretag::radera(&db, &klocka(), &anna, id).await.unwrap();

    assert!(foretag::mina_företag(&db, &anna).await.unwrap().is_empty());
}
