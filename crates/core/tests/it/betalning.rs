//! Betalning av kundfaktura genom `core` mot en tillfällig databas (#139): betalningens
//! verifikat enligt tabellen över automatkonteringar i samma transaktion som betalningen,
//! delbetalning, öresutjämning, spärren mot överbetalning, Rätta betalning, status som räknas
//! fram och triggrarna som skyddar betalningarna.

use jiff::civil::date;
use maya_core::atkomst::ÅtkomstFel;
use maya_core::betalning::{self, BetalningFel, Faktura};
use maya_core::kundfaktura::{self, Utfärdad};
use maya_core::verifikat::{self, Flöde, Rättelseval, VerifikatFel, VerifikatId};
use maya_core::{Databas, Klocka, behandlingshistorik, testhjalp};
use maya_core::{aktor::Aktör, foretag::FöretagsId};
use maya_domain::betalning::NyBetalning;
use maya_domain::reskontra::{Märke, märke};

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

async fn återstår(db: &Databas, anna: &Aktör, id: FöretagsId, f: &Utfärdad) -> i64 {
    betalning::reskontrapost(db, anna, id, Faktura::Kund(f.id))
        .await
        .unwrap()
        .återstår
}

async fn antal_verifikat(db: &Databas, anna: &Aktör, id: FöretagsId) -> usize {
    verifikat::lista(db, anna, id, verifikat::Urval::Bokförda)
        .await
        .unwrap()
        .len()
}

