use maya_domain::orgnr::{Organisationsnummer, OrgnrFel};

#[test]
fn ett_giltigt_nummer_godtas_med_eller_utan_bindestreck() {
    let nr = Organisationsnummer::tolka("556036-0793").unwrap();
    assert_eq!(nr.siffror(), "5560360793");
    assert_eq!(nr.to_string(), "556036-0793");
    assert_eq!(Organisationsnummer::tolka(" 5560360793 ").unwrap(), nr);
    assert!(Organisationsnummer::tolka("212000-0142").is_ok());
}

#[test]
fn ett_nummer_utan_tio_siffror_nekas() {
    for fel in [
        "",
        "556036-079",
        "556036-07931",
        "55603607 93x",
        "abcdefghij",
    ] {
        assert_eq!(
            Organisationsnummer::tolka(fel),
            Err(OrgnrFel::Längd),
            "{fel}"
        );
    }
}

#[test]
fn ett_nummer_med_fel_kontrollsiffra_nekas() {
    assert_eq!(
        Organisationsnummer::tolka("556036-0794"),
        Err(OrgnrFel::Kontrollsiffra)
    );
}

#[test]
fn ett_nummer_vars_tredje_siffra_är_under_2_nekas() {
    // Ett personnummer klarar Luhn men har månaden på tredje och fjärde plats.
    assert_eq!(
        Organisationsnummer::tolka("800101-0019"),
        Err(OrgnrFel::TredjeSiffran)
    );
}

#[test]
fn varje_fel_har_en_kod_och_en_text() {
    for (fel, kod) in [
        (OrgnrFel::Längd, "orgnr_langd"),
        (OrgnrFel::Kontrollsiffra, "orgnr_kontrollsiffra"),
        (OrgnrFel::TredjeSiffran, "orgnr_tredje_siffran"),
    ] {
        assert_eq!(fel.kod(), kod);
        assert_eq!(OrgnrFel::från_kod(kod), Some(fel));
        assert!(!fel.to_string().is_empty());
    }
}
