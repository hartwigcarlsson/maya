use maya_domain::kontoplan::{Kontonummer, Kontotyp};

fn nr(n: u16) -> Kontonummer {
    Kontonummer::new(n).unwrap()
}

#[test]
fn kontonumret_är_fyrsiffrigt_i_klass_1_till_8() {
    assert!(Kontonummer::new(999).is_none());
    assert!(Kontonummer::new(9000).is_none());
    assert_eq!(nr(1000).get(), 1000);
    assert_eq!(nr(8999).get(), 8999);
}

#[test]
fn kontoklassen_ur_numret_avgör_typen() {
    assert_eq!(nr(1930).klass(), 1);
    assert_eq!(nr(1930).typ(), Kontotyp::Tillgång);
    assert_eq!(nr(2099).typ(), Kontotyp::EgetKapitalOchSkuld);
    assert_eq!(nr(3001).typ(), Kontotyp::Intäkt);
    assert_eq!(nr(4010).typ(), Kontotyp::Kostnad);
    assert_eq!(nr(7010).typ(), Kontotyp::Kostnad);
    assert_eq!(nr(8999).klass(), 8);
    assert_eq!(nr(8310).typ(), Kontotyp::FinansiellPost);
}

#[test]
fn balanskonton_är_klass_1_och_2() {
    assert!(nr(2099).typ().balanskonto());
    assert!(!nr(3001).typ().balanskonto());
    assert!(!nr(8999).typ().balanskonto());
}
