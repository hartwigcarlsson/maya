//! Kundförlust på en kundfaktura (#141): spärrarna i panelen, konteringen enligt tabellen över
//! automatkonteringar med momsreduceringen fördelad proportionellt, och återkallelsen som vänder
//! förlustens verifikat.

use jiff::civil::date;
use maya_domain::belopp::Belopp;
use maya_domain::kontoplan::Kontonummer;
use maya_domain::kundforlust::{
    Fält, Förutsättningar, KUNDFÖRLUSTSPÄRRAR, NyKundförlust, Skäl, återkallelse,
};
use maya_domain::rakenskapsar::Räkenskapsår;
use maya_domain::verifikat::Rad;

fn rad(nr: u16, debet: i64, kredit: i64) -> Rad {
    Rad {
        konto: Kontonummer::new(nr).unwrap(),
        debet: Belopp::från_öre(debet),
        kredit: Belopp::från_öre(kredit),
        text: None,
    }
}

fn öre(rader: &[Rad]) -> Vec<(u16, i64, i64)> {
    rader
        .iter()
        .map(|r| (r.konto.get(), r.debet.öre(), r.kredit.öre()))
        .collect()
}

/// 9 500 kr netto med 25 % moms: utfärdandet D 1510 11 875 / K 3001 9 500 / K 2611 2 375.
fn förutsättningar(återstår: i64) -> Förutsättningar {
    Förutsättningar {
        återstår,
        fakturadatum: date(2026, 3, 10),
        kundförlust: false,
        utfärdandet: Some(vec![
            rad(1510, 1_187_500, 0),
            rad(3001, 0, 950_000),
            rad(2611, 0, 237_500),
        ]),
        räkenskapsår: vec![Räkenskapsår::befintligt(
            date(2026, 1, 1),
            date(2026, 12, 31),
        )],
    }
}

fn förlust(skäl: &str, datum: &str) -> NyKundförlust {
    NyKundförlust {
        skäl: skäl.into(),
        annat: String::new(),
        datum: datum.into(),
    }
}

fn koder(n: &NyKundförlust, f: &Förutsättningar) -> Vec<&'static str> {
    n.kontrollera(f)
        .err()
        .unwrap_or_default()
        .iter()
        .map(|s| s.kod)
        .collect()
}

#[test]
fn förlusten_bokför_6351_netto_och_momsen_mot_1510_på_det_som_återstår() {
    // 1 875 kr är betalt; 10 000 kr återstår och fördelas 8 000 netto och 2 000 moms.
    let k = förlust("konkurs", "2026-09-30")
        .kontrollera(&förutsättningar(1_000_000))
        .unwrap();
    assert_eq!(k.belopp, 1_000_000);
    assert_eq!(k.datum, date(2026, 9, 30));
    assert_eq!(k.skäl, Skäl::Konkurs);
    assert_eq!(
        öre(&k.kontering()),
        [(6351, 800_000, 0), (2611, 200_000, 0), (1510, 0, 1_000_000)]
    );
}

#[test]
fn momsreduceringen_fördelas_per_momssats_och_avrundas_så_att_summan_stämmer() {
    // 1 000 kr à 25 % och 1 000 kr à 12 %: total 2 370 kr, varav 100 kr återstår.
    let mut f = förutsättningar(10_000);
    f.utfärdandet = Some(vec![
        rad(1510, 237_000, 0),
        rad(3001, 0, 100_000),
        rad(3002, 0, 100_000),
        rad(2611, 0, 25_000),
        rad(2621, 0, 12_000),
    ]);
    let k = förlust("ackord", "2026-09-30").kontrollera(&f).unwrap();
    // 10 000 · 2 000/2 370 = 8 438,8; · 250/2 370 = 1 054,9; · 120/2 370 = 506,3.
    assert_eq!(
        öre(&k.kontering()),
        [
            (6351, 8_439, 0),
            (2611, 1_055, 0),
            (2621, 506, 0),
            (1510, 0, 10_000)
        ]
    );
}

#[test]
fn utan_moms_på_fakturan_går_hela_förlusten_till_6351() {
    let mut f = förutsättningar(500_000);
    f.utfärdandet = Some(vec![rad(1510, 500_000, 0), rad(3231, 0, 500_000)]);
    let k = förlust("utmatning", "2026-09-30").kontrollera(&f).unwrap();
    assert_eq!(
        öre(&k.kontering()),
        [(6351, 500_000, 0), (1510, 0, 500_000)]
    );
}

