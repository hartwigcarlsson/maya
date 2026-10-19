//! T2 under kontantmetoden genom `core` mot en tillfällig databas (#150): dagen efter
//! räkenskapsårets slut skapar Maya ett T2-utkast med det obetalda beloppet vid årets slut på
//! varje faktura vars fordran eller skuld inte är bokförd. Utkastet räknas om när fakturorna och
//! betalningarna ändras, kan inte ändras för hand och bokförs av en användare; då är fordringarna
//! och skulderna bokförda, och verifikatet vänds inte. En betalning efter årets slut av en faktura
//! som var obetald vid årets slut bokförs mot 1510 eller 2440, också innan T2 är bokfört.

use maya_core::aktor::Aktör;
use maya_core::atkomst::ÅtkomstFel;
use maya_core::att_gora::{self, Fakturautkast, Sak};
use maya_core::betalning::{self, Faktura};
use maya_core::foretag::FöretagsId;
use maya_core::kundfaktura::{self, KundfakturaId};
use maya_core::leverantorsfaktura::LeverantörsfakturaId;
use maya_core::t2::{self, T2Fel};
use maya_core::verifikat::{self, VerifikatFel, VerifikatId};
use maya_core::{Databas, Klocka, behandlingshistorik, kundforlust, testhjalp};
use maya_domain::betalning::NyBetalning;
use maya_domain::kreditfaktura::NyKreditering;
use maya_domain::kundforlust::NyKundförlust;

fn klocka(nu: &str) -> Klocka {
    Klocka::fast(nu.parse().unwrap())
}

/// Årets sista dag i Stockholm.
fn nyårsafton() -> Klocka {
    klocka("2026-12-31T12:00:00Z")
}

/// Dagen efter årets slut, när jobbet skapar utkastet.
fn nyårsdagen() -> Klocka {
    klocka("2027-01-01T03:00:00Z")
}

/// Ett företag med kontantmetoden, en kundfaktura på 11 875 kr (3001 netto 9 500 kr, 2611
/// 2 375 kr) daterad 2026-03-10 och en leverantörsfaktura på 1 250 kr (5410 netto 1 000 kr, 2641
/// 250 kr) daterad 2026-03-14.
async fn kontantföretag(
    db: &Databas,
) -> (Aktör, FöretagsId, KundfakturaId, LeverantörsfakturaId) {
    let (anna, id) = testhjalp::fakturaföretag_med_metod(db, "kontant").await;
    let k = testhjalp::utfärdad_kundfaktura(db, &anna, id, "Kund AB", "2026-03-10").await;
    let l = testhjalp::leverantör(db, &anna, id, "Kontorsvaror AB").await;
    let l = testhjalp::registrerad_leverantörsfaktura(db, &anna, id, l, "8812", "").await;
    (anna, id, k.id, l)
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
    k: &Klocka,
    anna: &Aktör,
    id: FöretagsId,
    faktura: Faktura,
    b: &NyBetalning,
) -> betalning::Registrerad {
    betalning::registrera(db, k, anna, id, faktura, b)
        .await
        .unwrap()
}

/// Verifikatets rader som (konto, debet, kredit) i kontoordning.
async fn rader(
    db: &Databas, anna: &Aktör, id: FöretagsId, v: VerifikatId
) -> Vec<(u16, i64, i64)> {
    let mut r: Vec<_> = verifikat::verifikatet(db, anna, id, v)
        .await
        .unwrap()
        .unwrap()
        .rader
        .iter()
        .map(|r| (r.konto.unwrap().get(), r.debet.öre(), r.kredit.öre()))
        .collect();
    r.sort();
    r
}

/// Företagets T2-utkast, om det finns.
async fn utkastet(db: &Databas, anna: &Aktör, id: FöretagsId) -> Option<VerifikatId> {
    t2::lista(db, anna, id)
        .await
        .unwrap()
        .into_iter()
        .find(|t| !t.bokfört)
        .map(|t| t.verifikat)
}

async fn typer(db: &Databas, anna: &Aktör, id: FöretagsId) -> Vec<(String, String)> {
    behandlingshistorik::läs(db, anna, id)
        .await
        .unwrap()
        .into_iter()
        .map(|p| (p.typ, verifikat_namn(&p.aktör)))
        .collect()
}

fn verifikat_namn(a: &Aktör) -> String {
    a.kanal.namn().to_string()
}

