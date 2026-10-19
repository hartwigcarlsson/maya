//! Externa kundfakturor genom `core` mot en tillfällig databas (#143): registreringen bokförd
//! enligt tabellen över automatkonteringar i samma transaktion, dubblettspärren på det externa
//! numret, betalning, kredit och kundförlust som för Mayas egna fakturor, Rätta registrering som
//! låser upp fakturan med betalningarna kvar, och triggrarna som bara släpper igenom det som en
//! upplåsning öppnar.

use jiff::civil::date;
use maya_core::betalning::{self, BetalningFel, Faktura};
use maya_core::extern_kundfaktura::{self, ExternFel};
use maya_core::kundfaktura::{self, KundfakturaFel, KundfakturaId, Status};
use maya_core::underlag;
use maya_core::verifikat::{self, Flöde, Rättelseval, VerifikatFel, VerifikatId};
use maya_core::{Databas, Klocka, TillfälligDatabas, behandlingshistorik, kundforlust, testhjalp};
use maya_core::{aktor::Aktör, foretag::FöretagsId, kund::KundId};
use maya_domain::betalning::NyBetalning;
use maya_domain::extern_kundfaktura::{NyExternFaktura, NyExternRad};
use maya_domain::kundforlust::NyKundförlust;

const NU: &str = "2026-10-18T08:00:00Z";

fn klocka() -> Klocka {
    Klocka::fast(NU.parse().unwrap())
}

/// 8 000 kr på 3001 med 25 % och 1 000 kr på 3002 med 12 %: 11 120,00 kr.
const TOTAL: i64 = 1_112_000;

fn rad(momssats: &str, konto: &str, belopp: &str) -> NyExternRad {
    NyExternRad {
        momssats: momssats.into(),
        konto: konto.into(),
        belopp: belopp.into(),
    }
}

fn formulär(kund: KundId, nummer: &str) -> NyExternFaktura {
    NyExternFaktura {
        kund: kund.to_string(),
        fakturanummer: nummer.into(),
        fakturadatum: "2026-03-10".into(),
        förfallodag: "2026-04-09".into(),
        rader: vec![rad("25", "3001", "8 000"), rad("12", "3002", "1 000")],
        ..Default::default()
    }
}

async fn utkast(db: &Databas, anna: &Aktör, id: FöretagsId) -> KundfakturaId {
    extern_kundfaktura::ladda_upp(
        db,
        &klocka(),
        anna,
        id,
        ("F-17.pdf", "application/pdf", b"%PDF-1.4 extern F-17"),
    )
    .await
    .unwrap()
    .0
}

async fn företag() -> (TillfälligDatabas, Aktör, FöretagsId, KundId) {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::fakturaföretag(&db).await;
    let kund = testhjalp::kund(&db, &anna, id, "Kund AB").await;
    (db, anna, id, kund)
}

