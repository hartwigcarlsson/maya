use maya_domain::luhn;

#[test]
fn luhn_ger_kontrollsiffran_och_prövar_ett_nummer_med_den_sist() {
    assert_eq!(luhn::kontrollsiffra("556036079"), Some(3));
    assert_eq!(luhn::kontrollsiffra("5050105"), Some(5));
    assert_eq!(luhn::kontrollsiffra(""), None);
    assert_eq!(luhn::kontrollsiffra("12a"), None);
    assert!(luhn::stämmer("5560360793"));
    assert!(!luhn::stämmer("5560360794"));
    assert!(!luhn::stämmer("5"));
}
