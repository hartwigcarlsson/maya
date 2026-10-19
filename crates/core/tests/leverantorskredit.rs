//! Kreditfaktura från leverantör och Rätta avskrift genom `core` mot en tillfällig databas
//! (#148): kreditfakturans verifikat speglar registreringen och minskar det som återstår,
//! kredit av en betald faktura ger ett tillgodohavande som återbetalas, och Rätta avskrift
//! vänder registreringen, låser upp fakturan med betalningarna kvar och registreras om med samma
//! regler. Triggrarna släpper bara igenom upplåsningen och avskriftens fält.

use maya_core::betalning::{self, BetalningFel, Faktura};
use maya_core::leverantor::LeverantörsId;
use maya_core::leverantorsfaktura::{self, FakturaFel, LeverantörsfakturaId, Status};
use maya_core::verifikat::{self, Flöde, Rättelseval, VerifikatFel, VerifikatId};
use maya_core::{Databas, Klocka, behandlingshistorik, testhjalp};
use maya_core::{aktor::Aktör, foretag::FöretagsId};
use maya_domain::betalning::NyBetalning;
use maya_domain::leverantorsfaktura::{NyLeverantörsfaktura, NyRad};

const NU: &str = "2026-10-19T08:00:00Z";

fn klocka() -> Klocka {
    Klocka::fast(NU.parse().unwrap())
}

/// Originalets total: 1 000 kr netto och 25 % moms.
const TOTAL: i64 = 125_000;

/// Ett företag, leverantören Kontorsvaror AB och dess registrerade faktura 8812 på 1 250 kr.
async fn original(db: &Databas) -> (Aktör, FöretagsId, LeverantörsId, LeverantörsfakturaId) {
    let (anna, id) = testhjalp::företag(db).await;
    let l = testhjalp::leverantör(db, &anna, id, "Kontorsvaror AB").await;
    let f = testhjalp::registrerad_leverantörsfaktura(db, &anna, id, l, "8812", "").await;
    (anna, id, l, f)
}

/// Kreditfakturan K-17 på `total` kr med en rad på 5410 netto `netto` kr och 25 % moms, med
/// hänvisning till `krediterar` om den finns.
fn kredit(
    l: LeverantörsId,
    krediterar: Option<LeverantörsfakturaId>,
    total: &str,
    netto: &str,
) -> NyLeverantörsfaktura {
    NyLeverantörsfaktura {
        leverantör: l.to_string(),
        fakturanummer: "K-17".into(),
        fakturadatum: "2026-03-20".into(),
        förfallodag: "2026-04-19".into(),
        totalbelopp: total.into(),
        rader: vec![NyRad {
            konto: "5410".into(),
            netto: netto.into(),
            momssats: "25".into(),
            moms: String::new(),
        }],
        krediterar: krediterar.map(|k| k.to_string()).unwrap_or_default(),
        ..Default::default()
    }
}

/// Laddar upp kreditfakturans original och registrerar den.
async fn registrera_kredit(
    db: &Databas,
    anna: &Aktör,
    id: FöretagsId,
    formulär: &NyLeverantörsfaktura,
) -> (LeverantörsfakturaId, VerifikatId) {
    let (k, _) = leverantorsfaktura::ladda_upp(
        db,
        &klocka(),
        anna,
        id,
        (
            "kredit-k17.pdf",
            "application/pdf",
            b"%PDF-1.4 kreditfaktura K-17",
        ),
    )
    .await
    .unwrap();
    let r = leverantorsfaktura::registrera(db, &klocka(), anna, id, k, formulär)
        .await
        .unwrap();
    (k, r.verifikat.as_ref().unwrap().id)
}

fn betalning(betaldag: &str, belopp: &str) -> NyBetalning {
    NyBetalning {
        betaldag: betaldag.into(),
        belopp: belopp.into(),
        konto: "1930".into(),
        ..Default::default()
    }
}

