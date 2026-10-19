//! Betalning av leverantörsfaktura genom `core` mot en tillfällig databas (#147): 2440 mot
//! betalkontot i samma transaktion som betalningen, delbetalning, öresutjämning mot 3740,
//! kursdifferens mot 3960 och 7960 på en faktura med originalvaluta, Rätta betalning och det som
//! återstår i listan.

use jiff::civil::date;
use maya_core::betalning::{self, BetalningFel, Faktura};
use maya_core::leverantorsfaktura::{self, LeverantörsfakturaId};
use maya_core::verifikat::{self, Flöde, Rättelseval, VerifikatFel, VerifikatId};
use maya_core::{Databas, Klocka, behandlingshistorik, testhjalp};
use maya_core::{aktor::Aktör, foretag::FöretagsId};
use maya_domain::betalning::NyBetalning;

const NU: &str = "2026-10-19T08:00:00Z";

fn klocka() -> Klocka {
    Klocka::fast(NU.parse().unwrap())
}

/// Fakturans total: 1 000 kr netto och 25 % moms.
const TOTAL: i64 = 125_000;

/// Ett företag, leverantören Kontorsvaror AB och dess registrerade faktura 8812, i
/// originalvaluta om `originalvaluta` inte är tom.
async fn faktura(
    db: &Databas,
    originalvaluta: &str,
) -> (Aktör, FöretagsId, LeverantörsfakturaId) {
    let (anna, id) = testhjalp::företag(db).await;
    let l = testhjalp::leverantör(db, &anna, id, "Kontorsvaror AB").await;
    let f =
        testhjalp::registrerad_leverantörsfaktura(db, &anna, id, l, "8812", originalvaluta).await;
    (anna, id, f)
}

fn betalning(betaldag: &str, belopp: &str) -> NyBetalning {
    NyBetalning {
        betaldag: betaldag.into(),
        belopp: belopp.into(),
        konto: "1930".into(),
        ..Default::default()
    }
}

async fn rader(
    db: &Databas, anna: &Aktör, id: FöretagsId, v: VerifikatId
) -> Vec<(u16, i64, i64)> {
    verifikat::verifikatet(db, anna, id, v)
        .await
        .unwrap()
        .unwrap()
        .rader
        .iter()
        .map(|r| (r.konto.unwrap().get(), r.debet.öre(), r.kredit.öre()))
        .collect()
}

async fn återstår_i_listan(
    db: &Databas,
    anna: &Aktör,
    id: FöretagsId,
    f: LeverantörsfakturaId,
) -> i64 {
    leverantorsfaktura::lista(db, anna, id)
        .await
        .unwrap()
        .into_iter()
        .find(|r| r.id == f)
        .unwrap()
        .återstår
}

fn spärrkoder(fel: &BetalningFel) -> Vec<&'static str> {
    match fel {
        BetalningFel::Spärrat(s) => s.iter().map(|s| s.kod).collect(),
        fel => panic!("väntade spärrar, fick {fel:?}"),
    }
}