async fn registrerad(db: &Databas, anna: &Aktör, id: FöretagsId, kund: KundId) -> KundfakturaId {
    let f = utkast(db, anna, id).await;
    extern_kundfaktura::registrera(db, &klocka(), anna, id, f, &formulär(kund, "F-17"))
        .await
        .unwrap();
    f
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

async fn betala(db: &Databas, anna: &Aktör, id: FöretagsId, f: KundfakturaId, belopp: &str) {
    betalning::registrera(
        db,
        &klocka(),
        anna,
        id,
        Faktura::Kund(f),
        &NyBetalning {
            betaldag: "2026-04-01".into(),
            belopp: belopp.into(),
            konto: "1930".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
}

async fn återstår(db: &Databas, anna: &Aktör, id: FöretagsId, f: KundfakturaId) -> i64 {
    betalning::reskontrapost(db, anna, id, Faktura::Kund(f))
        .await
        .unwrap()
        .återstår
}

async fn listraden(
    db: &Databas,
    anna: &Aktör,
    id: FöretagsId,
    f: KundfakturaId,
) -> kundfaktura::Kundfakturarad {
    kundfaktura::lista(db, anna, id)
        .await
        .unwrap()
        .into_iter()
        .find(|r| r.id == f)
        .unwrap()
}

fn spärrkoder(fel: ExternFel) -> Vec<&'static str> {
    match fel {
        ExternFel::Spärrat(s) => s.iter().map(|s| s.kod).collect(),
        fel => panic!("{fel:?}"),
    }
}

#[tokio::test]
async fn registreringen_bokför_1510_mot_intäkt_och_moms_med_originalet_som_underlag() {
    let (db, anna, id, kund) = företag().await;
    let f = utkast(&db, &anna, id).await;
    // Originalet till ett utkast är inget okopplat underlag och kan inte tas bort.
    assert!(
        underlag::lista(&db, &anna, id, underlag::Urval::Okopplade)
            .await
            .unwrap()
            .is_empty()
    );
    let ext = extern_kundfaktura::fakturan(&db, &anna, id, f)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(ext.status, Status::Utkast);

    let r = extern_kundfaktura::registrera(&db, &klocka(), &anna, id, f, &formulär(kund, "F-17"))
        .await
        .unwrap();
    assert_eq!(
        rader(&db, &anna, id, r.verifikat.id).await,
        [
            (1510, TOTAL, 0),
            (3001, 0, 800_000),
            (3002, 0, 100_000),
            (2611, 0, 200_000),
            (2621, 0, 12_000),
        ]
    );
    let v = verifikat::verifikatet(&db, &anna, id, r.verifikat.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        v.datum,
        Some(date(2026, 3, 10)),
        "daterat på fakturadatumet"
    );
    assert_eq!(v.beskrivning, "Extern kundfaktura F-17, Kund AB");
    assert_eq!(v.underlag.len(), 1, "originalet är verifikatets underlag");
    assert_eq!(v.underlag[0].id, ext.underlag);
    let flöde = v.flöde.expect("länken till fakturan");
    assert_eq!(flöde.flöde, Flöde::Kundfaktura);
    assert_eq!(flöde.id.to_string(), f.to_string());
    assert_eq!(flöde.text, "extern kundfaktura F-17 till Kund AB");
    // Verifikatet rättas inte för sig, bara med Rätta registrering.
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

    // Reskontran: samma lista och samma regel för det som återstår som Mayas egna.
    let listan = listraden(&db, &anna, id, f).await;
    assert_eq!(listan.status, Status::Registrerad);
    assert!(listan.är_extern);
    assert_eq!(listan.nummer, None, "ingår inte i fakturaserien");
    assert_eq!(listan.fakturanummer, "F-17");
    assert_eq!((listan.total, listan.återstår), (TOTAL, TOTAL));
    assert_eq!(listan.kund, "Kund AB");
    assert_eq!(återstår(&db, &anna, id, f).await, TOTAL);

    let ext = extern_kundfaktura::fakturan(&db, &anna, id, f)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(ext.status, Status::Registrerad);
    assert_eq!(ext.total, Some(TOTAL));
    assert_eq!(ext.verifikat.map(|l| l.id), Some(r.verifikat.id));
    assert!(ext.registrerad.is_some());
    let historik = behandlingshistorik::läs(&db, &anna, id).await.unwrap();
    let post = historik
        .iter()
        .find(|p| p.typ == "extern kundfaktura registrerad")
        .expect("registreringen loggas");
    assert_eq!(post.beskrivning["fakturanummer"], "F-17");
    assert_eq!(post.belopp().unwrap().öre(), TOTAL);
    assert_eq!(
        post.text(),
        format!(
            "Extern faktura F-17 till ”Kund AB” daterad 2026-03-10, förfaller 2026-04-09, \
             11 120,00 kr, bokförd som {}",
            r.verifikat.nummer
        )
    );
    // Mayas flöden för egna fakturor rör inte den externa.
    let fel = kundfaktura::kundfakturan(&db, &anna, id, f)
        .await
        .unwrap_err();
    assert!(matches!(fel, KundfakturaFel::FinnsInte), "{fel:?}");
}

#[tokio::test]
async fn en_spärrad_registrering_bokför_ingenting_och_lämnar_utkastet() {
    let (db, anna, id, kund) = företag().await;
    let f = utkast(&db, &anna, id).await;
    let mut fel = formulär(kund, "F-17");
    fel.rader.push(rad("25", "5410", "100"));
    let svar = extern_kundfaktura::registrera(&db, &klocka(), &anna, id, f, &fel)
        .await
        .unwrap_err();
    assert_eq!(spärrkoder(svar), ["konto_inte_intakt"]);
    assert!(
        verifikat::lista(&db, &anna, id, verifikat::Urval::Alla)
            .await
            .unwrap()
            .is_empty(),
        "inget verifikat"
    );
    assert_eq!(listraden(&db, &anna, id, f).await.status, Status::Utkast);
}

#[tokio::test]
async fn ett_externt_nummer_som_redan_finns_spärras_och_skyddas_av_schemat() {
    let (db, anna, id, kund) = företag().await;
    registrerad(&db, &anna, id, kund).await;
    let andra = utkast(&db, &anna, id).await;
    // Ett utkast med samma nummer går att spara, men inte att registrera.
    extern_kundfaktura::spara_utkast(&db, &klocka(), &anna, id, andra, &formulär(kund, "f-17"))
        .await
        .unwrap();
    let fel =
        extern_kundfaktura::registrera(&db, &klocka(), &anna, id, andra, &formulär(kund, "f-17"))
            .await
            .unwrap_err();
    assert_eq!(spärrkoder(fel), ["dubblett"]);
    let fel = testhjalp::kör_sql(
        &db,
        "UPDATE kundfaktura SET fakturanummer = 'F-17', status = 'registrerad',
             kunduppgifter = '{}', netto = 1, moms = 0, total = 1,
             registrerad = '2026-10-18T08:00:00Z', registrerad_kanal = 'webb'
         WHERE status = 'utkast'",
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(fel.contains("UNIQUE"), "{fel}");
    extern_kundfaktura::registrera(&db, &klocka(), &anna, id, andra, &formulär(kund, "F-18"))
        .await
        .unwrap();
}

#[tokio::test]
async fn externa_fakturor_ingår_inte_i_fakturaserien() {
    let (db, anna, id, kund) = företag().await;
    let externa = registrerad(&db, &anna, id, kund).await;
    // Startnumret går fortfarande att välja, och Mayas första faktura får det.
    let mut eget = maya_domain::kundfaktura::NyKundfaktura {
        startnummer: "500".into(),
        ..Default::default()
    };
    eget.kund = kund.to_string();
    let utkast = kundfaktura::spara_utkast(&db, &klocka(), &anna, id, None, &eget)
        .await
        .unwrap();
    assert!(utkast != externa);
    let f = testhjalp::utfärdad_kundfaktura(&db, &anna, id, "Annan AB", "2026-03-11").await;
    assert_eq!(f.nummer, 500);
    // Ett externt utkast utfärdas inte som Mayas egen faktura.
    let annat = self::utkast(&db, &anna, id).await;
    let fel = kundfaktura::utfärda(&db, &klocka(), &anna, id, annat)
        .await
        .unwrap_err();
    assert!(matches!(fel, KundfakturaFel::IngetUtkast), "{fel:?}");
}

#[tokio::test]
async fn en_extern_faktura_betalas_och_skrivs_av_som_kundförlust_som_mayas_egna() {
    let (db, anna, id, kund) = företag().await;
    let f = registrerad(&db, &anna, id, kund).await;
    betala(&db, &anna, id, f, "1 120,00").await;
    assert_eq!(återstår(&db, &anna, id, f).await, 1_000_000);
    let rad = listraden(&db, &anna, id, f).await;
    assert_eq!(rad.återstår, 1_000_000);
    let idag = date(2026, 10, 18);
    assert_eq!(
        maya_domain::reskontra::kundfakturans_märke(
            rad.total,
            rad.krediterat,
            rad.återstår,
            rad.kundförlust,
            rad.förfallodag.unwrap(),
            idag
        ),
        maya_domain::reskontra::Märke::Förfallen
    );

    let b = kundforlust::bokför(
        &db,
        &klocka(),
        &anna,
        id,
        f,
        &NyKundförlust {
            skäl: "konkurs".into(),
            annat: String::new(),
            datum: "2026-09-30".into(),
        },
    )
    .await
    .unwrap();
    // 10 000 kr återstår: netto och moms i proportion till registreringens 8 000 + 1 000
    // netto och 2 000 + 120 moms.
    let v = b.verifikat.expect("fordran är bokförd");
    let r = rader(&db, &anna, id, v.id).await;
    assert_eq!(r.iter().map(|(_, d, _)| d).sum::<i64>(), 1_000_000);
    assert_eq!(r.last(), Some(&(1510, 0, 1_000_000)));
    assert!(r.iter().any(|(k, _, _)| *k == 6351));
    assert!(r.iter().any(|(k, _, _)| *k == 2611));
    let verifikatet = verifikat::verifikatet(&db, &anna, id, v.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        verifikatet.beskrivning,
        "Kundförlust, kundfaktura F-17, Kund AB"
    );
    assert_eq!(återstår(&db, &anna, id, f).await, 0);
    assert!(listraden(&db, &anna, id, f).await.kundförlust);
}

#[tokio::test]
async fn en_extern_kreditfaktura_registreras_och_minskar_det_som_återstår() {
    let (db, anna, id, kund) = företag().await;
    let f = registrerad(&db, &anna, id, kund).await;
    let k = extern_kundfaktura::kreditera(
        &db,
        &klocka(),
        &anna,
        id,
        f,
        ("K-4.pdf", "application/pdf", b"%PDF-1.4 kredit K-4"),
    )
    .await
    .unwrap();
    let kredit = |belopp: &str| NyExternFaktura {
        kund: String::new(),
        fakturanummer: "K-4".into(),
        fakturadatum: "2026-03-20".into(),
        förfallodag: String::new(),
        rader: vec![rad("25", "3001", belopp)],
        ..Default::default()
    };
    // Högst det som finns kvar: 11 120 kr.
    let fel = extern_kundfaktura::registrera(&db, &klocka(), &anna, id, k, &kredit("-9 000"))
        .await
        .unwrap_err();
    assert_eq!(spärrkoder(fel), ["kredit_overstiger"]);
    let fel = extern_kundfaktura::registrera(&db, &klocka(), &anna, id, k, &kredit("100"))
        .await
        .unwrap_err();
    assert_eq!(spärrkoder(fel), ["summa_inte_negativ"]);

    let r = extern_kundfaktura::registrera(&db, &klocka(), &anna, id, k, &kredit("-2 000"))
        .await
        .unwrap();
    assert_eq!(
        rader(&db, &anna, id, r.verifikat.id).await,
        [(1510, 0, 250_000), (3001, 200_000, 0), (2611, 50_000, 0)]
    );
    assert_eq!(återstår(&db, &anna, id, f).await, TOTAL - 250_000);
    let original = listraden(&db, &anna, id, f).await;
    assert_eq!(original.krediterat, 250_000);
    assert_eq!(original.återstår, TOTAL - 250_000);
    let kreditraden = listraden(&db, &anna, id, k).await;
    assert_eq!(kreditraden.krediterar.as_deref(), Some("F-17"));
    assert_eq!(kreditraden.återstår, 0, "räknas av mot originalet");
    assert_eq!(kreditraden.kund, "Kund AB", "originalets kund");
    let ext = extern_kundfaktura::fakturan(&db, &anna, id, f)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(ext.krediteras_av.len(), 1);
    assert_eq!(ext.krediteras_av[0].fakturanummer, "K-4");
    // Registreringen syns i originalets historik.
    assert!(
        ext.historik
            .iter()
            .any(|h| h.post.typ == "kreditfaktura registrerad"),
        "{:?}",
        ext.historik
    );
    // En kreditfaktura betalas inte, och originalet kan inte rättas med kreditfakturor kvar.
    let fel = betalning::registrera(
        &db,
        &klocka(),
        &anna,
        id,
        Faktura::Kund(k),
        &NyBetalning {
            betaldag: "2026-04-01".into(),
            belopp: "1".into(),
            konto: "1930".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(fel, BetalningFel::FinnsInte), "{fel:?}");
    let fel = extern_kundfaktura::rätta_registrering(&db, &klocka(), &anna, id, f, "")
        .await
        .unwrap_err();
    assert!(matches!(fel, ExternFel::HarKreditfakturor), "{fel:?}");
}

#[tokio::test]
async fn rätta_registrering_vänder_verifikatet_och_låser_upp_fakturan_med_betalningarna_kvar() {
    let (db, anna, id, kund) = företag().await;
    let f = registrerad(&db, &anna, id, kund).await;
    let första = extern_kundfaktura::fakturan(&db, &anna, id, f)
        .await
        .unwrap()
        .unwrap()
        .verifikat
        .unwrap();
    betala(&db, &anna, id, f, "5 000").await;

    let r = extern_kundfaktura::rätta_registrering(&db, &klocka(), &anna, id, f, "fel belopp")
        .await
        .unwrap();
    let rättelse = r.rättelse.expect("fordran var bokförd");
    let v = verifikat::verifikatet(&db, &anna, id, rättelse.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(v.rättar.map(|l| l.id), Some(första.id));
    assert_eq!(v.datum, Some(date(2026, 10, 18)), "daterad i dag");
    assert_eq!(
        rader(&db, &anna, id, rättelse.id).await,
        [
            (1510, 0, TOTAL),
            (3001, 800_000, 0),
            (3002, 100_000, 0),
            (2611, 200_000, 0),
            (2621, 12_000, 0),
        ]
    );
    assert_eq!(
        v.flöde.map(|l| l.id.to_string()),
        Some(f.to_string()),
        "rättelsen länkar till fakturan"
    );

    // Upplåst: betalningen ligger kvar, men fakturan tar inte emot nya.
    let ext = extern_kundfaktura::fakturan(&db, &anna, id, f)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(ext.status, Status::Upplåst);
    assert_eq!(ext.rättelser.len(), 1);
    assert_eq!(ext.rättelser[0].orsak, "fel belopp");
    let post = betalning::reskontrapost(&db, &anna, id, Faktura::Kund(f))
        .await
        .unwrap();
    assert_eq!(post.betalningar.len(), 1, "betalningen ligger kvar");
    let fel = betalning::registrera(
        &db,
        &klocka(),
        &anna,
        id,
        Faktura::Kund(f),
        &NyBetalning {
            betaldag: "2026-04-01".into(),
            belopp: "1".into(),
            konto: "1930".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(fel, BetalningFel::FinnsInte), "{fel:?}");
    let raden = listraden(&db, &anna, id, f).await;
    assert_eq!(raden.status, Status::Upplåst);
    assert!(!raden.status.i_reskontran());
    let fel = extern_kundfaktura::radera_utkast(&db, &klocka(), &anna, id, f)
        .await
        .unwrap_err();
    assert!(
        matches!(fel, ExternFel::IngetUtkast),
        "en upplåst raderas inte"
    );

    // Registreras om: inte under det som redan betalats, och med ett nytt verifikat.
    let mut nytt = formulär(kund, "F-17");
    nytt.rader = vec![rad("25", "3001", "3 000")];
    let fel = extern_kundfaktura::registrera(&db, &klocka(), &anna, id, f, &nytt)
        .await
        .unwrap_err();
    assert_eq!(spärrkoder(fel), ["under_betalt"]);
    nytt.rader = vec![rad("25", "3001", "9 000")];
    let om = extern_kundfaktura::registrera(&db, &klocka(), &anna, id, f, &nytt)
        .await
        .unwrap();
    assert_ne!(om.verifikat.id, första.id);
    assert_eq!(
        rader(&db, &anna, id, om.verifikat.id).await,
        [(1510, 1_125_000, 0), (3001, 0, 900_000), (2611, 0, 225_000)]
    );
    assert_eq!(återstår(&db, &anna, id, f).await, 1_125_000 - 500_000);
    let post = betalning::reskontrapost(&db, &anna, id, Faktura::Kund(f))
        .await
        .unwrap();
    assert_eq!(post.betalningar.len(), 1, "betalningen är kvar");

    let typer: Vec<String> = behandlingshistorik::läs(&db, &anna, id)
        .await
        .unwrap()
        .into_iter()
        .map(|p| p.typ)
        .collect();
    assert!(
        typer.contains(&"registrering rättad".to_string()),
        "{typer:?}"
    );
    let historik = behandlingshistorik::läs(&db, &anna, id).await.unwrap();
    let rättad = historik
        .iter()
        .find(|p| p.typ == "registrering rättad")
        .unwrap();
    assert_eq!(
        rättad.text(),
        format!(
            "Registreringen av faktura F-17 till ”Kund AB” rättad med {} av {}, fel belopp, \
             fakturan är upplåst",
            rättelse.nummer,
            första.nummer.unwrap()
        )
    );
    let omregistrerad = historik
        .iter()
        .rfind(|p| p.typ == "extern kundfaktura registrerad")
        .unwrap();
    assert_eq!(omregistrerad.beskrivning["omregistrerad"], true);
}

#[tokio::test]
async fn rätta_registrering_spärras_när_en_kundförlust_gäller() {
    let (db, anna, id, kund) = företag().await;
    let f = registrerad(&db, &anna, id, kund).await;
    kundforlust::bokför(
        &db,
        &klocka(),
        &anna,
        id,
        f,
        &NyKundförlust {
            skäl: "konkurs".into(),
            annat: String::new(),
            datum: "2026-09-30".into(),
        },
    )
    .await
    .unwrap();
    let fel = extern_kundfaktura::rätta_registrering(&db, &klocka(), &anna, id, f, "")
        .await
        .unwrap_err();
    assert!(matches!(fel, ExternFel::HarKundförlust), "{fel:?}");
}

#[tokio::test]
async fn triggrarna_släpper_bara_igenom_upplåsningen_och_de_upplåsta_fälten() {
    let (db, anna, id, kund) = företag().await;
    let f = registrerad(&db, &anna, id, kund).await;
    for sql in [
        "UPDATE kundfaktura SET total = 1",
        "UPDATE kundfaktura SET fakturanummer = 'F-99'",
        "UPDATE kundfaktura SET status = 'upplast'",
        "UPDATE kundfaktura SET status = 'utkast'",
        "DELETE FROM kundfaktura",
        "UPDATE kundfakturarad SET netto = 1",
        "DELETE FROM kundfakturarad",
        "INSERT INTO kundfakturarad (faktura_id, radnr, beskrivning, enhet, momssats)
         SELECT id, 9, '', '', 25 FROM kundfaktura",
    ] {
        let fel = testhjalp::kör_sql(&db, sql).await.unwrap_err().to_string();
        assert!(fel.contains("kundfaktura går inte att"), "{sql}: {fel}");
    }
    extern_kundfaktura::rätta_registrering(&db, &klocka(), &anna, id, f, "")
        .await
        .unwrap();
    // Upplåst: avskriften och raderna ändras, men inte originalet, företaget, skaparen eller
    // registreringen, och fakturan blir inget utkast eller raderas.
    for sql in [
        "UPDATE kundfaktura SET fakturanummer = 'F-17b', total = 2",
        "UPDATE kundfakturarad SET netto = 1",
        "INSERT INTO kundfakturarad (faktura_id, radnr, beskrivning, enhet, momssats)
         SELECT id, 9, '', '', 25 FROM kundfaktura",
    ] {
        testhjalp::kör_sql(&db, sql).await.unwrap();
    }
    for sql in [
        "UPDATE kundfaktura SET underlag_id = underlag_id + 100",
        "UPDATE kundfaktura SET status = 'utkast'",
        "UPDATE kundfaktura SET extern = 0",
        "UPDATE kundfaktura SET skapad = '2020-01-01T00:00:00Z'",
        "UPDATE kundfaktura SET registrerad = '2020-01-01T00:00:00Z'",
        "UPDATE kundfaktura SET verifikat_id = NULL",
    ] {
        let fel = testhjalp::kör_sql(&db, sql).await.unwrap_err().to_string();
        assert!(fel.contains("upplåst kundfaktura"), "{sql}: {fel}");
    }
    let fel = testhjalp::kör_sql(&db, "DELETE FROM kundfaktura")
        .await
        .unwrap_err()
        .to_string();
    assert!(fel.contains("går inte att radera"), "{fel}");
    // Att registrera om med det rättade verifikatet avvisas.
    let fel = testhjalp::kör_sql(&db, "UPDATE kundfaktura SET status = 'registrerad'")
        .await
        .unwrap_err()
        .to_string();
    assert!(fel.contains("upplåst kundfaktura"), "{fel}");
    let fel = testhjalp::kör_sql(&db, "DELETE FROM kundfaktura_registrering")
        .await
        .unwrap_err()
        .to_string();
    assert!(fel.contains("rättad registrering"), "{fel}");
}

#[tokio::test]
async fn ett_raderat_utkast_lämnar_originalet_som_ett_okopplat_underlag() {
    let (db, anna, id, kund) = företag().await;
    let f = utkast(&db, &anna, id).await;
    extern_kundfaktura::spara_utkast(&db, &klocka(), &anna, id, f, &formulär(kund, "F-17"))
        .await
        .unwrap();
    assert_eq!(listraden(&db, &anna, id, f).await.total, TOTAL);
    extern_kundfaktura::radera_utkast(&db, &klocka(), &anna, id, f)
        .await
        .unwrap();
    assert!(
        extern_kundfaktura::fakturan(&db, &anna, id, f)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        underlag::lista(&db, &anna, id, underlag::Urval::Okopplade)
            .await
            .unwrap()
            .len(),
        1
    );
    let historik = behandlingshistorik::läs(&db, &anna, id).await.unwrap();
    let raderat = historik
        .iter()
        .find(|p| p.typ == "fakturautkast raderat")
        .unwrap();
    assert_eq!(raderat.beskrivning["sort"], "extern kundfaktura");
    assert_eq!(
        raderat.text(),
        "Utkast till extern kundfaktura, nummer ”F-17”, till ”Kund AB”, 2 rader"
    );
}

#[tokio::test]
async fn en_förfallen_extern_faktura_står_i_att_göra_tills_den_låses_upp() {
    let (db, anna, id, kund) = företag().await;
    let f = registrerad(&db, &anna, id, kund).await;
    betala(&db, &anna, id, f, "120").await;
    let fakturorna = |a: maya_core::att_gora::AttGöra| {
        a.saker
            .into_iter()
            .filter_map(|s| match s {
                maya_core::att_gora::Sak::Faktura(f) => Some(f),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    let a = maya_core::att_gora::att_göra(&db, &klocka(), &testhjalp::config(), &anna, id)
        .await
        .unwrap();
    let saker = fakturorna(a);
    assert_eq!(saker.len(), 1);
    assert_eq!(
        saker[0].faktura,
        maya_core::att_gora::Reskontrafaktura::Kund(f)
    );
    assert_eq!(saker[0].nummer, "F-17");
    assert_eq!((saker[0].total, saker[0].återstår), (TOTAL, TOTAL - 12_000));

    extern_kundfaktura::rätta_registrering(&db, &klocka(), &anna, id, f, "")
        .await
        .unwrap();
    let a = maya_core::att_gora::att_göra(&db, &klocka(), &testhjalp::config(), &anna, id)
        .await
        .unwrap();
    assert!(
        fakturorna(a).is_empty(),
        "en upplåst faktura är inte förfallen"
    );
}

// #153 Ny kund från det externa utkastet

fn med_ny_kund(kund: KundId) -> NyExternFaktura {
    NyExternFaktura {
        kund: maya_domain::kundfaktura::NY.into(),
        ny_kund: maya_domain::kund::NyKund {
            namn: "Nya Kunden AB".into(),
            betalningsvillkor: "10".into(),
            ..Default::default()
        },
        förfallodag: String::new(),
        ..formulär(kund, "F-17")
    }
}

async fn registret(db: &Databas, anna: &Aktör, id: FöretagsId) -> Vec<String> {
    maya_core::kund::kunder(db, anna, id)
        .await
        .unwrap()
        .into_iter()
        .map(|k| k.namn)
        .collect()
}

#[tokio::test]
async fn en_ny_kund_från_det_externa_utkastet_blir_verklig_när_fakturan_registreras() {
    let (db, anna, id, kund) = företag().await;
    let f = utkast(&db, &anna, id).await;
    extern_kundfaktura::spara_utkast(&db, &klocka(), &anna, id, f, &med_ny_kund(kund))
        .await
        .unwrap();

    // Den nya kunden hör till utkastet och syns varken i registret eller i väljaren.
    assert_eq!(registret(&db, &anna, id).await, ["Kund AB"]);
    let fö = extern_kundfaktura::förutsättningar(&db, &anna, id, Some(f))
        .await
        .unwrap();
    assert_eq!(fö.kunder.len(), 1);
    let sparat = extern_kundfaktura::fakturan(&db, &anna, id, f)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(sparat.kund, "Nya Kunden AB");
    assert_eq!(sparat.formulär.kund, maya_domain::kundfaktura::NY);
    assert_eq!(sparat.formulär.ny_kund.betalningsvillkor, "10");
    assert!(listraden(&db, &anna, id, f).await.ny_kund);

    extern_kundfaktura::registrera(&db, &klocka(), &anna, id, f, &sparat.formulär)
        .await
        .unwrap();

    assert_eq!(
        registret(&db, &anna, id).await,
        ["Kund AB", "Nya Kunden AB"]
    );
    let rad = listraden(&db, &anna, id, f).await;
    assert!(!rad.ny_kund);
    assert_eq!(rad.kund, "Nya Kunden AB");
    assert_eq!(
        rad.förfallodag.map(|d| d.to_string()).as_deref(),
        Some("2026-03-20"),
        "den nya kundens villkor"
    );
    let poster = behandlingshistorik::läs(&db, &anna, id).await.unwrap();
    let skapad = poster
        .iter()
        .find(|p| p.typ == "kund skapad" && p.objekt == "Nya Kunden AB")
        .unwrap();
    assert_eq!(skapad.aktör, anna);
    let registrerad = poster
        .iter()
        .find(|p| p.typ == "extern kundfaktura registrerad")
        .unwrap();
    assert_eq!(registrerad.beskrivning["kund"], skapad.beskrivning["id"]);
}

#[tokio::test]
async fn en_ny_kund_raderas_med_det_externa_utkastet_loggat_med_maya_som_aktör() {
    let (db, anna, id, kund) = företag().await;
    let f = utkast(&db, &anna, id).await;
    extern_kundfaktura::spara_utkast(&db, &klocka(), &anna, id, f, &med_ny_kund(kund))
        .await
        .unwrap();
    // Att spara igen skriver om samma nya kund.
    extern_kundfaktura::spara_utkast(&db, &klocka(), &anna, id, f, &med_ny_kund(kund))
        .await
        .unwrap();
    extern_kundfaktura::radera_utkast(&db, &klocka(), &anna, id, f)
        .await
        .unwrap();
    let poster = behandlingshistorik::läs(&db, &anna, id).await.unwrap();
    let raderade: Vec<_> = poster.iter().filter(|p| p.typ == "kund raderad").collect();
    assert_eq!(raderade.len(), 1);
    assert_eq!(raderade[0].aktör, Aktör::maya());
    assert_eq!(raderade[0].objekt, "Nya Kunden AB");
    let radering = poster
        .iter()
        .find(|p| p.typ == "fakturautkast raderat")
        .unwrap();
    assert_eq!(radering.beskrivning["kundnamn"], "Nya Kunden AB");
    assert_eq!(radering.beskrivning["ny_kund"], true);
    assert_eq!(registret(&db, &anna, id).await, ["Kund AB"]);

    // Ett utkast som byter till en kund i registret släpper den nya, som raderas.
    let g = utkast(&db, &anna, id).await;
    extern_kundfaktura::spara_utkast(&db, &klocka(), &anna, id, g, &med_ny_kund(kund))
        .await
        .unwrap();
    extern_kundfaktura::spara_utkast(&db, &klocka(), &anna, id, g, &formulär(kund, "F-18"))
        .await
        .unwrap();
    let raderade = behandlingshistorik::läs(&db, &anna, id)
        .await
        .unwrap()
        .into_iter()
        .filter(|p| p.typ == "kund raderad")
        .count();
    assert_eq!(raderade, 2);
}

#[tokio::test]
async fn en_ny_kund_som_inte_klarar_registrets_regler_spärrar_det_externa_utkastet() {
    let (db, anna, id, kund) = företag().await;
    let f = utkast(&db, &anna, id).await;
    let mut fel = med_ny_kund(kund);
    fel.ny_kund.epost = "inte en adress".into();
    let e = extern_kundfaktura::spara_utkast(&db, &klocka(), &anna, id, f, &fel)
        .await
        .unwrap_err();
    assert_eq!(spärrkoder(e), ["ny_kund"]);
    assert_eq!(registret(&db, &anna, id).await, ["Kund AB"]);
}