async fn betala(
    db: &Databas,
    anna: &Aktör,
    id: FöretagsId,
    f: LeverantörsfakturaId,
    belopp: &str,
) -> betalning::Registrerad {
    betalning::registrera(
        db,
        &klocka(),
        anna,
        id,
        Faktura::Leverantör(f),
        &betalning("2026-04-10", belopp),
    )
    .await
    .unwrap()
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

async fn fakturan(
    db: &Databas,
    anna: &Aktör,
    id: FöretagsId,
    f: LeverantörsfakturaId,
) -> leverantorsfaktura::Leverantörsfaktura {
    leverantorsfaktura::fakturan(db, anna, id, f)
        .await
        .unwrap()
        .unwrap()
}

/// Originalet 8812 som formulär, med totalen `total` och nettot `netto` på raden.
fn avskrift(l: LeverantörsId, total: &str, netto: &str) -> NyLeverantörsfaktura {
    NyLeverantörsfaktura {
        leverantör: l.to_string(),
        fakturanummer: "8812".into(),
        fakturadatum: "2026-03-14".into(),
        förfallodag: "2026-04-13".into(),
        referens: "4455667788".into(),
        totalbelopp: total.into(),
        rader: vec![NyRad {
            konto: "5410".into(),
            netto: netto.into(),
            momssats: "25".into(),
            moms: String::new(),
        }],
        ..Default::default()
    }
}

fn spärrkoder(fel: &FakturaFel) -> Vec<&'static str> {
    match fel {
        FakturaFel::Spärrat(s) => s.iter().map(|s| s.kod).collect(),
        fel => panic!("väntade spärrar, fick {fel:?}"),
    }
}

#[tokio::test]
async fn en_kreditfaktura_med_hänvisning_speglar_registreringen_och_minskar_det_som_återstår() {
    let db = Databas::tillfällig().await;
    let (anna, id, l, f) = original(&db).await;
    let (k, v) = registrera_kredit(&db, &anna, id, &kredit(l, Some(f), "-250", "-200")).await;
    assert_eq!(
        rader(&db, &anna, id, v).await,
        [(5410, 0, 20_000), (2641, 0, 5_000), (2440, 25_000, 0)]
    );
    let verifikatet = verifikat::verifikatet(&db, &anna, id, v)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        verifikatet.beskrivning,
        "Kreditfaktura K-17, Kontorsvaror AB"
    );
    let flöde = verifikatet.flöde.expect("länken till kreditfakturan");
    assert_eq!(flöde.flöde, Flöde::Leverantörsfaktura);
    assert_eq!(flöde.id.to_string(), k.to_string());

    // Kreditfakturan räknas av mot originalet och har inget kvar själv.
    assert_eq!(återstår_i_listan(&db, &anna, id, f).await, TOTAL - 25_000);
    assert_eq!(återstår_i_listan(&db, &anna, id, k).await, 0);
    let post = betalning::reskontrapost(&db, &anna, id, Faktura::Leverantör(f))
        .await
        .unwrap();
    assert_eq!((post.krediterat, post.återstår), (25_000, TOTAL - 25_000));
    let förutsättningar = betalning::förutsättningar(&db, &anna, id, Faktura::Leverantör(f))
        .await
        .unwrap();
    assert_eq!(förutsättningar.återstår, TOTAL - 25_000);

    // Hänvisningen syns åt båda hållen.
    let kreditfakturan = fakturan(&db, &anna, id, k).await;
    assert_eq!(kreditfakturan.krediterar.as_ref().map(|o| o.id), Some(f));
    assert_eq!(kreditfakturan.formulär.krediterar, f.to_string());
    assert_eq!(kreditfakturan.formulär.totalbelopp, "-250,00");
    assert_eq!(kreditfakturan.formulär.rader[0].netto, "-200,00");
    let originalet = fakturan(&db, &anna, id, f).await;
    assert_eq!(
        originalet
            .krediteras_av
            .iter()
            .map(|o| (o.id, o.fakturanummer.as_str(), o.total))
            .collect::<Vec<_>>(),
        [(k, "K-17", -25_000)]
    );

    let historik = behandlingshistorik::läs(&db, &anna, id).await.unwrap();
    let post = historik
        .iter()
        .find(|p| p.typ == "kreditfaktura registrerad")
        .expect("kreditfakturan loggas");
    assert_eq!(post.beskrivning["krediterar"]["fakturanummer"], "8812");
    assert!(
        post.text().contains("krediterar faktura 8812"),
        "{}",
        post.text()
    );
}

