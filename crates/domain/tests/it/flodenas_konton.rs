use maya_domain::kontoplan::{Kontonummer, bas, flöde_som_bokför_på, flödenas_konton};

#[test]
fn preliminär_ingående_balans_bokför_på_2099() {
    let nr = Kontonummer::new(2099).unwrap();
    assert_eq!(flöde_som_bokför_på(nr), Some("Preliminär ingående balans"));
    assert_eq!(flöde_som_bokför_på(Kontonummer::new(1910).unwrap()), None);
}

#[test]
fn fakturaflödenas_konton_är_flödets_konton() {
    // Specen för etapp 2: "Flödenas konton som läggs till i listan i domain".
    for nr in [
        1510, 1930, 2440, 2611, 2621, 2631, 2614, 2624, 2634, 2641, 2645, 2647, 3001, 3002, 3003,
        3004, 3231, 3308, 3740, 3960, 6351, 7960,
    ] {
        let konto = Kontonummer::new(nr).unwrap();
        assert!(
            flöde_som_bokför_på(konto).is_some(),
            "{nr} är inget flödes konto"
        );
    }
    assert_eq!(
        flöde_som_bokför_på(Kontonummer::new(1510).unwrap()),
        Some("Kundfakturor")
    );
}

#[test]
fn flödenas_konton_står_i_nummerordning() {
    let nummer: Vec<u16> = flödenas_konton().iter().map(|(n, _)| n.get()).collect();
    let mut sorterade = nummer.clone();
    sorterade.sort();
    sorterade.dedup();
    assert_eq!(nummer, sorterade);
}

#[test]
fn flödenas_konton_finns_i_bas() {
    assert!(!flödenas_konton().is_empty());
    for (nr, _) in flödenas_konton() {
        assert!(bas::konto(*nr).is_some(), "{nr} finns inte i BAS");
    }
}
