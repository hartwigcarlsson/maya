//! Att göra med fakturorna (#152), genom `core` mot en tillfällig databas: förfallna kund- och
//! leverantörsfakturor och leverantörsfakturor att betala med kopieringschipsen, antalet i
//! toppmenyn och nyckeltalen Kundfordringar och Leverantörsskulder under båda
//! bokföringsmetoderna.

use jiff::civil::{Date, date};
use maya_core::aktor::Aktör;
use maya_core::att_gora::{self, Fakturasak, Reskontrafaktura, Sak};
use maya_core::betalning::{self, Faktura};
use maya_core::foretag::{self, FöretagsId};
use maya_core::kundforlust;
use maya_core::leverantorsfaktura::{self, LeverantörsfakturaId};
use maya_core::{Databas, Klocka, testhjalp};
use maya_domain::belopp::Saldo;
use maya_domain::betalning::NyBetalning;
use maya_domain::kundforlust::NyKundförlust;
use maya_domain::leverantorsfaktura::{NyLeverantörsfaktura, NyRad};

/// Onsdag 14 oktober 2026, förmiddag i Stockholm.
fn i_dag() -> Klocka {
    Klocka::fast("2026-10-14T08:00:00Z".parse().unwrap())
}

/// En registrerad leverantörsfaktura från en ny leverantör `namn` med bankgiro 5050-1055:
/// 1 000 kr netto och 25 % moms, totalt 1 250 kr, med förfallodagen `förfallodag`.
async fn leverantörsfaktura(
    db: &Databas,
    anna: &Aktör,
    id: FöretagsId,
    namn: &str,
    förfallodag: &str,
    referens: &str,
) -> LeverantörsfakturaId {
    let leverantör = testhjalp::leverantör(db, anna, id, namn).await;
    let (f, _) = leverantorsfaktura::ladda_upp(
        db,
        &i_dag(),
        anna,
        id,
        ("faktura.pdf", "application/pdf", b"%PDF-1.4 faktura"),
    )
    .await
    .unwrap();
    leverantorsfaktura::registrera(
        db,
        &i_dag(),
        anna,
        id,
        f,
        &NyLeverantörsfaktura {
            leverantör: leverantör.to_string(),
            fakturanummer: format!("F-{förfallodag}"),
            fakturadatum: "2026-09-20".into(),
            förfallodag: förfallodag.into(),
            referens: referens.into(),
            totalbelopp: "1 250,00".into(),
            rader: vec![NyRad {
                konto: String::new(),
                netto: "1 000".into(),
                momssats: "25".into(),
                moms: String::new(),
            }],
            ..Default::default()
        },
    )
    .await
    .unwrap();
    f
}

fn fakturorna(saker: &[Sak]) -> Vec<&Fakturasak> {
    saker
        .iter()
        .filter_map(|s| match s {
            Sak::Faktura(f) => Some(f),
            _ => None,
        })
        .collect()
}

fn betalning(betaldag: &str, belopp: &str) -> NyBetalning {
    NyBetalning {
        betaldag: betaldag.into(),
        belopp: belopp.into(),
        konto: "1930".into(),
        ..Default::default()
    }
}