#[tokio::test]
async fn kredit_av_en_betald_faktura_ger_ett_tillgodohavande_som_återbetalas_mot_2440() {
    let db = Databas::tillfällig().await;
    let (anna, id, l, f) = original(&db).await;
    betala(&db, &anna, id, f, "1 250").await;
    registrera_kredit(&db, &anna, id, &kredit(l, Some(f), "-250", "-200")).await;
    assert_eq!(återstår_i_listan(&db, &anna, id, f).await, -25_000);

    // Återbetalningen är en betalning åt andra hållet: betalkontot mot 2440.
    let r = betala(&db, &anna, id, f, "250").await;
    assert_eq!(r.återstår, 0);
    assert_eq!(
        rader(&db, &anna, id, r.verifikat.id).await,
        [(2440, 0, 25_000), (1930, 25_000, 0)]
    );
    let v = verifikat::verifikatet(&db, &anna, id, r.verifikat.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        v.beskrivning,
        "Återbetalning av tillgodohavande på leverantörsfaktura 8812, Kontorsvaror AB"
    );
    assert_eq!(återstår_i_listan(&db, &anna, id, f).await, 0);
    let historik = behandlingshistorik::läs(&db, &anna, id).await.unwrap();
    assert!(historik.iter().any(|p| p.typ == "återbetalning"));

    // Mer än tillgodohavandet återbetalas inte.
    registrera_kredit_med_nummer(&db, &anna, id, l, f, "K-18").await;
    let fel = betalning::registrera(
        &db,
        &klocka(),
        &anna,
        id,
        Faktura::Leverantör(f),
        &betalning("2026-04-10", "300"),
    )
    .await
    .unwrap_err();
    assert!(
        matches!(&fel, BetalningFel::Spärrat(s) if s[0].kod == "overaterbetalning"),
        "{fel:?}"
    );
}

/// En kreditfaktura på 250 kr till med numret `nummer`.
async fn registrera_kredit_med_nummer(
    db: &Databas,
    anna: &Aktör,
    id: FöretagsId,
    l: LeverantörsId,
    f: LeverantörsfakturaId,
    nummer: &str,
) {
    let formulär = NyLeverantörsfaktura {
        fakturanummer: nummer.into(),
        ..kredit(l, Some(f), "-250", "-200")
    };
    let (k, _) = leverantorsfaktura::ladda_upp(
        db,
        &klocka(),
        anna,
        id,
        ("kredit.pdf", "application/pdf", nummer.as_bytes()),
    )
    .await
    .unwrap();
    leverantorsfaktura::registrera(db, &klocka(), anna, id, k, &formulär)
        .await
        .unwrap();
}

#[tokio::test]
async fn en_kreditfaktura_utan_hänvisning_är_ett_tillgodohavande_och_en_med_hänvisning_betalas_inte()
 {
    let db = Databas::tillfällig().await;
    let (anna, id, l, f) = original(&db).await;
    let (fristående, _) = registrera_kredit(&db, &anna, id, &kredit(l, None, "-250", "-200")).await;
    assert_eq!(återstår_i_listan(&db, &anna, id, fristående).await, -25_000);
    assert_eq!(återstår_i_listan(&db, &anna, id, f).await, TOTAL);
    let r = betala(&db, &anna, id, fristående, "250").await;
    assert_eq!(r.återstår, 0);

    registrera_kredit_med_nummer(&db, &anna, id, l, f, "K-18").await;
    let k18 = leverantorsfaktura::lista(&db, &anna, id)
        .await
        .unwrap()
        .into_iter()
        .find(|r| r.fakturanummer == "K-18")
        .unwrap();
    assert!(k18.kreditfaktura);
    let fel = betalning::registrera(
        &db,
        &klocka(),
        &anna,
        id,
        Faktura::Leverantör(k18.id),
        &betalning("2026-04-10", "250"),
    )
    .await
    .unwrap_err();
    assert!(
        matches!(&fel, BetalningFel::Spärrat(s) if s[0].kod == "redan_betald"),
        "{fel:?}"
    );
}