#[tokio::test]
async fn betalningen_bokför_2440_mot_betalkontot_daterad_på_betaldagen_med_fakturan_som_referens() {
    let db = Databas::tillfällig().await;
    let (anna, id, f) = faktura(&db, "").await;
    assert_eq!(återstår_i_listan(&db, &anna, id, f).await, TOTAL);
    let r = betalning::registrera(
        &db,
        &klocka(),
        &anna,
        id,
        Faktura::Leverantör(f),
        &betalning("2026-04-10", "1 250,00"),
    )
    .await
    .unwrap();
    assert_eq!(r.återstår, 0);
    let v = verifikat::verifikatet(&db, &anna, id, r.verifikat.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(v.datum, Some(date(2026, 4, 10)));
    assert_eq!(
        v.beskrivning,
        "Betalning av leverantörsfaktura 8812, Kontorsvaror AB"
    );
    assert_eq!(
        rader(&db, &anna, id, r.verifikat.id).await,
        [(2440, TOTAL, 0), (1930, 0, TOTAL)]
    );
    // Verifikatet länkar till fakturan och rättas där.
    let flöde = v.flöde.clone().expect("länken till fakturan");
    assert_eq!(flöde.flöde, Flöde::Leverantörsfaktura);
    assert_eq!(flöde.id.to_string(), f.to_string());
    assert!(flöde.text.contains("8812"), "{}", flöde.text);
    let fel = verifikat::rätta(
        &db,
        &klocka(),
        &anna,
        id,
        r.verifikat.id,
        "2026-10-19",
        "",
        Rättelseval::BaraRättelse,
    )
    .await
    .unwrap_err();
    assert!(
        matches!(&fel, VerifikatFel::InteRättbart(s) if s.kod() == "fran_flode"),
        "{fel:?}"
    );

    assert_eq!(återstår_i_listan(&db, &anna, id, f).await, 0);
    let post = betalning::reskontrapost(&db, &anna, id, Faktura::Leverantör(f))
        .await
        .unwrap();
    assert_eq!((post.total, post.återstår), (TOTAL, 0));
    assert_eq!(post.förfallodag, date(2026, 4, 13));
    assert_eq!(post.betalningar.len(), 1);

    let historik = behandlingshistorik::läs(&db, &anna, id).await.unwrap();
    let registrerad = historik
        .iter()
        .find(|p| p.typ == "betalning registrerad")
        .expect("betalningen loggas");
    assert_eq!(registrerad.belopp().unwrap().öre(), TOTAL);
    assert_eq!(
        registrerad.beskrivning["leverantörsfaktura"],
        f.to_string().parse::<i64>().unwrap()
    );
    assert!(
        registrerad.text().contains("Kontorsvaror AB"),
        "{}",
        registrerad.text()
    );
    // Betalningen syns i fakturans historik.
    let faktura = leverantorsfaktura::fakturan(&db, &anna, id, f)
        .await
        .unwrap()
        .unwrap();
    assert!(
        faktura
            .historik
            .iter()
            .any(|h| h.typ == "betalning registrerad")
    );
}

#[tokio::test]
async fn delbetalning_och_öresutjämning_mot_3740_som_för_kundfakturor() {
    let db = Databas::tillfällig().await;
    let (anna, id, f) = faktura(&db, "").await;
    let del = betalning::registrera(
        &db,
        &klocka(),
        &anna,
        id,
        Faktura::Leverantör(f),
        &betalning("2026-04-01", "500"),
    )
    .await
    .unwrap();
    assert_eq!(del.återstår, 75_000);
    assert_eq!(återstår_i_listan(&db, &anna, id, f).await, 75_000);
    let rest = betalning::registrera(
        &db,
        &klocka(),
        &anna,
        id,
        Faktura::Leverantör(f),
        &NyBetalning {
            öresutjämning: true,
            ..betalning("2026-04-10", "745")
        },
    )
    .await
    .unwrap();
    assert_eq!(rest.återstår, 0);
    assert_eq!(
        rader(&db, &anna, id, rest.verifikat.id).await,
        [(2440, 75_000, 0), (1930, 0, 74_500), (3740, 0, 500)]
    );
    // Utan kursdifferens spärras en överbetalning också på en leverantörsfaktura.
    let annan = testhjalp::leverantör(&db, &anna, id, "Annan AB").await;
    let ny = testhjalp::registrerad_leverantörsfaktura(&db, &anna, id, annan, "1", "").await;
    let fel = betalning::registrera(
        &db,
        &klocka(),
        &anna,
        id,
        Faktura::Leverantör(ny),
        &betalning("2026-04-10", "1 250,01"),
    )
    .await
    .unwrap_err();
    assert_eq!(spärrkoder(&fel), ["overbetalning"]);
}

#[tokio::test]
async fn en_avvikelse_i_originalvaluta_stängs_mot_kursförlust_eller_kursvinst_i_samma_verifikat() {
    let db = Databas::tillfällig().await;
    let (anna, id, f) = faktura(&db, "EUR 109,50").await;
    let förlust = betalning::registrera(
        &db,
        &klocka(),
        &anna,
        id,
        Faktura::Leverantör(f),
        &NyBetalning {
            kursdifferens: true,
            ..betalning("2026-04-10", "1 300")
        },
    )
    .await
    .unwrap();
    assert_eq!(förlust.återstår, 0);
    assert_eq!(
        rader(&db, &anna, id, förlust.verifikat.id).await,
        [(2440, TOTAL, 0), (1930, 0, 130_000), (7960, 5_000, 0)]
    );
    assert_eq!(återstår_i_listan(&db, &anna, id, f).await, 0);
    let post = betalning::reskontrapost(&db, &anna, id, Faktura::Leverantör(f))
        .await
        .unwrap();
    assert_eq!(post.betalningar[0].kursdifferens, -5_000);
    let historik = behandlingshistorik::läs(&db, &anna, id).await.unwrap();
    let text = historik
        .iter()
        .find(|p| p.typ == "betalning registrerad")
        .unwrap()
        .text();
    assert!(text.contains("50,00 kr kursförlust"), "{text}");

    let l = testhjalp::leverantör(&db, &anna, id, "Utlandet GmbH").await;
    let andra =
        testhjalp::registrerad_leverantörsfaktura(&db, &anna, id, l, "77", "EUR 109,50").await;
    let vinst = betalning::registrera(
        &db,
        &klocka(),
        &anna,
        id,
        Faktura::Leverantör(andra),
        &NyBetalning {
            kursdifferens: true,
            ..betalning("2026-04-10", "1 200")
        },
    )
    .await
    .unwrap();
    assert_eq!(
        rader(&db, &anna, id, vinst.verifikat.id).await,
        [(2440, TOTAL, 0), (1930, 0, 120_000), (3960, 0, 5_000)]
    );
    assert_eq!(återstår_i_listan(&db, &anna, id, andra).await, 0);
}

#[tokio::test]
async fn kursdifferens_spärras_på_en_faktura_utan_originalvaluta() {
    let db = Databas::tillfällig().await;
    let (anna, id, f) = faktura(&db, "").await;
    let fel = betalning::registrera(
        &db,
        &klocka(),
        &anna,
        id,
        Faktura::Leverantör(f),
        &NyBetalning {
            kursdifferens: true,
            ..betalning("2026-04-10", "1 200")
        },
    )
    .await
    .unwrap_err();
    assert_eq!(spärrkoder(&fel), ["kursdifferens_utan_originalvaluta"]);
    assert!(
        !betalning::förutsättningar(&db, &anna, id, Faktura::Leverantör(f))
            .await
            .unwrap()
            .originalvaluta
    );
}

#[tokio::test]
async fn rätta_betalning_vänder_verifikatet_med_kursdifferensen_och_återställer_skulden() {
    let db = Databas::tillfällig().await;
    let (anna, id, f) = faktura(&db, "EUR 109,50").await;
    let r = betalning::registrera(
        &db,
        &klocka(),
        &anna,
        id,
        Faktura::Leverantör(f),
        &NyBetalning {
            kursdifferens: true,
            ..betalning("2026-04-10", "1 300")
        },
    )
    .await
    .unwrap();
    let rättad = betalning::rätta(&db, &klocka(), &anna, id, r.id, "2026-10-19", "fel kurs")
        .await
        .unwrap();
    assert_eq!(rättad.återstår, TOTAL);
    assert_eq!(
        rader(&db, &anna, id, rättad.rättelse.id).await,
        [(2440, 0, TOTAL), (1930, 130_000, 0), (7960, 0, 5_000)]
    );
    assert_eq!(återstår_i_listan(&db, &anna, id, f).await, TOTAL);
    let v = verifikat::verifikatet(&db, &anna, id, rättad.rättelse.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(v.flöde.unwrap().flöde, Flöde::Leverantörsfaktura);
    // Betalningen kan registreras om.
    let om = betalning::registrera(
        &db,
        &klocka(),
        &anna,
        id,
        Faktura::Leverantör(f),
        &betalning("2026-04-10", "1 250"),
    )
    .await
    .unwrap();
    assert_eq!(om.återstår, 0);
    let historik = behandlingshistorik::läs(&db, &anna, id).await.unwrap();
    let post = historik
        .iter()
        .find(|p| p.typ == "betalning rättad")
        .expect("rättelsen loggas");
    assert_eq!(
        post.beskrivning["leverantörsfaktura"],
        f.to_string().parse::<i64>().unwrap()
    );
}

#[tokio::test]
async fn ett_utkast_tar_inte_emot_betalningar() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    let (utkast, _) = leverantorsfaktura::ladda_upp(
        &db,
        &klocka(),
        &anna,
        id,
        ("f.pdf", "application/pdf", b"%PDF-1.4 utkast"),
    )
    .await
    .unwrap();
    let fel = betalning::registrera(
        &db,
        &klocka(),
        &anna,
        id,
        Faktura::Leverantör(utkast),
        &betalning("2026-04-10", "100"),
    )
    .await
    .unwrap_err();
    assert!(matches!(fel, BetalningFel::FinnsInte), "{fel:?}");
    assert_eq!(återstår_i_listan(&db, &anna, id, utkast).await, 0);
}

#[tokio::test]
async fn databasen_avvisar_kursdifferens_utan_originalvaluta() {
    let db = Databas::tillfällig().await;
    let (anna, id, f) = faktura(&db, "").await;
    betalning::registrera(
        &db,
        &klocka(),
        &anna,
        id,
        Faktura::Leverantör(f),
        &betalning("2026-04-10", "100"),
    )
    .await
    .unwrap();
    // Registreringens verifikat är bokfört och inte en betalnings, så bara kursdifferensen
    // hindrar raden.
    let fel = testhjalp::kör_sql(
        &db,
        "INSERT INTO betalning (foretag_id, leverantorsfaktura_id, betaldag, belopp, konto,
                                kursdifferens, verifikat_id, registrerad, registrerad_kanal)
         SELECT b.foretag_id, b.leverantorsfaktura_id, b.betaldag, 5, b.konto, 5,
                f.verifikat_id, b.registrerad, b.registrerad_kanal
         FROM betalning b JOIN leverantorsfaktura f ON f.id = b.leverantorsfaktura_id",
    )
    .await
    .unwrap_err();
    assert!(fel.to_string().contains("originalvaluta"), "{fel}");
}
