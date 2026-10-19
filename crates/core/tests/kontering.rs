//! Rätta kontering på en utfärdad kundfaktura genom `core` mot en tillfällig databas (#142):
//! rättelsen av utfärdandets verifikat och det nya utfärdandeverifikatet i samma transaktion,
//! fakturan, numret och PDF:en oförändrade, och behandlingshistoriken.

use maya_core::atkomst::ÅtkomstFel;
use maya_core::kontering::{self, KonteringRättad};
use maya_core::kundfaktura::{self, KundfakturaFel, KundfakturaId, Utfärdad};
use maya_core::verifikat::{
    self, Flöde, Källa, Rättelseval, Verifikat, VerifikatFel, VerifikatId
};
use maya_core::{Databas, Klocka, behandlingshistorik, testhjalp};
use maya_core::{aktor::Aktör, foretag::FöretagsId};
use maya_domain::kontering::NyKontering;
use maya_domain::kundfaktura::{NyFakturarad, NyKundfaktura};

const NU: &str = "2026-10-18T08:00:00Z";

fn klocka() -> Klocka {
    Klocka::fast(NU.parse().unwrap())
}

fn rad(beskrivning: &str, antal: &str, à_pris: &str, sats: &str, konto: &str) -> NyFakturarad {
    NyFakturarad {
        beskrivning: beskrivning.into(),
        antal: antal.into(),
        enhet: "st".into(),
        à_pris: à_pris.into(),
        rabatt: String::new(),
        momssats: sats.into(),
        konto: konto.into(),
    }
}

/// Ett fakturaföretag med en utfärdad faktura 1 till Kund AB den 10 mars: 10 konsulttimmar à
/// 950 kr (25 %, 3001) och 2 handböcker à 200 kr (6 %, 3003), totalt 12 299 kr.
async fn utfärdad(db: &Databas) -> (Aktör, FöretagsId, Utfärdad) {
    utfärdad_den(db, "2026-03-10").await
}