#[tokio::test]
async fn en_kreditfaktura_hänvisar_bara_till_samma_leverantörs_registrerade_faktura() {
    let db = Databas::tillfällig().await;
    let (anna, id, _, f) = original(&db).await;
    let annan = testhjalp::leverantör(&db, &anna, id, "Tryckeriet AB").await;
    let (k, _) = leverantorsfaktura::ladda_upp(
        &db,
        &klocka(),
        &anna,
        id,
        ("kredit.pdf", "application/pdf", b"%PDF-1.4 annan"),
    )
    .await
    .unwrap();
    let fel = leverantorsfaktura::registrera(
        &db,
        &klocka(),
        &anna,
        id,
        k,
        &kredit(annan, Some(f), "-250", "-200"),
    )
    .await
    .unwrap_err();
    assert_eq!(spärrkoder(&fel), ["krediterar_okand"]);
}

#[tokio::test]
async fn rätta_avskrift_vänder_registreringen_och_låser_upp_fakturan_med_betalningarna_kvar() {
    let db = Databas::tillfällig().await;
    let (anna, id, _, f) = original(&db).await;
    betala(&db, &anna, id, f, "500").await;
    let registrering = fakturan(&db, &anna, id, f).await.verifikat.unwrap().0;

    let r = leverantorsfaktura::rätta_avskrift(&db, &klocka(), &anna, id, f, "")
        .await
        .unwrap();
    let rättelse = r.rättelse.expect("registreringen var bokförd");
    assert_eq!(
        rader(&db, &anna, id, rättelse.id).await,
        [(5410, 0, 100_000), (2641, 0, 25_000), (2440, TOTAL, 0)]
    );
    let v = verifikat::verifikatet(&db, &anna, id, rättelse.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(v.rättar.map(|r| r.id), Some(registrering));
    assert_eq!(v.datum, Some(jiff::civil::date(2026, 10, 19)));
    // Rättelsen länkar till fakturan och rättas inte som ett vanligt verifikat.
    assert_eq!(v.flöde.map(|l| l.id.to_string()), Some(f.to_string()));
    let fel = verifikat::rätta(
        &db,
        &klocka(),
        &anna,
        id,
        rättelse.id,
        "2026-10-19",
        "",
        Rättelseval::BaraRättelse,
    )
    .await
    .unwrap_err();
    assert!(matches!(fel, VerifikatFel::InteRättbart(_)), "{fel:?}");

    let upplåst = fakturan(&db, &anna, id, f).await;
    assert_eq!(upplåst.status, Status::Upplåst);
    assert_eq!(upplåst.avskrifter.len(), 1);
    assert_eq!(
        upplåst.avskrifter[0].rättelse.as_ref().map(|l| l.id),
        Some(rättelse.id)
    );
    // Betalningarna ligger kvar och det som återstår är detsamma.
    let post = betalning::reskontrapost(&db, &anna, id, Faktura::Leverantör(f))
        .await
        .unwrap();
    assert_eq!(post.betalningar.len(), 1);
    assert_eq!(återstår_i_listan(&db, &anna, id, f).await, TOTAL - 50_000);
    // En upplåst faktura tar inte emot betalningar, raderas inte och rättas inte igen.
    let fel = betalning::registrera(
        &db,
        &klocka(),
        &anna,
        id,
        Faktura::Leverantör(f),
        &betalning("2026-04-10", "100"),
    )
    .await
    .unwrap_err();
    assert!(matches!(fel, BetalningFel::FinnsInte), "{fel:?}");
    assert!(matches!(
        leverantorsfaktura::radera_utkast(&db, &klocka(), &anna, id, f).await,
        Err(FakturaFel::IngetUtkast)
    ));
    assert!(matches!(
        leverantorsfaktura::rätta_avskrift(&db, &klocka(), &anna, id, f, "").await,
        Err(FakturaFel::InteRegistrerad)
    ));

    let historik = behandlingshistorik::läs(&db, &anna, id).await.unwrap();
    let post = historik
        .iter()
        .find(|p| p.typ == "avskrift rättad")
        .expect("upplåsningen loggas");
    assert_eq!(
        post.beskrivning["leverantörsfaktura"],
        f.to_string().parse::<i64>().unwrap()
    );
    assert!(post.text().contains("upplåst"), "{}", post.text());
}

#[tokio::test]
async fn en_upplåst_faktura_registreras_om_med_samma_regler_och_ett_nytt_verifikat() {
    let db = Databas::tillfällig().await;
    let (anna, id, l, f) = original(&db).await;
    betala(&db, &anna, id, f, "500").await;
    let första = fakturan(&db, &anna, id, f).await.verifikat.unwrap().0;
    leverantorsfaktura::rätta_avskrift(&db, &klocka(), &anna, id, f, "Fel belopp")
        .await
        .unwrap();

    // Utkastet sparas med upplåst status kvar.
    leverantorsfaktura::spara_utkast(&db, &klocka(), &anna, id, f, &avskrift(l, "1 500", "1 200"))
        .await
        .unwrap();
    assert_eq!(fakturan(&db, &anna, id, f).await.status, Status::Upplåst);

    // Spärren mot en total under det som redan har betalats.
    let fel =
        leverantorsfaktura::registrera(&db, &klocka(), &anna, id, f, &avskrift(l, "400", "320"))
            .await
            .unwrap_err();
    assert_eq!(spärrkoder(&fel), ["under_betalt"]);

    let r = leverantorsfaktura::registrera(
        &db,
        &klocka(),
        &anna,
        id,
        f,
        &avskrift(l, "1 500", "1 200"),
    )
    .await
    .unwrap();
    assert_ne!(r.verifikat.as_ref().unwrap().id, första);
    assert_eq!(
        rader(&db, &anna, id, r.verifikat.as_ref().unwrap().id).await,
        [(5410, 120_000, 0), (2641, 30_000, 0), (2440, 0, 150_000)]
    );
    let omregistrerad = fakturan(&db, &anna, id, f).await;
    assert_eq!(omregistrerad.status, Status::Registrerad);
    assert_eq!(
        omregistrerad.verifikat.map(|v| v.0),
        Some(r.verifikat.as_ref().unwrap().id)
    );
    assert_eq!(återstår_i_listan(&db, &anna, id, f).await, 150_000 - 50_000);
    // Den första registreringen länkar fortfarande till fakturan.
    let v = verifikat::verifikatet(&db, &anna, id, första)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(v.flöde.map(|l| l.id.to_string()), Some(f.to_string()));
    // Den nya registreringen kan rättas i sin tur.
    leverantorsfaktura::rätta_avskrift(&db, &klocka(), &anna, id, f, "")
        .await
        .unwrap();
    assert_eq!(fakturan(&db, &anna, id, f).await.avskrifter.len(), 2);
}

#[tokio::test]
async fn databasen_släpper_bara_igenom_upplåsningen_och_avskriftens_fält() {
    let db = Databas::tillfällig().await;
    let (anna, id, _, f) = original(&db).await;
    // En registrerad faktura blir inte upplåst utan en rättelse av registreringen, och ändras
    // inte.
    for sql in [
        "UPDATE leverantorsfaktura SET status = 'upplast'",
        "UPDATE leverantorsfaktura SET fakturanummer = '9999'",
        "UPDATE leverantorsfakturarad SET netto = 1",
        "DELETE FROM leverantorsfaktura",
    ] {
        let fel = testhjalp::kör_sql(&db, sql).await.unwrap_err();
        assert!(fel.to_string().contains("går inte att"), "{sql}: {fel}");
    }

    leverantorsfaktura::rätta_avskrift(&db, &klocka(), &anna, id, f, "")
        .await
        .unwrap();
    // Avskriftens fält och raderna skrivs av fritt på en upplåst faktura.
    testhjalp::kör_sql(
        &db,
        "UPDATE leverantorsfaktura SET fakturanummer = '8812-2'",
    )
    .await
    .unwrap();
    testhjalp::kör_sql(&db, "UPDATE leverantorsfakturarad SET netto = 90000")
        .await
        .unwrap();
    // Originalet, skaparen och registreringen ändras inte, och fakturan blir inget utkast.
    for sql in [
        "UPDATE leverantorsfaktura SET underlag_id = underlag_id + 1",
        "UPDATE leverantorsfaktura SET skapad = '2020-01-01T00:00:00Z'",
        "UPDATE leverantorsfaktura SET status = 'utkast'",
        "UPDATE leverantorsfaktura SET verifikat_id = NULL",
        "UPDATE leverantorsfaktura SET registrerad = '2020-01-01T00:00:00Z'",
        // Att registrera om kräver ett verifikat som inte redan är rättat.
        "UPDATE leverantorsfaktura SET status = 'registrerad'",
    ] {
        let fel = testhjalp::kör_sql(&db, sql).await.unwrap_err();
        assert!(fel.to_string().contains("avskriften"), "{sql}: {fel}");
    }
    let fel = testhjalp::kör_sql(&db, "DELETE FROM leverantorsfaktura")
        .await
        .unwrap_err();
    assert!(fel.to_string().contains("går inte att radera"), "{fel}");
    // Upplåsningen ändras och raderas aldrig.
    for sql in [
        "UPDATE leverantorsfaktura_avskrift SET orsak = 'x'",
        "DELETE FROM leverantorsfaktura_avskrift",
    ] {
        let fel = testhjalp::kör_sql(&db, sql).await.unwrap_err();
        assert!(
            fel.to_string().contains("går inte att ändra"),
            "{sql}: {fel}"
        );
    }
}

#[tokio::test]
async fn databasen_avvisar_en_betalning_på_en_kreditfaktura_med_hänvisning() {
    let db = Databas::tillfällig().await;
    let (anna, id, l, f) = original(&db).await;
    registrera_kredit(&db, &anna, id, &kredit(l, Some(f), "-250", "-200")).await;
    // Kreditfakturans eget registreringsverifikat är bokfört, så bara hänvisningen hindrar raden.
    let fel = testhjalp::kör_sql(
        &db,
        "INSERT INTO betalning (foretag_id, leverantorsfaktura_id, betaldag, belopp, konto,
                                verifikat_id, registrerad, registrerad_kanal)
         SELECT k.foretag_id, k.id, '2026-04-10', 5, 1930, k.verifikat_id, k.registrerad, 'webb'
         FROM leverantorsfaktura k WHERE k.fakturanummer = 'K-17'",
    )
    .await
    .unwrap_err();
    assert!(
        fel.to_string().contains("registrerad leverantörsfaktura"),
        "{fel}"
    );
}

#[tokio::test]
async fn en_upplåst_faktura_får_ingen_total_under_det_som_redan_är_betalt_och_krediterat() {
    // Berättelse 68: reskontran går aldrig under noll. 1 250 kr, kredit 500 kr, betalt 750 kr:
    // registreras fakturan om med 1 000 kr skulle det återstå −250 kr.
    let db = Databas::tillfällig().await;
    let (anna, id, l, f) = original(&db).await;
    registrera_kredit(&db, &anna, id, &kredit(l, Some(f), "-500", "-400")).await;
    betala(&db, &anna, id, f, "750").await;
    leverantorsfaktura::rätta_avskrift(&db, &klocka(), &anna, id, f, "Fel belopp")
        .await
        .unwrap();

    let fel =
        leverantorsfaktura::registrera(&db, &klocka(), &anna, id, f, &avskrift(l, "1 000", "800"))
            .await
            .unwrap_err();
    assert_eq!(spärrkoder(&fel), ["under_betalt"]);

    leverantorsfaktura::registrera(&db, &klocka(), &anna, id, f, &avskrift(l, "1 250", "1 000"))
        .await
        .unwrap();
    assert_eq!(återstår_i_listan(&db, &anna, id, f).await, 0);
}
