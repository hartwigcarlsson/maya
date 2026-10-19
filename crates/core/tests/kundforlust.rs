//! Kundförlust och återkallelse genom `core` mot en tillfällig databas (#141): förlustens
//! verifikat enligt tabellen över automatkonteringar med momsreduceringen fördelad
//! proportionellt, i samma transaktion som förlusten; spärrarna för skäl och datum;
//! återkallelsen som vänder verifikatet och gör fakturan betalbar igen; märket, historiken och
//! triggrarna.

use jiff::civil::date;
use maya_core::atkomst::ÅtkomstFel;
use maya_core::betalning::{self, Faktura};
use maya_core::kundfaktura::{self, Utfärdad};
use maya_core::kundforlust::{self, KundförlustFel};
use maya_core::verifikat::{self, Flöde, Rättelseval, VerifikatFel, VerifikatId};
use maya_core::{Databas, Klocka, behandlingshistorik, testhjalp};
use maya_core::{aktor::Aktör, foretag::FöretagsId};
use maya_domain::betalning::NyBetalning;
use maya_domain::kundforlust::{NyKundförlust, Skäl};

const NU: &str = "2026-10-18T08:00:00Z";

fn klocka() -> Klocka {
    Klocka::fast(NU.parse().unwrap())
}

/// 10 timmar à 950 kr med 25 % moms: 11 875,00 kr, fakturerad 2026-03-10.
const TOTAL: i64 = 1_187_500;

async fn faktura(db: &Databas) -> (Aktör, FöretagsId, Utfärdad) {
    let (anna, id) = testhjalp::fakturaföretag(db).await;
    let f = testhjalp::utfärdad_kundfaktura(db, &anna, id, "Kund AB", "2026-03-10").await;
    (anna, id, f)
}

fn förlust(skäl: &str, datum: &str) -> NyKundförlust {
    NyKundförlust {
        skäl: skäl.into(),
        annat: String::new(),
        datum: datum.into(),
    }
}