async fn utfärdad_den(db: &Databas, fakturadatum: &str) -> (Aktör, FöretagsId, Utfärdad) {
    let (anna, id) = testhjalp::fakturaföretag(db).await;
    let kund = testhjalp::kund(db, &anna, id, "Kund AB").await;
    let utkast = kundfaktura::spara_utkast(
        db,
        &klocka(),
        &anna,
        id,
        None,
        &NyKundfaktura {
            kund: kund.to_string(),
            fakturadatum: fakturadatum.into(),
            rader: vec![
                rad("Konsulttimmar", "10", "950", "25", "3001"),
                rad("Handbok", "2", "200", "6", "3003"),
            ],
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let u = kundfaktura::utfärda(db, &klocka(), &anna, id, utkast)
        .await
        .unwrap();
    (anna, id, u)
}

fn formulär(konton: &[&str], orsak: &str) -> NyKontering {
    NyKontering {
        konton: konton.iter().map(|k| k.to_string()).collect(),
        orsak: orsak.into(),
    }
}

async fn rätta(
    db: &Databas,
    anna: &Aktör,
    id: FöretagsId,
    f: KundfakturaId,
    nya: &NyKontering,
) -> Result<KonteringRättad, KundfakturaFel> {
    kontering::rätta(db, &klocka(), anna, id, f, nya).await
}

async fn läs(db: &Databas, anna: &Aktör, id: FöretagsId, v: VerifikatId) -> Verifikat {
    verifikat::verifikatet(db, anna, id, v)
        .await
        .unwrap()
        .unwrap()
}

fn rader(v: &Verifikat) -> Vec<(u16, i64, i64)> {
    v.rader
        .iter()
        .map(|r| (r.konto.unwrap().get(), r.debet.öre(), r.kredit.öre()))
        .collect()
}

async fn antal_verifikat(db: &Databas, anna: &Aktör, id: FöretagsId) -> usize {
    verifikat::lista(db, anna, id, verifikat::Urval::Alla)
        .await
        .unwrap()
        .len()
}

#[tokio::test]
async fn rättelsen_och_det_nya_verifikatet_bokförs_med_dagens_datum_och_länkar_till_fakturan() {
    let db = Databas::tillfällig().await;
    let (anna, id, u) = utfärdad(&db).await;

    let r = rätta(
        &db,
        &anna,
        id,
        u.id,
        &formulär(&["3002", "3003"], "fel momssats"),
    )
    .await
    .unwrap();
    assert_eq!(r.rättelse.nummer.to_string(), "V2");
    assert_eq!(r.verifikat.nummer.to_string(), "V3");

    // Rättelsen vänder utfärdandets verifikat enligt etapp 1 (R1, R2).
    let rättelse = läs(&db, &anna, id, r.rättelse.id).await;
    assert_eq!(rättelse.källa, Källa::Rättelse);
    assert_eq!(rättelse.rättar.as_ref().unwrap().id, u.verifikat.id);
    assert_eq!(rättelse.datum.unwrap().to_string(), "2026-10-18");
    assert_eq!(rättelse.beskrivning, "Rättelse av V1, fel momssats");
    assert_eq!(
        rader(&rättelse),
        [
            (1510, 0, 1_229_900),
            (3001, 950_000, 0),
            (3003, 40_000, 0),
            (2611, 237_500, 0),
            (2631, 2_400, 0),
        ]
    );
    let original = läs(&db, &anna, id, u.verifikat.id).await;
    assert_eq!(original.rättad_av.unwrap().id, r.rättelse.id);

    // Det nya utfärdandeverifikatet: samma fordran och moms, intäkterna på de nya kontona,
    // med PDF:en som underlag och motparten i texten.
    let ny = läs(&db, &anna, id, r.verifikat.id).await;
    assert_eq!(ny.källa, Källa::Manuell);
    assert_eq!(ny.datum.unwrap().to_string(), "2026-10-18");
    assert_eq!(ny.beskrivning, "Kundfaktura 1, Kund AB");
    assert_eq!(
        rader(&ny),
        [
            (1510, 1_229_900, 0),
            (3002, 0, 950_000),
            (3003, 0, 40_000),
            (2611, 0, 237_500),
            (2631, 0, 2_400),
        ]
    );
    assert_eq!(ny.underlag.len(), 1);
    assert_eq!(ny.underlag[0].id, u.underlag);
    assert!(!ny.underlag[0].komplettering);

    // Båda länkar till fakturan.
    for v in [&rättelse, &ny] {
        let flöde = v.flöde.as_ref().unwrap();
        assert_eq!(flöde.flöde, Flöde::Kundfaktura);
        assert_eq!(flöde.id.to_string(), u.id.to_string());
        assert_eq!(flöde.text, "kundfaktura 1 till Kund AB");
    }
}

#[tokio::test]
async fn fakturan_numret_och_pdfen_ändras_inte() {
    let db = Databas::tillfällig().await;
    let (anna, id, u) = utfärdad(&db).await;
    let före = kundfaktura::kundfakturan(&db, &anna, id, u.id)
        .await
        .unwrap()
        .utfärdad
        .unwrap();
    let pdf_före = maya_core::underlag::filen(&db, &anna, id, u.underlag)
        .await
        .unwrap()
        .unwrap();

    rätta(&db, &anna, id, u.id, &formulär(&["3002", "3004"], ""))
        .await
        .unwrap();

    let efter = kundfaktura::kundfakturan(&db, &anna, id, u.id)
        .await
        .unwrap()
        .utfärdad
        .unwrap();
    assert_eq!(efter.nummer, före.nummer);
    assert_eq!(efter.referens, före.referens);
    assert_eq!(efter.rader, före.rader);
    assert_eq!(efter.total, före.total);
    assert_eq!(efter.pdf, före.pdf);
    assert_eq!(efter.pdf.sha256, före.pdf.sha256);
    let pdf_efter = maya_core::underlag::filen(&db, &anna, id, u.underlag)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(pdf_efter.innehåll, pdf_före.innehåll);
}

#[tokio::test]
async fn konteringen_visar_det_gällande_verifikatet_och_kontona_per_rad() {
    let db = Databas::tillfällig().await;
    let (anna, id, u) = utfärdad(&db).await;

    let k = kontering::kontering(&db, &anna, id, u.id).await.unwrap();
    assert_eq!(k.verifikat.id, u.verifikat.id);
    let konton: Vec<u16> = k.rader.iter().map(|r| r.konto.get()).collect();
    assert_eq!(konton, [3001, 3003]);
    assert_eq!(k.rader[0].beskrivning, "Konsulttimmar");
    assert_eq!(k.rader[0].netto, 950_000);
    assert!(k.rättningar.is_empty());

    let r = rätta(
        &db,
        &anna,
        id,
        u.id,
        &formulär(&["3002", "3003"], "fel konto"),
    )
    .await
    .unwrap();
    let k = kontering::kontering(&db, &anna, id, u.id).await.unwrap();
    assert_eq!(k.verifikat.id, r.verifikat.id);
    let konton: Vec<u16> = k.rader.iter().map(|r| r.konto.get()).collect();
    assert_eq!(konton, [3002, 3003]);
    assert_eq!(k.rättningar.len(), 1);
    assert_eq!(k.rättningar[0].rättelse.id, r.rättelse.id);
    assert_eq!(k.rättningar[0].verifikat.id, r.verifikat.id);
    assert_eq!(k.rättningar[0].orsak, "fel konto");
}

#[tokio::test]
async fn en_andra_rättning_vänder_det_nya_utfärdandeverifikatet() {
    let db = Databas::tillfällig().await;
    let (anna, id, u) = utfärdad(&db).await;
    let första = rätta(&db, &anna, id, u.id, &formulär(&["3002", "3003"], ""))
        .await
        .unwrap();

    // Kontona i formuläret jämförs med det som är bokfört nu.
    assert!(matches!(
        rätta(&db, &anna, id, u.id, &formulär(&["3002", "3003"], "")).await,
        Err(KundfakturaFel::Spärrat(s)) if s[0].kod == "kontering_oforandrad"
    ));
    let andra = rätta(&db, &anna, id, u.id, &formulär(&["3001", "3003"], ""))
        .await
        .unwrap();
    assert_eq!(andra.rättelse.nummer.to_string(), "V4");
    assert_eq!(andra.verifikat.nummer.to_string(), "V5");
    let rättelse = läs(&db, &anna, id, andra.rättelse.id).await;
    assert_eq!(rättelse.rättar.unwrap().id, första.verifikat.id);
    assert_eq!(rättelse.beskrivning, "Rättelse av V3");
    let ny = läs(&db, &anna, id, andra.verifikat.id).await;
    assert_eq!(
        rader(&ny),
        [
            (1510, 1_229_900, 0),
            (3001, 0, 950_000),
            (3003, 0, 40_000),
            (2611, 0, 237_500),
            (2631, 0, 2_400),
        ]
    );
}

#[tokio::test]
async fn kontering_rättad_loggas_med_rättelsen_det_nya_verifikatet_och_kontona() {
    let db = Databas::tillfällig().await;
    let (anna, id, u) = utfärdad(&db).await;
    rätta(
        &db,
        &anna,
        id,
        u.id,
        &formulär(&["3002", "3003"], "fel konto"),
    )
    .await
    .unwrap();

    let poster = behandlingshistorik::läs(&db, &anna, id).await.unwrap();
    let typer: Vec<&str> = poster
        .iter()
        .rev()
        .take(4)
        .map(|p| p.typ.as_str())
        .collect();
    assert_eq!(
        typer,
        [
            "kontering rättad",
            "verifikat bokfört",
            "verifikat rättat",
            "verifikat bokfört"
        ]
    );
    let post = poster.last().unwrap();
    assert_eq!(post.objekt, "1");
    assert_eq!(post.aktör, anna);
    assert_eq!(post.tid.to_string(), NU);
    let b = &post.beskrivning;
    assert_eq!(b["kundfaktura"].to_string(), u.id.to_string());
    assert_eq!(b["nummer"], 1);
    assert_eq!(b["orsak"], "fel konto");
    assert_eq!(b["vänt"]["nummer"], "V1");
    assert_eq!(b["rättelse"]["nummer"], "V2");
    assert_eq!(b["verifikat"]["nummer"], "V3");
    assert_eq!(
        b["rader"],
        serde_json::json!([{ "rad": 1, "beskrivning": "Konsulttimmar", "från": 3001, "till": 3002 }])
    );

    // Posten syns i fakturans historik, och texten säger vad som hände.
    let faktura = kundfaktura::kundfakturan(&db, &anna, id, u.id)
        .await
        .unwrap();
    let sista = faktura.historik.last().unwrap();
    assert_eq!(sista.post.typ, "kontering rättad");
    let text = behandlingshistorik::Post::text(&sista.post);
    assert!(text.contains("V1"), "{text}");
    assert!(text.contains("3001 till 3002"), "{text}");
}

#[tokio::test]
async fn ett_ogiltigt_konto_spärras_och_inget_bokförs() {
    let db = Databas::tillfällig().await;
    let (anna, id, u) = utfärdad(&db).await;
    let före = antal_verifikat(&db, &anna, id).await;

    let Err(KundfakturaFel::Spärrat(s)) =
        rätta(&db, &anna, id, u.id, &formulär(&["1930", "3003"], "")).await
    else {
        panic!("spärr väntades");
    };
    assert_eq!(s[0].kod, "konto_inte_intakt");
    assert_eq!(antal_verifikat(&db, &anna, id).await, före);
}

#[tokio::test]
async fn när_rättelsen_spärras_bokförs_varken_den_eller_det_nya_verifikatet() {
    let db = Databas::tillfällig().await;
    // Fakturadatumet är efter dagens datum, så rättelsen skulle dateras före originalet.
    let (anna, id, u) = utfärdad_den(&db, "2026-12-01").await;
    let före = antal_verifikat(&db, &anna, id).await;

    let fel = rätta(&db, &anna, id, u.id, &formulär(&["3002", "3003"], ""))
        .await
        .unwrap_err();
    assert!(
        matches!(&fel, KundfakturaFel::Verifikat(VerifikatFel::Spärrat(s)) if s[0].kod == "fore_originalet"),
        "{fel:?}"
    );
    assert_eq!(antal_verifikat(&db, &anna, id).await, före);
    let original = läs(&db, &anna, id, u.verifikat.id).await;
    assert!(original.rättad_av.is_none());
    let poster = behandlingshistorik::läs(&db, &anna, id).await.unwrap();
    assert!(poster.iter().all(|p| p.typ != "kontering rättad"));
}

#[tokio::test]
async fn ett_utkast_har_ingen_kontering_att_rätta() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::fakturaföretag(&db).await;
    let kund = testhjalp::kund(&db, &anna, id, "Kund AB").await;
    let utkast = testhjalp::kundfakturautkast(&db, &anna, id, kund, "2026-03-10").await;

    assert!(matches!(
        rätta(&db, &anna, id, utkast, &formulär(&["3002"], "")).await,
        Err(KundfakturaFel::Verifikat(VerifikatFel::IngetVerifikat))
    ));
    assert!(matches!(
        kontering::kontering(&db, &anna, id, utkast).await,
        Err(KundfakturaFel::Verifikat(VerifikatFel::IngetVerifikat))
    ));
}

#[tokio::test]
async fn verifikaten_från_rättningen_rättas_inte_för_sig() {
    let db = Databas::tillfällig().await;
    let (anna, id, u) = utfärdad(&db).await;
    let r = rätta(&db, &anna, id, u.id, &formulär(&["3002", "3003"], ""))
        .await
        .unwrap();
    for v in [r.verifikat.id, r.rättelse.id] {
        let fel = verifikat::rätta(
            &db,
            &klocka(),
            &anna,
            id,
            v,
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
    }
}

#[tokio::test]
async fn den_som_saknar_åtkomst_får_inte_rätta_konteringen() {
    let db = Databas::tillfällig().await;
    let (_, id, u) = utfärdad(&db).await;
    let bo = testhjalp::användare(&db, "bo@example.se").await;
    assert!(matches!(
        rätta(&db, &bo, id, u.id, &formulär(&["3002", "3003"], "")).await,
        Err(KundfakturaFel::Åtkomst(ÅtkomstFel::IngenÅtkomst))
    ));
    assert!(matches!(
        kontering::kontering(&db, &bo, id, u.id).await,
        Err(KundfakturaFel::Åtkomst(ÅtkomstFel::IngenÅtkomst))
    ));
}

#[tokio::test]
async fn triggers_avvisar_ändring_och_radering_av_en_rättad_kontering() {
    let db = Databas::tillfällig().await;
    let (anna, id, u) = utfärdad(&db).await;
    rätta(&db, &anna, id, u.id, &formulär(&["3002", "3003"], ""))
        .await
        .unwrap();
    for sql in [
        "UPDATE kundfaktura_kontering SET orsak = 'x'",
        "DELETE FROM kundfaktura_kontering",
        "UPDATE kundfaktura_konteringsrad SET konto = 3004",
        "DELETE FROM kundfaktura_konteringsrad",
        "INSERT INTO kundfaktura_konteringsrad (kontering_id, radnr, konto)
         SELECT id, 3, 3004 FROM kundfaktura_kontering",
    ] {
        let fel = testhjalp::kör_sql(&db, sql).await.unwrap_err();
        assert!(
            fel.to_string().contains("går inte att ändra"),
            "{sql}: {fel}"
        );
    }
}
