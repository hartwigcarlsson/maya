//! Urvalet av konton i huvudboken, som användaren skriver det.

use maya_domain::kontoplan::Kontonummer;
use maya_domain::kontourval::Kontourval;

fn nr(n: u16) -> Kontonummer {
    Kontonummer::new(n).unwrap()
}

#[test]
fn ett_tomt_urval_omfattar_alla_konton() {
    let urval = Kontourval::tolka("  ").unwrap();
    assert_eq!(urval, Kontourval::alla());
    assert!(urval.omfattar(nr(1000)) && urval.omfattar(nr(8999)));
    assert_eq!(urval.to_string(), "");
}

#[test]
fn konton_och_intervall_skiljs_med_komma() {
    let urval = Kontourval::tolka("1930, 3000-3999,2610 – 2650").unwrap();

    for omfattas in [1930, 3000, 3500, 3999, 2610, 2640, 2650] {
        assert!(urval.omfattar(nr(omfattas)), "{omfattas}");
    }
    for utanför in [1910, 2999, 4000, 2600, 2660] {
        assert!(!urval.omfattar(nr(utanför)), "{utanför}");
    }
    assert_eq!(urval.to_string(), "1930, 3000–3999, 2610–2650");
}

#[test]
fn ett_baklänges_intervall_vänds() {
    let urval = Kontourval::tolka("3999-3000").unwrap();
    assert!(urval.omfattar(nr(3500)));
    assert_eq!(urval.to_string(), "3000–3999");
}

#[test]
fn annat_än_kontonummer_nekas() {
    for fel in ["abc", "19300", "999", "1930-", "1930,,1940", "0-9999"] {
        assert!(Kontourval::tolka(fel).is_none(), "{fel}");
    }
}
