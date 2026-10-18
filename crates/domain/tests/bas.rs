//! BAS 2026 som Maya har inbyggd. De förväntade talen är räknade för hand ur källfilerna i
//! `crates/domain/data/bas/`, inte med Mayas tolkare.

use maya_domain::kontoplan::{Kontonummer, bas};

#[test]
fn bas_2026_har_lika_många_konton_som_källan() {
    // 1 285 kontonummer står i XLSX:en, men 2120 och 2130 står både som huvud- och underkonto
    // och 2087 har två namn: 1 282 olika konton.
    assert_eq!(bas::konton().len(), 1282);
}

#[test]
fn grundkontona_är_2025_års_markering_på_2026_års_konton() {
    // 341 konton har ■ i BAS 2025; 1249, 1259, 1580, 2128 och 6410 finns inte i BAS 2026.
    let grundkonton = bas::konton().iter().filter(|k| k.grundkonto()).count();
    assert_eq!(grundkonton, 336);
}

#[test]
fn kontot_har_bas_namnet_och_grundkontomarkeringen() {
    let konto = |n| bas::konto(Kontonummer::new(n).unwrap()).unwrap();

    assert_eq!(konto(1930).namn(), "Företagskonto");
    assert!(konto(1930).grundkonto());
    assert_eq!(konto(1010).namn(), "Utvecklingsutgifter");
    assert!(!konto(1010).grundkonto());
    // Radbrytningen i utskriftslayouten hör inte till namnet.
    assert_eq!(
        konto(1080).namn(),
        "Pågående projekt och förskott för immateriella anläggningstillgångar"
    );
    // 2087 står två gånger; det första namnet gäller.
    assert_eq!(konto(2087).namn(), "Bunden överkursfond");
    assert!(bas::konto(Kontonummer::new(1249).unwrap()).is_none());
}