#[tokio::test]
async fn förfallna_kundfakturor_och_obetalda_leverantörsfakturor_kommer_efter_förfallodag() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::fakturaföretag(&db).await;
    // Förföll 2026-04-09 och är delbetald.
    let förfallen = testhjalp::utfärdad_kundfaktura(&db, &anna, id, "Kund AB", "2026-03-10").await;
    betalning::registrera(
        &db,
        &i_dag(),
        &anna,
        id,
        Faktura::Kund(förfallen.id),
        &betalning("2026-04-20", "1 875,00"),
    )
    .await
    .unwrap();
    // Förfaller 2026-11-09: inget att göra ännu.
    testhjalp::utfärdad_kundfaktura(&db, &anna, id, "Senare AB", "2026-10-10").await;
    // Betald i sin helhet.
    let betald = testhjalp::utfärdad_kundfaktura(&db, &anna, id, "Betald AB", "2026-03-11").await;
    betalning::registrera(
        &db,
        &i_dag(),
        &anna,
        id,
        Faktura::Kund(betald.id),
        &betalning("2026-04-01", "11 875,00"),
    )
    .await
    .unwrap();
    let hyra = leverantörsfaktura(
        &db,
        &anna,
        id,
        "Solna Lokaler AB",
        "2026-12-01",
        "Hyra nov, objekt 0412",
    )
    .await;
    let telia = leverantörsfaktura(&db, &anna, id, "Telia AB", "2026-10-10", "8812 0044 711").await;
    let kontor = leverantörsfaktura(&db, &anna, id, "Kontor AB", "2026-10-17", "").await;

    let a = att_gora::att_göra(&db, &i_dag(), &testhjalp::config(), &anna, id)
        .await
        .unwrap();
    let f = fakturorna(&a.saker);

    // I förfallodagens ordning; kundfakturor bara när de har förfallit.
    let ordning: Vec<(Reskontrafaktura, Date)> =
        f.iter().map(|f| (f.faktura, f.förfallodag)).collect();
    assert_eq!(
        ordning,
        [
            (Reskontrafaktura::Kund(förfallen.id), date(2026, 4, 9)),
            (Reskontrafaktura::Leverantör(telia), date(2026, 10, 10)),
            (Reskontrafaktura::Leverantör(kontor), date(2026, 10, 17)),
            (Reskontrafaktura::Leverantör(hyra), date(2026, 12, 1)),
        ]
    );
    let kund = f[0];
    assert_eq!(kund.nummer, förfallen.nummer.to_string());
    assert_eq!(kund.motpart, "Kund AB");
    assert_eq!((kund.total, kund.återstår), (1_187_500, 1_000_000));
    assert_eq!(kund.kontakt, "ekonomi@kund.se");
    assert!(kund.kopieringschips.is_empty(), "{kund:?}");
    let telia = f[1];
    assert_eq!(telia.motpart, "Telia AB");
    assert_eq!(telia.nummer, "F-2026-10-10");
    assert_eq!((telia.total, telia.återstår), (125_000, 125_000));
    assert_eq!(
        telia.kopieringschips,
        [
            ("BG", "5050-1055".to_string()),
            ("OCR", "88120044711".to_string())
        ]
    );
    assert_eq!(
        f[3].kopieringschips,
        [
            ("BG", "5050-1055".to_string()),
            ("Meddelande", "Hyra nov, objekt 0412".to_string())
        ]
    );

    // Toppmenyn räknar det som är förfallet eller förfaller inom 7 dagar, inte det senare.
    testhjalp::backup(&db, &i_dag()).await;
    let antal = att_gora::antal(&db, &i_dag(), &testhjalp::config(), &anna)
        .await
        .unwrap();
    assert_eq!(antal.get(&id), Some(&3));
}

#[tokio::test]
async fn nyckeltalen_visar_1510_och_2440_ur_bokföringen_med_det_förfallna_ur_reskontran() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::fakturaföretag(&db).await;
    let förfallen = testhjalp::utfärdad_kundfaktura(&db, &anna, id, "Kund AB", "2026-03-10").await;
    betalning::registrera(
        &db,
        &i_dag(),
        &anna,
        id,
        Faktura::Kund(förfallen.id),
        &betalning("2026-04-20", "1 875,00"),
    )
    .await
    .unwrap();
    testhjalp::utfärdad_kundfaktura(&db, &anna, id, "Senare AB", "2026-10-10").await;
    // En kundfaktura daterad efter i dag räknas inte i saldot per i dag.
    testhjalp::utfärdad_kundfaktura(&db, &anna, id, "Framtid AB", "2026-10-20").await;
    leverantörsfaktura(&db, &anna, id, "Telia AB", "2026-10-10", "").await;
    leverantörsfaktura(&db, &anna, id, "Kontor AB", "2026-10-17", "").await;

    let n = att_gora::att_göra(&db, &i_dag(), &testhjalp::config(), &anna, id)
        .await
        .unwrap()
        .nyckeltal;

    assert_eq!(
        n.kundfordringar.saldo,
        Saldo::från_öre(1_000_000 + 1_187_500)
    );
    assert_eq!(n.kundfordringar.förfallet, Saldo::från_öre(1_000_000));
    assert!(!n.kundfordringar.ur_reskontran);
    assert_eq!(n.leverantörsskulder.saldo, Saldo::från_öre(250_000));
    assert_eq!(n.leverantörsskulder.förfallet, Saldo::från_öre(125_000));
    assert!(!n.leverantörsskulder.ur_reskontran);
}

