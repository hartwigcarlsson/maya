use maya_domain::betalsatt::{
    Betalsättsfel, tolka_bankgiro, tolka_bic, tolka_iban, tolka_plusgiro,
};

#[test]
fn ett_bankgironummer_har_sju_eller_åtta_siffror_och_skrivs_med_bindestreck() {
    assert_eq!(tolka_bankgiro("5050-1055").unwrap(), "5050-1055");
    assert_eq!(tolka_bankgiro(" 50501055 ").unwrap(), "5050-1055");
    assert_eq!(tolka_bankgiro("123 4566").unwrap(), "123-4566");
    for fel in ["", "123456", "123456789", "5050-105x"] {
        assert_eq!(tolka_bankgiro(fel), Err(Betalsättsfel::Bankgiro), "{fel}");
    }
}

#[test]
fn ett_bankgironummer_med_fel_kontrollsiffra_nekas() {
    assert_eq!(
        tolka_bankgiro("5050-1056"),
        Err(Betalsättsfel::BankgiroKontrollsiffra)
    );
    assert_eq!(
        Betalsättsfel::BankgiroKontrollsiffra.kod(),
        "bankgiro_kontrollsiffra"
    );
}

#[test]
fn ett_plusgironummer_har_två_till_åtta_siffror_med_kontrollsiffran_efter_bindestreck() {
    assert_eq!(tolka_plusgiro("1 23 45-5").unwrap(), "12345-5");
    assert_eq!(tolka_plusgiro("123455").unwrap(), "12345-5");
    assert_eq!(tolka_plusgiro("18").unwrap(), "1-8");
    for fel in ["", "1", "123456789", "12x45-5"] {
        assert_eq!(tolka_plusgiro(fel), Err(Betalsättsfel::Plusgiro), "{fel}");
    }
    assert_eq!(
        tolka_plusgiro("12345-6"),
        Err(Betalsättsfel::PlusgiroKontrollsiffra)
    );
}

#[test]
fn ett_iban_nummer_prövas_med_modulo_97_och_skrivs_i_grupper_om_fyra() {
    assert_eq!(
        tolka_iban("se4550000000058398257466").unwrap(),
        "SE45 5000 0000 0583 9825 7466"
    );
    assert_eq!(
        tolka_iban("SE45 5000 0000 0583 9825 7466").unwrap(),
        "SE45 5000 0000 0583 9825 7466"
    );
    assert_eq!(
        tolka_iban("SE46 5000 0000 0583 9825 7466"),
        Err(Betalsättsfel::IbanKontrollsiffra)
    );
    // Ett svenskt IBAN-nummer har 24 tecken.
    for fel in [
        "",
        "SE45",
        "4550000000058398257466SE",
        "SE45 5000 0000 0583 9825 746",
        "SE45-5000",
    ] {
        assert_eq!(tolka_iban(fel), Err(Betalsättsfel::Iban), "{fel}");
    }
}

#[test]
fn en_bic_har_åtta_eller_elva_tecken_med_land_på_femte_och_sjätte_platsen() {
    assert_eq!(tolka_bic("essesess").unwrap(), "ESSESESS");
    assert_eq!(tolka_bic(" NDEA SE SS XXX ").unwrap(), "NDEASESSXXX");
    for fel in [
        "",
        "ESSESES",
        "ESSESESSX",
        "ESS1SESS",
        "ESSE5ESS",
        "ESSESES!",
    ] {
        assert_eq!(tolka_bic(fel), Err(Betalsättsfel::Bic), "{fel}");
    }
}