async fn betala(db: &Databas, anna: &Aktör, id: FöretagsId, f: &Utfärdad, belopp: &str) {
    betalning::registrera(
        db,
        &klocka(),
        anna,
        id,
        Faktura::Kund(f.id),
        &NyBetalning {
            betaldag: "2026-04-09".into(),
            belopp: belopp.into(),
            konto: "1930".into(),
            öresutjämning: false,
        },
    )
    .await
    .unwrap();
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

async fn återstår(db: &Databas, anna: &Aktör, id: FöretagsId, f: &Utfärdad) -> i64 {
    betalning::reskontrapost(db, anna, id, Faktura::Kund(f.id))
        .await
        .unwrap()
        .återstår
}

#[tokio::test]
async fn kundförlusten_bokför_6351_och_momsreduceringen_mot_1510_på_det_som_återstår() {
    let db = Databas::tillfällig().await;
    let (anna, id, f) = faktura(&db).await;
    betala(&db, &anna, id, &f, "1 875,00").await;

    let b = kundforlust::bokför(
        &db,
        &klocka(),
        &anna,
        id,
        f.id,
        &förlust("konkurs", "2026-09-30"),
    )
    .await
    .unwrap();
    let v = b.verifikat.expect("fordran är bokförd");
    // 10 000 kr återstår: 8 000 netto och 2 000 moms, i proportion till 9 500 och 2 375.
    assert_eq!(
        rader(&db, &anna, id, v.id).await,
        [(6351, 800_000, 0), (2611, 200_000, 0), (1510, 0, 1_000_000)]
    );
    let verifikatet = verifikat::verifikatet(&db, &anna, id, v.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(verifikatet.datum, Some(date(2026, 9, 30)));
    assert_eq!(
        verifikatet.beskrivning,
        format!("Kundförlust, kundfaktura {}, Kund AB", f.nummer)
    );
    // Verifikatet länkar till fakturan och kan inte rättas för sig.
    let flöde = verifikatet.flöde.clone().expect("länken till fakturan");
    assert_eq!(flöde.flöde, Flöde::Kundfaktura);
    assert_eq!(flöde.id.to_string(), f.id.to_string());
    let fel = verifikat::rätta(
        &db,
        &klocka(),
        &anna,
        id,
        v.id,
        "2026-10-18",
        "",
        Rättelseval::BaraRättelse,
    )
    .await
    .unwrap_err();
    assert!(
        matches!(&fel, VerifikatFel::InteRättbart(s) if s.kod() == "fran_flode"),
        "{fel:?}"
    );

    // Inget återstår, märket är kundförlust och fakturan tar inte emot fler betalningar.
    let post = betalning::reskontrapost(&db, &anna, id, Faktura::Kund(f.id))
        .await
        .unwrap();
    assert_eq!(post.återstår, 0);
    assert!(post.kundförlust);
    let listan = kundfaktura::lista(&db, &anna, id).await.unwrap();
    let rad = listan.iter().find(|r| r.id == f.id).unwrap();
    assert_eq!(rad.återstår, 0);
    assert!(rad.kundförlust);
    let förlusterna = kundforlust::kundförluster(&db, &anna, id, f.id)
        .await
        .unwrap();
    assert_eq!(förlusterna.len(), 1);
    assert_eq!(förlusterna[0].belopp, 1_000_000);
    assert_eq!(förlusterna[0].skäl, Skäl::Konkurs);
    assert_eq!(förlusterna[0].skälet, "Konkurs");
    assert_eq!(förlusterna[0].datum, date(2026, 9, 30));
    assert_eq!(förlusterna[0].verifikat.as_ref().map(|l| l.id), Some(v.id));
    assert!(förlusterna[0].återkallad.is_none());

    let historik = behandlingshistorik::läs(&db, &anna, id).await.unwrap();
    let bokförd = historik
        .iter()
        .find(|p| p.typ == "kundförlust bokförd")
        .expect("förlusten loggas");
    assert_eq!(bokförd.belopp().unwrap().öre(), 1_000_000);
    assert_eq!(
        bokförd.beskrivning["kundfaktura"],
        f.id.to_string().parse::<i64>().unwrap()
    );
    assert_eq!(bokförd.beskrivning["skäl"], "konkurs");
    assert!(bokförd.text().contains("10 000,00"), "{}", bokförd.text());
    assert!(bokförd.text().contains("Konkurs"), "{}", bokförd.text());
}

#[tokio::test]
async fn skäl_och_datum_krävs_och_inget_bokförs_utan_dem() {
    let db = Databas::tillfällig().await;
    let (anna, id, f) = faktura(&db).await;
    let fel = kundforlust::bokför(&db, &klocka(), &anna, id, f.id, &förlust("", ""))
        .await
        .unwrap_err();
    let KundförlustFel::Spärrat(spärrar) = fel else {
        panic!("{fel:?}");
    };
    let koder: Vec<_> = spärrar.iter().map(|s| s.kod).collect();
    assert_eq!(koder, ["skal_saknas", "datum_saknas"]);
    let mut annat = förlust("annat", "2026-09-30");
    let fel = kundforlust::bokför(&db, &klocka(), &anna, id, f.id, &annat)
        .await
        .unwrap_err();
    assert!(
        matches!(&fel, KundförlustFel::Spärrat(s) if s[0].kod == "annat_saknas"),
        "{fel:?}"
    );
    assert_eq!(återstår(&db, &anna, id, &f).await, TOTAL);

    annat.annat = "Dödsboet saknar tillgångar".into();
    kundforlust::bokför(&db, &klocka(), &anna, id, f.id, &annat)
        .await
        .unwrap();
    let förlusterna = kundforlust::kundförluster(&db, &anna, id, f.id)
        .await
        .unwrap();
    assert_eq!(förlusterna[0].skälet, "Annat: Dödsboet saknar tillgångar");
    // En gällande förlust spärrar en till.
    let fel = kundforlust::bokför(
        &db,
        &klocka(),
        &anna,
        id,
        f.id,
        &förlust("konkurs", "2026-09-30"),
    )
    .await
    .unwrap_err();
    assert!(
        matches!(&fel, KundförlustFel::Spärrat(s) if s[0].kod == "inget_aterstar"),
        "{fel:?}"
    );
}

#[tokio::test]
async fn återkallelsen_vänder_förlustens_verifikat_och_fakturan_kan_betalas_igen() {
    let db = Databas::tillfällig().await;
    let (anna, id, f) = faktura(&db).await;
    let b = kundforlust::bokför(
        &db,
        &klocka(),
        &anna,
        id,
        f.id,
        &förlust("utmatning", "2026-09-30"),
    )
    .await
    .unwrap();
    let förlustens = b.verifikat.unwrap();
    assert_eq!(
        rader(&db, &anna, id, förlustens.id).await,
        [(6351, 950_000, 0), (2611, 237_500, 0), (1510, 0, TOTAL)]
    );

    let å = kundforlust::återkalla(&db, &klocka(), &anna, id, b.id)
        .await
        .unwrap();
    assert_eq!(å.återstår, TOTAL);
    let v = å.verifikat.expect("ett nytt verifikat");
    assert_ne!(v.id, förlustens.id);
    assert_eq!(
        rader(&db, &anna, id, v.id).await,
        [(6351, 0, 950_000), (2611, 0, 237_500), (1510, TOTAL, 0)]
    );
    let verifikatet = verifikat::verifikatet(&db, &anna, id, v.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(verifikatet.datum, Some(date(2026, 10, 18)));
    assert_eq!(
        verifikatet.beskrivning,
        format!("Återkallad kundförlust, kundfaktura {}, Kund AB", f.nummer)
    );
    assert_eq!(verifikatet.flöde.map(|l| l.flöde), Some(Flöde::Kundfaktura));

    let post = betalning::reskontrapost(&db, &anna, id, Faktura::Kund(f.id))
        .await
        .unwrap();
    assert_eq!(post.återstår, TOTAL);
    assert!(!post.kundförlust);
    let förlusterna = kundforlust::kundförluster(&db, &anna, id, f.id)
        .await
        .unwrap();
    let återkallad = förlusterna[0].återkallad.as_ref().expect("återkallad");
    assert_eq!(återkallad.verifikat.as_ref().map(|l| l.id), Some(v.id));
    // En sen betalning tas emot.
    betala(&db, &anna, id, &f, "11 875,00").await;
    assert_eq!(återstår(&db, &anna, id, &f).await, 0);

    // En förlust återkallas en gång.
    let fel = kundforlust::återkalla(&db, &klocka(), &anna, id, b.id)
        .await
        .unwrap_err();
    assert!(matches!(fel, KundförlustFel::RedanÅterkallad), "{fel:?}");

    let historik = behandlingshistorik::läs(&db, &anna, id).await.unwrap();
    let post = historik
        .iter()
        .find(|p| p.typ == "kundförlust återkallad")
        .expect("återkallelsen loggas");
    assert_eq!(post.belopp().unwrap().öre(), TOTAL);
    assert_eq!(
        post.beskrivning["kundförlust"],
        b.id.to_string().parse::<i64>().unwrap()
    );
    assert!(post.text().contains("11 875,00"), "{}", post.text());
}

#[tokio::test]
async fn efter_en_återkallelse_kan_en_ny_förlust_bokföras() {
    let db = Databas::tillfällig().await;
    let (anna, id, f) = faktura(&db).await;
    let b = kundforlust::bokför(
        &db,
        &klocka(),
        &anna,
        id,
        f.id,
        &förlust("ackord", "2026-09-30"),
    )
    .await
    .unwrap();
    kundforlust::återkalla(&db, &klocka(), &anna, id, b.id)
        .await
        .unwrap();
    betala(&db, &anna, id, &f, "1 875,00").await;
    let ny = kundforlust::bokför(
        &db,
        &klocka(),
        &anna,
        id,
        f.id,
        &förlust("konkurs", "2026-10-18"),
    )
    .await
    .unwrap();
    assert_eq!(
        rader(&db, &anna, id, ny.verifikat.unwrap().id).await,
        [(6351, 800_000, 0), (2611, 200_000, 0), (1510, 0, 1_000_000)]
    );
    assert_eq!(
        kundforlust::kundförluster(&db, &anna, id, f.id)
            .await
            .unwrap()
            .len(),
        2
    );
}

#[tokio::test]
async fn förlusten_bygger_på_det_gällande_utfärdandet_efter_rätta_kontering() {
    let db = Databas::tillfällig().await;
    let (anna, id, f) = faktura(&db).await;
    maya_core::kontering::rätta(
        &db,
        &klocka(),
        &anna,
        id,
        f.id,
        &maya_domain::kontering::NyKontering {
            konton: vec!["3002".into()],
            orsak: String::new(),
        },
    )
    .await
    .unwrap();
    let b = kundforlust::bokför(
        &db,
        &klocka(),
        &anna,
        id,
        f.id,
        &förlust("konkurs", "2026-09-30"),
    )
    .await
    .unwrap();
    assert_eq!(
        rader(&db, &anna, id, b.verifikat.unwrap().id).await,
        [(6351, 950_000, 0), (2611, 237_500, 0), (1510, 0, TOTAL)]
    );
}

#[tokio::test]
async fn ett_datum_före_fakturadatumet_spärras() {
    let db = Databas::tillfällig().await;
    let (anna, id, f) = faktura(&db).await;
    let fel = kundforlust::bokför(
        &db,
        &klocka(),
        &anna,
        id,
        f.id,
        &förlust("konkurs", "2026-03-09"),
    )
    .await
    .unwrap_err();
    assert!(
        matches!(&fel, KundförlustFel::Spärrat(s) if s[0].kod == "fore_fakturadatum"),
        "{fel:?}"
    );
}

#[tokio::test]
async fn ett_utkast_och_en_okänd_faktura_kan_inte_skrivas_av() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::fakturaföretag(&db).await;
    let kund = testhjalp::kund(&db, &anna, id, "Kund AB").await;
    let utkast = testhjalp::kundfakturautkast(&db, &anna, id, kund, "2026-03-10").await;
    let fel = kundforlust::bokför(
        &db,
        &klocka(),
        &anna,
        id,
        utkast,
        &förlust("konkurs", "2026-09-30"),
    )
    .await
    .unwrap_err();
    assert!(matches!(fel, KundförlustFel::FinnsInte), "{fel:?}");
}

#[tokio::test]
async fn den_som_saknar_åtkomst_får_inte_bokföra_eller_återkalla_en_kundförlust() {
    let db = Databas::tillfällig().await;
    let (anna, id, f) = faktura(&db).await;
    let bo = testhjalp::användare(&db, "bo@example.se").await;
    let fel = kundforlust::bokför(
        &db,
        &klocka(),
        &bo,
        id,
        f.id,
        &förlust("konkurs", "2026-09-30"),
    )
    .await
    .unwrap_err();
    assert!(
        matches!(fel, KundförlustFel::Åtkomst(ÅtkomstFel::IngenÅtkomst)),
        "{fel:?}"
    );
    let b = kundforlust::bokför(
        &db,
        &klocka(),
        &anna,
        id,
        f.id,
        &förlust("konkurs", "2026-09-30"),
    )
    .await
    .unwrap();
    let fel = kundforlust::återkalla(&db, &klocka(), &bo, id, b.id)
        .await
        .unwrap_err();
    assert!(
        matches!(fel, KundförlustFel::Åtkomst(ÅtkomstFel::IngenÅtkomst)),
        "{fel:?}"
    );
    // Bara webben bokför kundförluster.
    let fel = kundforlust::återkalla(&db, &klocka(), &Aktör::cli(), id, b.id)
        .await
        .unwrap_err();
    assert!(matches!(fel, KundförlustFel::Åtkomst(_)), "{fel:?}");
    assert!(
        kundforlust::kundförluster(&db, &bo, id, f.id)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn triggrarna_avvisar_att_en_kundförlust_eller_dess_återkallelse_ändras_eller_raderas() {
    let db = Databas::tillfällig().await;
    let (anna, id, f) = faktura(&db).await;
    let b = kundforlust::bokför(
        &db,
        &klocka(),
        &anna,
        id,
        f.id,
        &förlust("konkurs", "2026-09-30"),
    )
    .await
    .unwrap();
    for sql in [
        "UPDATE kundforlust SET belopp = 1",
        "UPDATE kundforlust SET skal = 'ackord'",
    ] {
        let fel = testhjalp::kör_sql(&db, sql).await.unwrap_err();
        assert!(fel.to_string().contains("går inte att ändra"), "{fel}");
    }
    let fel = testhjalp::kör_sql(&db, "DELETE FROM kundforlust")
        .await
        .unwrap_err();
    assert!(fel.to_string().contains("går inte att radera"), "{fel}");
    // En andra förlust på samma faktura, eller en förlust utan bokfört verifikat, avvisas.
    let fel = testhjalp::kör_sql(
        &db,
        "INSERT INTO kundforlust (foretag_id, kundfaktura_id, datum, skal, belopp, bokford,
                                  bokford_kanal)
         SELECT foretag_id, kundfaktura_id, datum, skal, 5, bokford, bokford_kanal
         FROM kundforlust",
    )
    .await
    .unwrap_err();
    assert!(fel.to_string().contains("gäller redan"), "{fel}");
    testhjalp::utfärdad_kundfaktura(&db, &anna, id, "Kund 2 AB", "2026-03-10").await;
    let fel = testhjalp::kör_sql(
        &db,
        "INSERT INTO kundforlust (foretag_id, kundfaktura_id, datum, skal, belopp, verifikat_id,
                                  bokford, bokford_kanal)
         SELECT foretag_id, (SELECT max(id) FROM kundfaktura), datum, skal, 5,
                (SELECT max(id) FROM verifikat) + 1, bokford, bokford_kanal FROM kundforlust",
    )
    .await
    .unwrap_err();
    assert!(fel.to_string().contains("kräver en utfärdad"), "{fel}");
    // Återkallelsen kräver ett annat bokfört verifikat än förlustens.
    let fel = testhjalp::kör_sql(
        &db,
        "INSERT INTO kundforlust_aterkallelse (kundforlust_id, verifikat_id, aterkallad,
                                               aterkallad_kanal)
         SELECT id, verifikat_id, bokford, bokford_kanal FROM kundforlust",
    )
    .await
    .unwrap_err();
    assert!(
        fel.to_string().contains("kräver ett bokfört verifikat"),
        "{fel}"
    );

    kundforlust::återkalla(&db, &klocka(), &anna, id, b.id)
        .await
        .unwrap();
    let fel = testhjalp::kör_sql(&db, "UPDATE kundforlust_aterkallelse SET aterkallad = 'x'")
        .await
        .unwrap_err();
    assert!(fel.to_string().contains("går inte att ändra"), "{fel}");
    let fel = testhjalp::kör_sql(&db, "DELETE FROM kundforlust_aterkallelse")
        .await
        .unwrap_err();
    assert!(fel.to_string().contains("går inte att ändra"), "{fel}");
}