const KUNDRADER: [(u16, i64, i64); 3] =
    [(1510, 1_187_500, 0), (2611, 0, 237_500), (3001, 0, 950_000)];
const LEVERANTÖRSRADER: [(u16, i64, i64); 3] =
    [(2440, 0, 125_000), (2641, 25_000, 0), (5410, 100_000, 0)];

fn sorterat(mut r: Vec<(u16, i64, i64)>) -> Vec<(u16, i64, i64)> {
    r.sort();
    r
}

#[tokio::test]
async fn jobbet_skapar_utkastet_dagen_efter_årets_slut_med_de_obetalda_fakturorna() {
    let db = Databas::tillfällig().await;
    let (anna, id, k, _) = kontantföretag(&db).await;
    // En delbetalning i året: 1 875 kr, varav 1 500 kr intäkt och 375 kr moms.
    betala(
        &db,
        &nyårsafton(),
        &anna,
        id,
        Faktura::Kund(k),
        &betalning("2026-05-02", "1 875"),
    )
    .await;

    // Under året skapas inget.
    assert!(t2::kör(&db, &nyårsafton()).await.unwrap().is_empty());
    assert_eq!(utkastet(&db, &anna, id).await, None);

    let skapade = t2::kör(&db, &nyårsdagen()).await.unwrap();
    let v = utkastet(&db, &anna, id).await.expect("T2-utkastet");
    assert_eq!(skapade, [(id, v)]);
    let utkast = verifikat::verifikatet(&db, &anna, id, v)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(utkast.nummer, None);
    assert_eq!(utkast.datum.unwrap().to_string(), "2026-12-31");
    assert!(utkast.t2);
    assert_eq!(utkast.skapad.aktör, Aktör::maya());
    assert_eq!(
        rader(&db, &anna, id, v).await,
        sorterat(vec![
            (1510, 1_000_000, 0),
            (2440, 0, 125_000),
            (2611, 0, 200_000),
            (2641, 25_000, 0),
            (3001, 0, 800_000),
            (5410, 100_000, 0),
        ])
    );
    // Varje faktura har sina rader med fakturan som text.
    assert!(
        utkast
            .rader
            .iter()
            .any(|r| r.text.as_deref() == Some("kundfaktura 1, Kund AB"))
    );
    assert!(
        utkast
            .rader
            .iter()
            .any(|r| r.text.as_deref() == Some("leverantörsfaktura 8812, Kontorsvaror AB"))
    );

    // Jobbet är idempotent: samma utkast nästa natt och en post om att det skapades.
    assert!(
        t2::kör(&db, &klocka("2027-01-02T03:00:00Z"))
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(utkastet(&db, &anna, id).await, Some(v));
    let skapat: Vec<_> = typer(&db, &anna, id)
        .await
        .into_iter()
        .filter(|(typ, _)| typ == "T2-utkast skapat")
        .collect();
    assert_eq!(
        skapat,
        [("T2-utkast skapat".to_string(), "maya".to_string())]
    );
}

#[tokio::test]
async fn inget_utkast_skapas_utan_obetalda_fakturor_eller_under_fakturametoden() {
    // Fakturametoden: fordringarna och skulderna är bokförda från utfärdandet.
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::fakturaföretag_med_metod(&db, "faktura").await;
    testhjalp::utfärdad_kundfaktura(&db, &anna, id, "Kund AB", "2026-03-10").await;
    assert!(t2::kör(&db, &nyårsdagen()).await.unwrap().is_empty());
    assert_eq!(utkastet(&db, &anna, id).await, None);

    // Kontantmetoden, allt betalt i året.
    let db = Databas::tillfällig().await;
    let (anna, id, k, l) = kontantföretag(&db).await;
    let k_ = nyårsafton();
    betala(
        &db,
        &k_,
        &anna,
        id,
        Faktura::Kund(k),
        &betalning("2026-04-01", "11 875"),
    )
    .await;
    betala(
        &db,
        &k_,
        &anna,
        id,
        Faktura::Leverantör(l),
        &betalning("2026-04-01", "1 250"),
    )
    .await;
    assert!(t2::kör(&db, &nyårsdagen()).await.unwrap().is_empty());
    assert_eq!(utkastet(&db, &anna, id).await, None);
}

#[tokio::test]
async fn utkastet_räknas_om_när_betalningar_registreras_och_rättas_och_raderas_när_inget_är_obetalt()
 {
    let db = Databas::tillfällig().await;
    let (anna, id, k, l) = kontantföretag(&db).await;
    t2::kör(&db, &nyårsdagen()).await.unwrap();
    let v = utkastet(&db, &anna, id).await.unwrap();
    let januari = klocka("2027-01-10T09:00:00Z");

    // En betalning i året som registreras i januari tar bort kundfakturan ur utkastet.
    let b = betala(
        &db,
        &januari,
        &anna,
        id,
        Faktura::Kund(k),
        &betalning("2026-12-20", "11 875"),
    )
    .await;
    assert_eq!(utkastet(&db, &anna, id).await, Some(v));
    assert_eq!(rader(&db, &anna, id, v).await, LEVERANTÖRSRADER);

    // Rättas betalningen med en rättelse i året var fakturan obetald vid årets slut igen. (En
    // rättelse i januari vänder betalningen i det nya året, och vid årets slut var den betald.)
    betalning::rätta(&db, &januari, &anna, id, b.id, "2026-12-31", "fel faktura")
        .await
        .unwrap();
    assert_eq!(
        rader(&db, &anna, id, v).await,
        sorterat([KUNDRADER, LEVERANTÖRSRADER].concat())
    );

    // Betalas båda i året finns inget obetalt, och Maya raderar utkastet.
    betala(
        &db,
        &januari,
        &anna,
        id,
        Faktura::Kund(k),
        &betalning("2026-12-28", "11 875"),
    )
    .await;
    betala(
        &db,
        &januari,
        &anna,
        id,
        Faktura::Leverantör(l),
        &betalning("2026-12-28", "1 250"),
    )
    .await;
    assert_eq!(utkastet(&db, &anna, id).await, None);
    assert!(
        verifikat::verifikatet(&db, &anna, id, v)
            .await
            .unwrap()
            .is_none()
    );
    let raderat = behandlingshistorik::läs(&db, &anna, id)
        .await
        .unwrap()
        .into_iter()
        .rfind(|p| p.typ == "utkast raderat")
        .unwrap();
    assert_eq!(raderat.aktör, Aktör::maya());
}

#[tokio::test]
async fn utkastet_kan_inte_ändras_raderas_eller_bokföras_som_ett_vanligt_utkast() {
    let db = Databas::tillfällig().await;
    let (anna, id, _, _) = kontantföretag(&db).await;
    t2::kör(&db, &nyårsdagen()).await.unwrap();
    let v = utkastet(&db, &anna, id).await.unwrap();
    let k = nyårsdagen();
    let formulär = maya_domain::verifikat::NyttVerifikat {
        datum: "2026-12-31".into(),
        beskrivning: "Ändrat".into(),
        rader: Vec::new(),
    };
    let fel = verifikat::spara_utkast(&db, &k, &anna, id, Some(v), &formulär)
        .await
        .unwrap_err();
    assert!(matches!(fel, VerifikatFel::T2Utkast), "{fel:?}");
    let fel = verifikat::bokför_utkast(&db, &k, &anna, id, v, &formulär)
        .await
        .unwrap_err();
    assert!(matches!(fel, VerifikatFel::T2Utkast), "{fel:?}");
    let fel = verifikat::radera_utkast(&db, &k, &anna, id, v)
        .await
        .unwrap_err();
    assert!(matches!(fel, VerifikatFel::T2Utkast), "{fel:?}");
    assert_eq!(utkastet(&db, &anna, id).await, Some(v));
    let fel = testhjalp::kör_sql(&db, "UPDATE t2 SET rakenskapsar_id = rakenskapsar_id").await;
    assert!(fel.unwrap_err().to_string().contains("går inte att ändra"));
}

#[tokio::test]
async fn användaren_bokför_utkastet_och_fakturorna_betalas_sedan_mot_1510_och_2440() {
    let db = Databas::tillfällig().await;
    let (anna, id, k, l) = kontantföretag(&db).await;
    t2::kör(&db, &nyårsdagen()).await.unwrap();
    let v = utkastet(&db, &anna, id).await.unwrap();
    let januari = klocka("2027-01-10T09:00:00Z");

    // Maya bokför inte, och den som saknar åtkomst får inte.
    let fel = t2::bokför(&db, &januari, &Aktör::maya(), id, v)
        .await
        .unwrap_err();
    assert!(
        matches!(fel, T2Fel::Åtkomst(ÅtkomstFel::KanalenFårInte)),
        "{fel:?}"
    );
    let bo = testhjalp::användare(&db, "bo@example.se").await;
    let fel = t2::bokför(&db, &januari, &bo, id, v).await.unwrap_err();
    assert!(
        matches!(fel, T2Fel::Åtkomst(ÅtkomstFel::IngenÅtkomst)),
        "{fel:?}"
    );

    let bokfört = t2::bokför(&db, &januari, &anna, id, v).await.unwrap();
    assert_eq!(bokfört.id, v);
    let t2v = verifikat::verifikatet(&db, &anna, id, v)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(t2v.nummer, Some(bokfört.nummer));
    assert_eq!(t2v.datum.unwrap().to_string(), "2026-12-31");
    assert_eq!(
        rader(&db, &anna, id, v).await,
        sorterat([KUNDRADER, LEVERANTÖRSRADER].concat())
    );
    assert!(t2::lista(&db, &anna, id).await.unwrap()[0].bokfört);
    let historik = typer(&db, &anna, id).await;
    assert!(
        historik.contains(&("T2-utkast bokfört".into(), "webb".into())),
        "{historik:?}"
    );

    // Fordringarna och skulderna är bokförda nu.
    for f in [Faktura::Kund(k), Faktura::Leverantör(l)] {
        assert!(
            betalning::reskontrapost(&db, &anna, id, f)
                .await
                .unwrap()
                .bokförd
        );
    }
    // Betalningarna i det nya året bokförs mot reskontrakontot, så intäkten och kostnaden
    // bokförs inte två gånger.
    let b = betala(
        &db,
        &januari,
        &anna,
        id,
        Faktura::Kund(k),
        &betalning("2027-01-20", "11 875"),
    )
    .await;
    assert_eq!(
        rader(&db, &anna, id, b.verifikat.id).await,
        [(1510, 0, 1_187_500), (1930, 1_187_500, 0)]
    );
    let b = betala(
        &db,
        &januari,
        &anna,
        id,
        Faktura::Leverantör(l),
        &betalning("2027-01-20", "1 250"),
    )
    .await;
    assert_eq!(
        rader(&db, &anna, id, b.verifikat.id).await,
        [(1930, 0, 125_000), (2440, 125_000, 0)]
    );

    // T2-verifikatet vänds inte, varken med Rätta eller i databasen.
    let fel = verifikat::rätta(
        &db,
        &januari,
        &anna,
        id,
        v,
        "2027-01-20",
        "",
        verifikat::Rättelseval::BaraRättelse,
    )
    .await
    .unwrap_err();
    assert!(
        matches!(&fel, VerifikatFel::InteRättbart(s) if s.kod() == "t2_vands_inte"),
        "{fel:?}"
    );
    // Ett nytt utkast skapas inte för året.
    assert!(
        t2::kör(&db, &klocka("2027-01-21T03:00:00Z"))
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(utkastet(&db, &anna, id).await, None);
}

#[tokio::test]
async fn en_betalning_efter_årets_slut_bokförs_mot_reskontrakontot_också_innan_t2_är_bokfört() {
    let db = Databas::tillfällig().await;
    let (anna, id, k, l) = kontantföretag(&db).await;
    let januari = klocka("2027-01-10T09:00:00Z");
    // Innan jobbet har kört, och efter.
    let b = betala(
        &db,
        &januari,
        &anna,
        id,
        Faktura::Kund(k),
        &betalning("2027-01-05", "1 875"),
    )
    .await;
    assert_eq!(
        rader(&db, &anna, id, b.verifikat.id).await,
        [(1510, 0, 187_500), (1930, 187_500, 0)]
    );
    t2::kör(&db, &januari).await.unwrap();
    let b = betala(
        &db,
        &januari,
        &anna,
        id,
        Faktura::Leverantör(l),
        &betalning("2027-01-08", "1 250"),
    )
    .await;
    assert_eq!(
        rader(&db, &anna, id, b.verifikat.id).await,
        [(1930, 0, 125_000), (2440, 125_000, 0)]
    );
    // Utkastet har fakturornas obetalda belopp vid årets slut.
    let v = utkastet(&db, &anna, id).await.unwrap();
    assert_eq!(
        rader(&db, &anna, id, v).await,
        sorterat([KUNDRADER, LEVERANTÖRSRADER].concat())
    );
    // En kundförlust efter årets slut bokförs också mot 1510.
    let förlust = kundforlust::bokför(
        &db,
        &januari,
        &anna,
        id,
        k,
        &NyKundförlust {
            skäl: "konkurs".into(),
            datum: "2027-01-09".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let v = förlust.verifikat.expect("förlustens verifikat").id;
    assert_eq!(
        rader(&db, &anna, id, v).await,
        [(1510, 0, 1_000_000), (2611, 200_000, 0), (6351, 800_000, 0)]
    );
    // Men en betalning i året bokförs mot intäkt och moms.
    let db = Databas::tillfällig().await;
    let (anna, id, k, _) = kontantföretag(&db).await;
    let b = betala(
        &db,
        &januari,
        &anna,
        id,
        Faktura::Kund(k),
        &betalning("2026-12-30", "1 875"),
    )
    .await;
    assert_eq!(
        rader(&db, &anna, id, b.verifikat.id).await,
        [(1930, 187_500, 0), (2611, 0, 37_500), (3001, 0, 150_000)]
    );
}

#[tokio::test]
async fn en_kredit_före_årets_slut_av_en_faktura_som_väntar_på_t2_bokförs_inte_när_den_ryms_i_det_obetalda()
 {
    let db = Databas::tillfällig().await;
    let (anna, id, k, _) = kontantföretag(&db).await;
    let januari = klocka("2027-01-10T09:00:00Z");
    // Hela fakturan betalas efter årets slut, mot 1510 eftersom T2 tar med den.
    betala(
        &db,
        &januari,
        &anna,
        id,
        Faktura::Kund(k),
        &betalning("2027-01-05", "11 875"),
    )
    .await;
    // Sedan krediteras en timme med ett datum i året, när fakturan var obetald: krediten ryms
    // i det obetalda vid årets slut och bokförs inte, och T2 tar med det som var kvar då.
    let utkast = kundfaktura::kreditera(
        &db,
        &januari,
        &anna,
        id,
        k,
        &NyKreditering {
            hela: false,
            antal: vec!["1".into()],
        },
    )
    .await
    .unwrap();
    let mut formulär = kundfaktura::kundfakturan(&db, &anna, id, utkast)
        .await
        .unwrap()
        .formulär;
    formulär.fakturadatum = "2026-10-04".into();
    kundfaktura::spara_utkast(&db, &januari, &anna, id, Some(utkast), &formulär)
        .await
        .unwrap();
    let kredit = kundfaktura::utfärda(&db, &januari, &anna, id, utkast)
        .await
        .unwrap();
    assert_eq!(kredit.verifikat, None);
    t2::kör(&db, &januari).await.unwrap();
    let v = utkastet(&db, &anna, id).await.unwrap();
    assert_eq!(
        rader(&db, &anna, id, v).await,
        sorterat(
            [
                vec![(1510, 1_068_750, 0), (2611, 0, 213_750), (3001, 0, 855_000)],
                LEVERANTÖRSRADER.to_vec(),
            ]
            .concat()
        )
    );
}

#[tokio::test]
async fn utkastet_står_under_utkast_i_att_göra_och_räknas_en_gång() {
    let db = Databas::tillfällig().await;
    let (anna, id, _, _) = kontantföretag(&db).await;
    let k = nyårsdagen();
    let före = att_gora::antal(&db, &k, &testhjalp::config(), &anna)
        .await
        .unwrap()[&id];
    t2::kör(&db, &k).await.unwrap();
    let v = utkastet(&db, &anna, id).await.unwrap();
    let a = att_gora::att_göra(&db, &k, &testhjalp::config(), &anna, id)
        .await
        .unwrap();
    assert!(
        !a.saker
            .iter()
            .any(|s| matches!(s, Sak::Utkast(u) if u.id == v))
    );
    let sak = a
        .saker
        .iter()
        .find_map(|s| match s {
            Sak::Fakturautkast(f) if f.utkast == Fakturautkast::T2(v) => Some(f),
            _ => None,
        })
        .expect("T2-utkastet under Utkast");
    assert_eq!(sak.fakturadatum.unwrap().to_string(), "2026-12-31");
    assert_eq!(sak.total, Some(1_187_500 + 125_000));
    let efter = att_gora::antal(&db, &k, &testhjalp::config(), &anna)
        .await
        .unwrap()[&id];
    assert_eq!(efter, före + 1);
}