#[tokio::test]
async fn under_kontantmetoden_är_nyckeltalen_reskontrans_obetalda_belopp() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::fakturaföretag(&db).await;
    let förfallen = testhjalp::utfärdad_kundfaktura(&db, &anna, id, "Kund AB", "2026-03-10").await;
    betalning::registrera(
        &db,
        &i_dag(),
        &anna,
        id,
        Faktura::Kund(förfallen.id),
        &betalning("2026-04-20", "1 875,00"),
    )
    .await
    .unwrap();
    testhjalp::utfärdad_kundfaktura(&db, &anna, id, "Senare AB", "2026-10-10").await;
    leverantörsfaktura(&db, &anna, id, "Telia AB", "2026-10-10", "").await;
    foretag::byt_bokföringsmetod(
        &db,
        &i_dag(),
        &anna,
        id,
        &maya_domain::foretag::Byte {
            gäller_från: "2027-01-01".into(),
            val: "kontant".into(),
        },
    )
    .await
    .unwrap();
    // En bokning på 1510 utan faktura syns i bokföringen men inte i reskontran.
    testhjalp::bokför(
        &db,
        &anna,
        id,
        "2026-10-01",
        &[(1510, 50_000, 0), (3001, 0, 50_000)],
    )
    .await;
    let februari = Klocka::fast("2027-02-01T08:00:00Z".parse().unwrap());

    let n = att_gora::att_göra(&db, &februari, &testhjalp::config(), &anna, id)
        .await
        .unwrap()
        .nyckeltal;

    assert!(n.kundfordringar.ur_reskontran);
    assert_eq!(
        n.kundfordringar.saldo,
        Saldo::från_öre(1_000_000 + 1_187_500)
    );
    assert_eq!(
        n.kundfordringar.förfallet,
        Saldo::från_öre(1_000_000 + 1_187_500)
    );
    assert!(n.leverantörsskulder.ur_reskontran);
    assert_eq!(n.leverantörsskulder.saldo, Saldo::från_öre(125_000));
    assert_eq!(n.leverantörsskulder.förfallet, Saldo::från_öre(125_000));
}

// #141 Kundförlust: en gällande kundförlust har skrivit bort det som återstår, så fakturan är
// inte förfallen i Att göra; en återkallad förlust räknas inte.
#[tokio::test]
async fn en_kundfaktura_med_en_gällande_kundförlust_är_varken_förfallen_eller_obetald_i_att_göra() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::fakturaföretag(&db).await;
    let förlust = || NyKundförlust {
        skäl: "konkurs".into(),
        annat: String::new(),
        datum: "2026-09-30".into(),
    };
    // Förföll 2026-04-09; förlusten gäller.
    let förlorad =
        testhjalp::utfärdad_kundfaktura(&db, &anna, id, "Förlust AB", "2026-03-10").await;
    kundforlust::bokför(&db, &i_dag(), &anna, id, förlorad.id, &förlust())
        .await
        .unwrap();
    // Förföll 2026-04-10; förlusten är återkallad, så fakturan är förfallen igen.
    let återkallad =
        testhjalp::utfärdad_kundfaktura(&db, &anna, id, "Återkallad AB", "2026-03-11").await;
    let b = kundforlust::bokför(&db, &i_dag(), &anna, id, återkallad.id, &förlust())
        .await
        .unwrap();
    kundforlust::återkalla(&db, &i_dag(), &anna, id, b.id)
        .await
        .unwrap();

    let a = att_gora::att_göra(&db, &i_dag(), &testhjalp::config(), &anna, id)
        .await
        .unwrap();
    let ordning: Vec<Reskontrafaktura> = fakturorna(&a.saker).iter().map(|f| f.faktura).collect();
    assert_eq!(ordning, [Reskontrafaktura::Kund(återkallad.id)]);
    assert_eq!(
        a.nyckeltal.kundfordringar.förfallet,
        Saldo::från_öre(1_187_500)
    );

    testhjalp::backup(&db, &i_dag()).await;
    let antal = att_gora::antal(&db, &i_dag(), &testhjalp::config(), &anna)
        .await
        .unwrap();
    assert_eq!(antal.get(&id), Some(&1));
}