#[test]
fn när_fordran_inte_är_bokförd_blir_förlusten_bara_en_status() {
    let mut f = förutsättningar(1_187_500);
    f.utfärdandet = None;
    f.räkenskapsår.clear();
    let k = förlust("konkurs", "2027-02-01").kontrollera(&f).unwrap();
    assert!(k.kontering().is_empty());
    assert_eq!(k.räkenskapsår, None);
}

#[test]
fn skäl_och_datum_krävs() {
    assert_eq!(
        koder(&förlust("", ""), &förutsättningar(1_000)),
        ["skal_saknas", "datum_saknas"]
    );
    assert_eq!(
        koder(&förlust("strejk", "30/9"), &förutsättningar(1_000)),
        ["skal_ogiltigt", "datum_ogiltigt"]
    );
    let fel = förlust("", "")
        .kontrollera(&förutsättningar(1_000))
        .unwrap_err();
    assert_eq!(fel[0].fält, Fält::Skäl);
    assert_eq!(fel[1].fält, Fält::Datum);
}

#[test]
fn annat_skäl_kräver_en_beskrivning_som_blir_skälets_text() {
    assert_eq!(
        koder(&förlust("annat", "2026-09-30"), &förutsättningar(1_000)),
        ["annat_saknas"]
    );
    let mut n = förlust("annat", "2026-09-30");
    n.annat = "  Kunden har avlidit, dödsboet saknar tillgångar ".into();
    let k = n.kontrollera(&förutsättningar(1_000)).unwrap();
    assert_eq!(k.skäl, Skäl::Annat);
    assert_eq!(
        k.skälet(),
        "Annat: Kunden har avlidit, dödsboet saknar tillgångar"
    );
    let k = förlust("utmatning", "2026-09-30")
        .kontrollera(&förutsättningar(1_000))
        .unwrap();
    assert_eq!(k.skälet(), "Utmätning utan resultat");
}

#[test]
fn förlusten_gäller_bara_en_faktura_med_något_kvar_och_utan_gällande_förlust() {
    assert_eq!(
        koder(&förlust("konkurs", "2026-09-30"), &förutsättningar(0)),
        ["inget_aterstar"]
    );
    let mut f = förutsättningar(1_000);
    f.kundförlust = true;
    assert_eq!(
        koder(&förlust("konkurs", "2026-09-30"), &f),
        ["redan_kundforlust"]
    );
}

#[test]
fn datumet_är_inte_före_fakturadatumet_och_ligger_i_ett_räkenskapsår() {
    assert_eq!(
        koder(&förlust("konkurs", "2026-03-09"), &förutsättningar(1_000)),
        ["fore_fakturadatum"]
    );
    assert_eq!(
        koder(&förlust("konkurs", "2028-03-09"), &förutsättningar(1_000)),
        ["saknat_rakenskapsar"]
    );
}

#[test]
fn återkallelsen_vänder_förlustens_rader() {
    let förlusten = [
        rad(6351, 800_000, 0),
        rad(2611, 200_000, 0),
        rad(1510, 0, 1_000_000),
    ];
    assert_eq!(
        öre(&återkallelse(&förlusten)),
        [(6351, 0, 800_000), (2611, 0, 200_000), (1510, 1_000_000, 0)]
    );
}

#[test]
fn varje_spärrkod_är_dokumenterad() {
    let mut fall = vec![
        (förlust("", ""), förutsättningar(1_000)),
        (förlust("strejk", "30/9"), förutsättningar(1_000)),
        (förlust("annat", "2026-03-09"), förutsättningar(0)),
        (förlust("konkurs", "2028-03-09"), förutsättningar(1_000)),
        (förlust("konkurs", "2025-03-09"), förutsättningar(1_000)),
    ];
    let mut redan = förutsättningar(1_000);
    redan.kundförlust = true;
    fall.push((förlust("konkurs", "2026-09-30"), redan));
    let mut koderna: Vec<&str> = fall.iter().flat_map(|(n, f)| koder(n, f)).collect();
    koderna.sort_unstable();
    koderna.dedup();
    let mut dokumenterade: Vec<&str> = KUNDFÖRLUSTSPÄRRAR.iter().map(|(k, _)| *k).collect();
    dokumenterade.sort_unstable();
    assert_eq!(koderna, dokumenterade);
}
