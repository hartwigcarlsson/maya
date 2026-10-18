//! INK2-kopplingstabellen. De förväntade raderna och fältkoderna är avlästa för hand i BAS
//! kopplingstabell `crates/domain/data/ink2/INK2_P1-231002_241119.xlsx`.

use maya_domain::kontoplan::{Kontonummer, bas, ink2};

fn koppling(n: u16) -> ink2::Koppling {
    ink2::koppling(Kontonummer::new(n).unwrap()).unwrap_or_else(|| panic!("{n} saknar INK2-rad"))
}

/// INK2-raden och fältkoden för ett konto med positivt respektive negativt netto.
fn fält(n: u16) -> [(&'static str, u16); 2] {
    let k = koppling(n);
    [true, false].map(|plus| {
        let f = k.fält(plus);
        (f.rad().rad(), f.kod())
    })
}

#[test]
fn varje_konto_i_bas_2026_får_en_ink2_rad() {
    let utan: Vec<_> = bas::konton()
        .iter()
        .map(|k| k.nummer())
        .filter(|&nr| ink2::koppling(nr).is_none())
        .collect();
    assert_eq!(utan, []);
}

#[test]
fn kontot_får_raden_ur_intervallet() {
    assert_eq!(fält(1930), [("2.26", 7281); 2]);
    assert_eq!(fält(1510), [("2.19", 7251); 2]);
    assert_eq!(fält(2099), [("2.28", 7302); 2]);
    assert_eq!(fält(2440), [("2.45", 7365); 2]);
    assert_eq!(fält(3001), [("3.1", 7410); 2]);
    assert_eq!(fält(6110), [("3.7", 7513); 2]);
    assert_eq!(fält(7010), [("3.8", 7514); 2]);
}

#[test]
fn ett_undantaget_konto_får_bara_sin_egen_rad() {
    // 10xx (exkl. 1088) och 1088.
    assert_eq!(fält(1010), [("2.1", 7201); 2]);
    assert_eq!(fält(1088), [("2.2", 7202); 2]);
    // 157x på 2.20, men 1573 står uttryckligen på 2.21.
    assert_eq!(fält(1570), [("2.20", 7252); 2]);
    assert_eq!(fält(1573), [("2.21", 7261); 2]);
    // 28xx (exkl. 286x-287x) och 2873 på 2.48; 286x-287x på 2.47.
    assert_eq!(fält(2873), [("2.48", 7369); 2]);
    assert_eq!(fält(2860), [("2.47", 7367); 2]);
    // 49xx (exkl. 4910-4929, 496x, 498x).
    assert_eq!(fält(4910), [("3.5", 7511); 2]);
    assert_eq!(fält(4960), [("3.6", 7512); 2]);
}

#[test]
fn bas_2026_kontogrupper_avgör_mellan_råvaror_och_handelsvaror() {
    // Tabellen har 40xx-47xx på både 3.5 och 3.6. BAS 2026: 40 "Inköp av handelsvaror",
    // 43 "… (Råvaror och förnödenheter)", 48 (ny) "… (Råvaror och förnödenheter)".
    assert_eq!(fält(4010), [("3.6", 7512); 2]);
    assert_eq!(fält(4211), [("3.6", 7512); 2]);
    assert_eq!(fält(4310), [("3.5", 7511); 2]);
    assert_eq!(fält(4731), [("3.5", 7511); 2]);
    assert_eq!(fält(4810), [("3.5", 7511); 2]);
}

#[test]
fn eget_kapital_för_andra_företagsformer_är_fritt_eget_kapital() {
    // 2010–2079 saknas i tabellen; INK2 lämnas inte av enskild firma eller handelsbolag.
    assert_eq!(fält(2010), [("2.28", 7302); 2]);
    assert_eq!(fält(2072), [("2.28", 7302); 2]);
}

#[test]
fn saldots_tecken_avgör_fältet() {
    assert_eq!(fält(4990), [("3.2", 7411), ("3.2", 7510)]);
    assert_eq!(fält(8999), [("3.26", 7450), ("3.27", 7550)]);
    assert_eq!(fält(8810), [("3.21", 7420), ("3.22", 7525)]);
    assert_eq!(fält(8819), [("3.21", 7420); 2]);
    assert_eq!(fält(8811), [("3.22", 7525); 2]);
    assert_eq!(fält(8850), [("3.23", 7421), ("3.23", 7526)]);
    // 884x står bara med minusfältet.
    assert_eq!(fält(8840), [("3.24", 7527); 2]);
}

#[test]
fn ett_eget_konto_hamnar_rätt_via_intervallen() {
    // 1939 och 6999 finns inte i BAS 2026.
    assert!(bas::konto(Kontonummer::new(1939).unwrap()).is_none());
    assert_eq!(fält(1939), [("2.26", 7281); 2]);
    assert_eq!(fält(6999), [("3.7", 7513); 2]);
}

#[test]
fn raden_har_blankettens_benämning_och_rubriker() {
    let rad = koppling(1930).fält(true).rad();
    assert_eq!(rad.benämning(), "Kassa, bank och redovisningsmedel");
    assert_eq!(
        rad.rubriker(),
        [
            "Balansräkning",
            "Tillgångar",
            "Omsättningstillgångar",
            "Kassa och bank"
        ]
    );
    let rad = koppling(1010).fält(true).rad();
    assert_eq!(
        rad.rubriker(),
        [
            "Balansräkning",
            "Tillgångar",
            "Anläggningstillgångar",
            "Immateriella anläggningstillgångar"
        ]
    );
    assert_eq!(
        koppling(3001).fält(true).rad().rubriker(),
        ["Resultaträkning"]
    );
}

#[test]
fn raderna_står_i_blankettens_ordning() {
    let rader: Vec<_> = ink2::rader().iter().map(|r| r.rad()).collect();
    assert_eq!(rader.len(), 77);
    assert_eq!(rader[..3], ["2.1", "2.2", "2.3"]);
    assert_eq!(rader[rader.len() - 2..], ["3.26", "3.27"]);
}

#[test]
fn tabellen_bär_sin_blankettutgåva() {
    assert_eq!(ink2::UTGÅVA, "INK2_P1-231002_241119");
}