// #140 Kreditfakturor: utfärdade kreditfakturor drar av från originalet, och en kreditfaktura
// är aldrig att betala eller förfallen i Att göra; en krediterad faktura med ett
// tillgodohavande är inte förfallen.
#[tokio::test]
async fn kreditfakturor_räknas_av_mot_originalet_och_syns_inte_själva_i_att_göra() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::fakturaföretag(&db).await;
    // Förföll 2026-04-09; 4 av 10 timmar krediterade, 7 125 kr återstår.
    let delvis = testhjalp::utfärdad_kundfaktura(&db, &anna, id, "Delvis AB", "2026-03-10").await;
    let utkast = maya_core::kundfaktura::kreditera(
        &db,
        &i_dag(),
        &anna,
        id,
        delvis.id,
        &maya_domain::kreditfaktura::NyKreditering {
            hela: false,
            antal: vec!["4".into()],
        },
    )
    .await
    .unwrap();
    let delkredit = maya_core::kundfaktura::utfärda(&db, &i_dag(), &anna, id, utkast)
        .await
        .unwrap();
    // Förföll 2026-04-10; helt krediterad.
    let hel = testhjalp::utfärdad_kundfaktura(&db, &anna, id, "Hel AB", "2026-03-11").await;
    let helkredit = testhjalp::utfärdad_kreditfaktura(&db, &anna, id, hel.id).await;
    // Förföll 2026-04-11; betald och sedan krediterad: kunden har pengar tillgodo.
    let tillgodo =
        testhjalp::utfärdad_kundfaktura(&db, &anna, id, "Tillgodo AB", "2026-03-12").await;
    betalning::registrera(
        &db,
        &i_dag(),
        &anna,
        id,
        Faktura::Kund(tillgodo.id),
        &betalning("2026-04-01", "11 875,00"),
    )
    .await
    .unwrap();
    let tillgodokredit = testhjalp::utfärdad_kreditfaktura(&db, &anna, id, tillgodo.id).await;
    let post = betalning::reskontrapost(&db, &anna, id, Faktura::Kund(tillgodo.id))
        .await
        .unwrap();
    assert_eq!(post.återstår, -1_187_500);

    let a = att_gora::att_göra(&db, &i_dag(), &testhjalp::config(), &anna, id)
        .await
        .unwrap();
    let f = fakturorna(&a.saker);
    let ordning: Vec<Reskontrafaktura> = f.iter().map(|f| f.faktura).collect();
    assert_eq!(ordning, [Reskontrafaktura::Kund(delvis.id)]);
    assert_eq!(f[0].återstår, 1_187_500 - 475_000);
    for k in [&delkredit, &helkredit, &tillgodokredit] {
        assert!(!ordning.contains(&Reskontrafaktura::Kund(k.id)));
    }
    assert_eq!(
        a.nyckeltal.kundfordringar.förfallet,
        Saldo::från_öre(1_187_500 - 475_000)
    );

    testhjalp::backup(&db, &i_dag()).await;
    let antal = att_gora::antal(&db, &i_dag(), &testhjalp::config(), &anna)
        .await
        .unwrap();
    assert_eq!(antal.get(&id), Some(&1));
}

// #148 Kreditfaktura från leverantör och Rätta avskrift

/// Registrerar en kreditfaktura på `total` kr (netto `netto` kr, 25 %) som krediterar `original`.
async fn leverantörskredit(
    db: &Databas,
    anna: &Aktör,
    id: FöretagsId,
    original: LeverantörsfakturaId,
    total: &str,
    netto: &str,
) {
    let o = leverantorsfaktura::fakturan(db, anna, id, original)
        .await
        .unwrap()
        .unwrap();
    let (k, _) = leverantorsfaktura::ladda_upp(
        db,
        &i_dag(),
        anna,
        id,
        ("kredit.pdf", "application/pdf", b"%PDF-1.4 kredit"),
    )
    .await
    .unwrap();
    leverantorsfaktura::registrera(
        db,
        &i_dag(),
        anna,
        id,
        k,
        &NyLeverantörsfaktura {
            leverantör: o.leverantör.unwrap().to_string(),
            fakturanummer: format!("K-{}", o.formulär.fakturanummer),
            fakturadatum: "2026-09-25".into(),
            förfallodag: "2026-10-25".into(),
            totalbelopp: total.into(),
            krediterar: original.to_string(),
            rader: vec![NyRad {
                konto: String::new(),
                netto: netto.into(),
                momssats: "25".into(),
                moms: String::new(),
            }],
            ..Default::default()
        },
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn leverantörens_kreditfakturor_minskar_det_som_ska_betalas_och_en_upplåst_faktura_väntar() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    let delvis = leverantörsfaktura(&db, &anna, id, "Delvis AB", "2026-10-10", "").await;
    leverantörskredit(&db, &anna, id, delvis, "-250", "-200").await;
    let hel = leverantörsfaktura(&db, &anna, id, "Hel AB", "2026-10-11", "").await;
    leverantörskredit(&db, &anna, id, hel, "-1250", "-1000").await;
    let upplåst = leverantörsfaktura(&db, &anna, id, "Upplåst AB", "2026-10-12", "").await;
    leverantorsfaktura::rätta_avskrift(&db, &i_dag(), &anna, id, upplåst, "")
        .await
        .unwrap();

    let a = att_gora::att_göra(&db, &i_dag(), &testhjalp::config(), &anna, id)
        .await
        .unwrap();
    let f = fakturorna(&a.saker);
    let ordning: Vec<Reskontrafaktura> = f.iter().map(|f| f.faktura).collect();
    assert_eq!(ordning, [Reskontrafaktura::Leverantör(delvis)]);
    assert_eq!(f[0].återstår, 100_000);

    testhjalp::backup(&db, &i_dag()).await;
    let antal = att_gora::antal(&db, &i_dag(), &testhjalp::config(), &anna)
        .await
        .unwrap();
    assert_eq!(antal.get(&id), Some(&1));
}