#[tokio::test]
async fn betalningen_bokför_betalkontot_mot_1510_daterad_på_betaldagen_med_fakturan_som_referens() {
    let db = Databas::tillfällig().await;
    let (anna, id, f) = faktura(&db).await;
    let r = betalning::registrera(
        &db,
        &klocka(),
        &anna,
        id,
        Faktura::Kund(f.id),
        &betalning("2026-04-09", "11 875,00"),
    )
    .await
    .unwrap();
    assert_eq!(r.återstår, 0);
    let v = verifikat::verifikatet(&db, &anna, id, r.verifikat.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(v.datum, Some(date(2026, 4, 9)));
    assert_eq!(
        v.beskrivning,
        format!("Betalning av kundfaktura {}, Kund AB", f.nummer)
    );
    assert_eq!(
        rader(&db, &anna, id, r.verifikat.id).await,
        [(1930, TOTAL, 0), (1510, 0, TOTAL)]
    );
    // Verifikatet länkar till fakturan och rättas där.
    let flöde = v.flöde.clone().expect("länken till fakturan");
    assert_eq!(flöde.flöde, Flöde::Kundfaktura);
    assert_eq!(flöde.id.to_string(), f.id.to_string());
    let fel = verifikat::rätta(
        &db,
        &klocka(),
        &anna,
        id,
        r.verifikat.id,
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

    let post = betalning::reskontrapost(&db, &anna, id, Faktura::Kund(f.id))
        .await
        .unwrap();
    assert_eq!(post.total, TOTAL);
    assert_eq!(post.betalningar.len(), 1);
    assert_eq!(post.betalningar[0].belopp, TOTAL);
    assert_eq!(post.betalningar[0].betaldag, date(2026, 4, 9));
    assert_eq!(post.betalningar[0].verifikat.id, r.verifikat.id);
    assert!(post.betalningar[0].rättad.is_none());

    let historik = behandlingshistorik::läs(&db, &anna, id).await.unwrap();
    let registrerad = historik
        .iter()
        .find(|p| p.typ == "betalning registrerad")
        .expect("betalningen loggas");
    assert_eq!(registrerad.belopp().unwrap().öre(), TOTAL);
    assert_eq!(
        registrerad.beskrivning["kundfaktura"],
        f.id.to_string().parse::<i64>().unwrap()
    );
    assert!(
        registrerad.text().contains("11 875,00"),
        "{}",
        registrerad.text()
    );
    // Betalningen syns i fakturans historik.
    let k = kundfaktura::kundfakturan(&db, &anna, id, f.id)
        .await
        .unwrap();
    assert!(
        k.historik
            .iter()
            .any(|h| h.post.typ == "betalning registrerad")
    );
}

#[tokio::test]
async fn delbetalningar_minskar_det_som_återstår_tills_fakturan_är_betald() {
    let db = Databas::tillfällig().await;
    let (anna, id, f) = faktura(&db).await;
    let första = betalning::registrera(
        &db,
        &klocka(),
        &anna,
        id,
        Faktura::Kund(f.id),
        &betalning("2026-04-01", "5 000"),
    )
    .await
    .unwrap();
    assert_eq!(första.återstår, TOTAL - 500_000);
    assert_eq!(
        rader(&db, &anna, id, första.verifikat.id).await,
        [(1930, 500_000, 0), (1510, 0, 500_000)]
    );
    let lista = kundfaktura::lista(&db, &anna, id).await.unwrap();
    assert_eq!(lista[0].återstår, TOTAL - 500_000);
    let förfallodag = lista[0].förfallodag.unwrap();
    assert_eq!(
        märke(lista[0].total, lista[0].återstår, förfallodag, förfallodag),
        Märke::Delbetald
    );

    let andra = betalning::registrera(
        &db,
        &klocka(),
        &anna,
        id,
        Faktura::Kund(f.id),
        &betalning("2026-04-20", "6 875"),
    )
    .await
    .unwrap();
    assert_eq!(andra.återstår, 0);
    assert_eq!(återstår(&db, &anna, id, &f).await, 0);
    let fel = betalning::registrera(
        &db,
        &klocka(),
        &anna,
        id,
        Faktura::Kund(f.id),
        &betalning("2026-04-21", "1"),
    )
    .await
    .unwrap_err();
    assert!(
        matches!(&fel, BetalningFel::Spärrat(s) if s[0].kod == "redan_betald"),
        "{fel:?}"
    );
}

#[tokio::test]
async fn en_rest_på_högst_10_kr_stängs_mot_öresutjämning_i_samma_verifikat() {
    let db = Databas::tillfällig().await;
    let (anna, id, f) = faktura(&db).await;
    let r = betalning::registrera(
        &db,
        &klocka(),
        &anna,
        id,
        Faktura::Kund(f.id),
        &NyBetalning {
            öresutjämning: true,
            ..betalning("2026-04-09", "11 870")
        },
    )
    .await
    .unwrap();
    assert_eq!(r.återstår, 0);
    assert_eq!(
        rader(&db, &anna, id, r.verifikat.id).await,
        [(1930, 1_187_000, 0), (3740, 500, 0), (1510, 0, TOTAL)]
    );
    let post = betalning::reskontrapost(&db, &anna, id, Faktura::Kund(f.id))
        .await
        .unwrap();
    assert_eq!(post.betalningar[0].öresutjämning, 500);

    // En rest över 10 kr spärras.
    let andra = testhjalp::utfärdad_kundfaktura(&db, &anna, id, "Kund 2 AB", "2026-03-10").await;
    let fel = betalning::registrera(
        &db,
        &klocka(),
        &anna,
        id,
        Faktura::Kund(andra.id),
        &NyBetalning {
            öresutjämning: true,
            ..betalning("2026-04-09", "11 864,99")
        },
    )
    .await
    .unwrap_err();
    assert!(
        matches!(&fel, BetalningFel::Spärrat(s) if s[0].kod == "oresutjamning_for_stor"),
        "{fel:?}"
    );
}

#[tokio::test]
async fn en_överbetalning_spärras_och_inget_bokförs() {
    let db = Databas::tillfällig().await;
    let (anna, id, f) = faktura(&db).await;
    let före = antal_verifikat(&db, &anna, id).await;
    let fel = betalning::registrera(
        &db,
        &klocka(),
        &anna,
        id,
        Faktura::Kund(f.id),
        &betalning("2026-04-09", "11 875,01"),
    )
    .await
    .unwrap_err();
    let BetalningFel::Spärrat(spärrar) = &fel else {
        panic!("{fel:?}");
    };
    assert_eq!(spärrar[0].kod, "overbetalning");
    assert!(spärrar[0].text.contains("manuellt verifikat"));
    assert_eq!(antal_verifikat(&db, &anna, id).await, före);
    assert_eq!(återstår(&db, &anna, id, &f).await, TOTAL);
}

#[tokio::test]
async fn ett_betalkonto_utanför_kassa_och_bank_spärras_och_betaldagen_kan_skapa_nästa_år() {
    let db = Databas::tillfällig().await;
    let (anna, id, f) = faktura(&db).await;
    let fel = betalning::registrera(
        &db,
        &klocka(),
        &anna,
        id,
        Faktura::Kund(f.id),
        &NyBetalning {
            konto: "3001".into(),
            ..betalning("2026-04-09", "100")
        },
    )
    .await
    .unwrap_err();
    assert!(
        matches!(&fel, BetalningFel::Spärrat(s) if s[0].kod == "konto_inte_betalkonto"),
        "{fel:?}"
    );
    let r = betalning::registrera(
        &db,
        &klocka(),
        &anna,
        id,
        Faktura::Kund(f.id),
        &betalning("2027-01-15", "100"),
    )
    .await
    .unwrap();
    let v = verifikat::verifikatet(&db, &anna, id, r.verifikat.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(v.räkenskapsår.unwrap().start(), date(2027, 1, 1));
}

#[tokio::test]
async fn rätta_betalning_vänder_verifikatet_och_återställer_det_som_återstår() {
    let db = Databas::tillfällig().await;
    let (anna, id, f) = faktura(&db).await;
    let r = betalning::registrera(
        &db,
        &klocka(),
        &anna,
        id,
        Faktura::Kund(f.id),
        &betalning("2026-04-09", "1 875"),
    )
    .await
    .unwrap();
    assert_eq!(återstår(&db, &anna, id, &f).await, TOTAL - 187_500);

    let rättad = betalning::rätta(&db, &klocka(), &anna, id, r.id, "2026-10-18", "fel belopp")
        .await
        .unwrap();
    assert_eq!(rättad.återstår, TOTAL);
    assert_eq!(
        rader(&db, &anna, id, rättad.rättelse.id).await,
        [(1930, 0, 187_500), (1510, 187_500, 0)]
    );
    let rättelse = verifikat::verifikatet(&db, &anna, id, rättad.rättelse.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(rättelse.datum, Some(date(2026, 10, 18)));
    assert!(rättelse.beskrivning.contains("fel belopp"));
    assert_eq!(rättelse.rättar.unwrap().id, r.verifikat.id);
    let original = verifikat::verifikatet(&db, &anna, id, r.verifikat.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(original.rättad_av.unwrap().id, rättad.rättelse.id);

    // Betalningen finns kvar och visar rättelsen, men räknas inte i reskontran.
    let post = betalning::reskontrapost(&db, &anna, id, Faktura::Kund(f.id))
        .await
        .unwrap();
    assert_eq!(post.återstår, TOTAL);
    let rättelseinfo = post.betalningar[0].rättad.as_ref().expect("rättelsen");
    assert_eq!(rättelseinfo.verifikat.id, rättad.rättelse.id);
    assert_eq!(rättelseinfo.orsak, "fel belopp");
    assert_eq!(
        kundfaktura::lista(&db, &anna, id).await.unwrap()[0].återstår,
        TOTAL
    );

    // En rättad betalning rättas inte igen.
    let fel = betalning::rätta(&db, &klocka(), &anna, id, r.id, "2026-10-18", "")
        .await
        .unwrap_err();
    assert!(matches!(fel, BetalningFel::RedanRättad), "{fel:?}");

    // Betalningen kan registreras om.
    let om = betalning::registrera(
        &db,
        &klocka(),
        &anna,
        id,
        Faktura::Kund(f.id),
        &betalning("2026-04-09", "11 875"),
    )
    .await
    .unwrap();
    assert_eq!(om.återstår, 0);

    let historik = behandlingshistorik::läs(&db, &anna, id).await.unwrap();
    let post = historik
        .iter()
        .find(|p| p.typ == "betalning rättad")
        .expect("rättelsen loggas");
    assert_eq!(post.belopp().unwrap().öre(), 187_500);
    assert!(post.text().contains("fel belopp"), "{}", post.text());
    assert!(historik.iter().any(|p| p.typ == "verifikat rättat"));
}

#[tokio::test]
async fn rätta_betalning_före_betaldagen_spärras() {
    let db = Databas::tillfällig().await;
    let (anna, id, f) = faktura(&db).await;
    let r = betalning::registrera(
        &db,
        &klocka(),
        &anna,
        id,
        Faktura::Kund(f.id),
        &betalning("2026-04-09", "100"),
    )
    .await
    .unwrap();
    let fel = betalning::rätta(&db, &klocka(), &anna, id, r.id, "2026-04-08", "")
        .await
        .unwrap_err();
    assert!(
        matches!(&fel, BetalningFel::Verifikat(VerifikatFel::Spärrat(s)) if s[0].kod == "fore_originalet"),
        "{fel:?}"
    );
    assert_eq!(återstår(&db, &anna, id, &f).await, TOTAL - 10_000);
}

#[tokio::test]
async fn ett_utkast_och_en_okänd_faktura_tar_inte_emot_betalningar() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::fakturaföretag(&db).await;
    let kund = testhjalp::kund(&db, &anna, id, "Kund AB").await;
    let utkast = testhjalp::kundfakturautkast(&db, &anna, id, kund, "2026-03-10").await;
    let fel = betalning::registrera(
        &db,
        &klocka(),
        &anna,
        id,
        Faktura::Kund(utkast),
        &betalning("2026-04-09", "100"),
    )
    .await
    .unwrap_err();
    assert!(matches!(fel, BetalningFel::FinnsInte), "{fel:?}");
}

#[tokio::test]
async fn den_som_saknar_åtkomst_får_inte_registrera_eller_rätta_en_betalning() {
    let db = Databas::tillfällig().await;
    let (anna, id, f) = faktura(&db).await;
    let bo = testhjalp::användare(&db, "bo@example.se").await;
    let fel = betalning::registrera(
        &db,
        &klocka(),
        &bo,
        id,
        Faktura::Kund(f.id),
        &betalning("2026-04-09", "100"),
    )
    .await
    .unwrap_err();
    assert!(
        matches!(fel, BetalningFel::Åtkomst(ÅtkomstFel::IngenÅtkomst)),
        "{fel:?}"
    );
    let r = betalning::registrera(
        &db,
        &klocka(),
        &anna,
        id,
        Faktura::Kund(f.id),
        &betalning("2026-04-09", "100"),
    )
    .await
    .unwrap();
    let fel = betalning::rätta(&db, &klocka(), &bo, id, r.id, "2026-10-18", "")
        .await
        .unwrap_err();
    assert!(
        matches!(fel, BetalningFel::Åtkomst(ÅtkomstFel::IngenÅtkomst)),
        "{fel:?}"
    );
    assert!(
        betalning::reskontrapost(&db, &bo, id, Faktura::Kund(f.id))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn triggrarna_avvisar_att_en_betalning_eller_dess_rättelse_ändras_eller_raderas() {
    let db = Databas::tillfällig().await;
    let (anna, id, f) = faktura(&db).await;
    let r = betalning::registrera(
        &db,
        &klocka(),
        &anna,
        id,
        Faktura::Kund(f.id),
        &betalning("2026-04-09", "100"),
    )
    .await
    .unwrap();
    for sql in [
        "UPDATE betalning SET belopp = 1",
        "UPDATE betalning SET betaldag = '2026-04-10'",
    ] {
        let fel = testhjalp::kör_sql(&db, sql).await.unwrap_err();
        assert!(fel.to_string().contains("går inte att ändra"), "{fel}");
    }
    let fel = testhjalp::kör_sql(&db, "DELETE FROM betalning")
        .await
        .unwrap_err();
    assert!(fel.to_string().contains("går inte att radera"), "{fel}");

    // En betalning utan bokfört verifikat, eller på ett utkast, går inte att lägga in.
    let fel = testhjalp::kör_sql(
        &db,
        "INSERT INTO betalning (foretag_id, kundfaktura_id, betaldag, belopp, konto, verifikat_id,
                                registrerad, registrerad_kanal)
         SELECT foretag_id, kundfaktura_id, betaldag, 5, konto,
                (SELECT max(id) FROM verifikat) + 1, registrerad, registrerad_kanal FROM betalning",
    )
    .await
    .unwrap_err();
    assert!(
        fel.to_string().contains("kräver en utfärdad faktura"),
        "{fel}"
    );
    // En rättelse måste vända just betalningens verifikat.
    let fel = testhjalp::kör_sql(
        &db,
        "INSERT INTO betalningsrattelse (betalning_id, verifikat_id, rattad, rattad_kanal)
         SELECT id, verifikat_id, registrerad, registrerad_kanal FROM betalning",
    )
    .await
    .unwrap_err();
    assert!(
        fel.to_string().contains("kräver en bokförd rättelse"),
        "{fel}"
    );

    betalning::rätta(&db, &klocka(), &anna, id, r.id, "2026-10-18", "")
        .await
        .unwrap();
    let fel = testhjalp::kör_sql(&db, "UPDATE betalningsrattelse SET orsak = 'x'")
        .await
        .unwrap_err();
    assert!(fel.to_string().contains("går inte att ändra"), "{fel}");
    let fel = testhjalp::kör_sql(&db, "DELETE FROM betalningsrattelse")
        .await
        .unwrap_err();
    assert!(fel.to_string().contains("går inte att ändra"), "{fel}");
}

#[tokio::test]
async fn betalningar_på_samma_faktura_samtidigt_överbetalar_aldrig() {
    let db = Databas::tillfällig().await;
    let (anna, id, f) = faktura(&db).await;
    let mut jobb = Vec::new();
    for _ in 0..4 {
        let db = db.clone();
        jobb.push(tokio::spawn(async move {
            betalning::registrera(
                &db,
                &klocka(),
                &anna,
                id,
                Faktura::Kund(f.id),
                &betalning("2026-04-09", "5 000"),
            )
            .await
        }));
    }
    let mut lyckade = 0;
    for j in jobb {
        if j.await.unwrap().is_ok() {
            lyckade += 1;
        }
    }
    assert_eq!(lyckade, 2);
    assert_eq!(återstår(&db, &anna, id, &f).await, TOTAL - 1_000_000);
}

#[tokio::test]
async fn kundens_fakturor_visar_vad_som_är_obetalt() {
    let db = Databas::tillfällig().await;
    let (anna, id, f) = faktura(&db).await;
    testhjalp::utfärdad_kundfaktura(&db, &anna, id, "Annan AB", "2026-03-11").await;
    betalning::registrera(
        &db,
        &klocka(),
        &anna,
        id,
        Faktura::Kund(f.id),
        &betalning("2026-04-09", "1 875"),
    )
    .await
    .unwrap();
    let kund = maya_core::kund::kunder(&db, &anna, id)
        .await
        .unwrap()
        .into_iter()
        .find(|k| k.namn == "Kund AB")
        .unwrap()
        .id;
    let fakturor = kundfaktura::kundens_fakturor(&db, &anna, id, kund)
        .await
        .unwrap();
    assert_eq!(fakturor.len(), 1);
    assert_eq!(fakturor[0].id, f.id);
    assert_eq!(fakturor[0].återstår, TOTAL - 187_500);
}
