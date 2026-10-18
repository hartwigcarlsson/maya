use maya_domain::kontoplan::{Kontonummer, bas, flöde_som_bokför_på, flödenas_konton};

#[test]
fn preliminär_ingående_balans_bokför_på_2099() {
    let nr = Kontonummer::new(2099).unwrap();
    assert_eq!(flöde_som_bokför_på(nr), Some("Preliminär ingående balans"));
    assert_eq!(flöde_som_bokför_på(Kontonummer::new(1930).unwrap()), None);
}

#[test]
fn flödenas_konton_finns_i_bas() {
    assert!(!flödenas_konton().is_empty());
    for (nr, _) in flödenas_konton() {
        assert!(bas::konto(*nr).is_some(), "{nr} finns inte i BAS");
    }
}
