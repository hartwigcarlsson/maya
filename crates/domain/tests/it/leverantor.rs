use maya_domain::kontoplan::Kontonummer;
use maya_domain::leverantor::{Fält, NyLeverantör, OmvändSkattskyldighet};

fn nr(n: u16) -> Kontonummer {
    Kontonummer::new(n).unwrap()
}

fn konton() -> Vec<(Kontonummer, bool)> {
    vec![(nr(4010), true), (nr(5410), true), (nr(6110), false)]
}

fn kontorsvaror() -> NyLeverantör {
    NyLeverantör {
        namn: " Kontorsvaror AB ".into(),
        orgnr: "5560360793".into(),
        gatuadress: "Lagervägen 1".into(),
        postnummer: "41101".into(),
        postort: "Göteborg".into(),
        land: "se".into(),
        momsregnr: "SE 556036079301".into(),
        bankgiro: "50501055".into(),
        standardkonto: "5410".into(),
        ..Default::default()
    }
}

fn koder(l: &NyLeverantör) -> Vec<(Fält, &'static str)> {
    l.kontrollera(&konton())
        .unwrap_err()
        .into_iter()
        .map(|f| (f.fält, f.kod))
        .collect()
}

#[test]
fn en_svensk_leverantör_skrivs_på_ett_enhetligt_sätt() {
    let l = kontorsvaror().kontrollera(&konton()).unwrap();
    assert_eq!(l.namn, "Kontorsvaror AB");
    assert_eq!(l.orgnr, "556036-0793");
    assert_eq!(l.postnummer, "411 01");
    assert_eq!(l.land, "SE");
    assert_eq!(l.momsregnr, "SE556036079301");
    assert_eq!(l.bankgiro, "5050-1055");
    assert_eq!(l.standardkonto, Some(nr(5410)));
    assert_eq!(l.omvänd_skattskyldighet, OmvändSkattskyldighet::Ingen);
}

#[test]
fn bara_namnet_krävs_och_tomt_land_är_sverige() {
    let l = NyLeverantör {
        namn: "Fikabröd".into(),
        ..Default::default()
    }
    .kontrollera(&[])
    .unwrap();
    assert_eq!(l.land, "SE");
    assert_eq!(l.standardkonto, None);
    assert!(l.bankgiro.is_empty() && l.iban.is_empty());
    assert_eq!(
        koder(&NyLeverantör::default()),
        [(Fält::Namn, "namn_saknas")]
    );
}

#[test]
fn en_utländsk_leverantör_har_sitt_lands_nummer_och_iban() {
    let l = NyLeverantör {
        namn: "Hosting GmbH".into(),
        orgnr: "HRB 12345".into(),
        postnummer: "10115".into(),
        land: "DE".into(),
        momsregnr: "de123456789".into(),
        iban: "DE89 3704 0044 0532 0130 00".into(),
        bic: "DEUTDEFF".into(),
        omvänd_skattskyldighet: "eu_tjanst".into(),
        ..Default::default()
    }
    .kontrollera(&[])
    .unwrap();
    assert_eq!(l.orgnr, "HRB 12345");
    assert_eq!(l.postnummer, "10115");
    assert_eq!(l.momsregnr, "DE123456789");
    assert_eq!(l.iban, "DE89 3704 0044 0532 0130 00");
    assert_eq!(l.omvänd_skattskyldighet, OmvändSkattskyldighet::EuTjänst);
}

#[test]
fn svenska_nummer_prövas_med_samma_regler_som_företagets() {
    let l = NyLeverantör {
        orgnr: "556036-0794".into(),
        postnummer: "4110".into(),
        momsregnr: "SE5560360793".into(),
        ..kontorsvaror()
    };
    assert_eq!(
        koder(&l),
        [
            (Fält::Orgnr, "orgnr_kontrollsiffra"),
            (Fält::Postnummer, "postnummer_ogiltigt"),
            (Fält::Momsregnr, "momsregnr_ogiltigt"),
        ]
    );
}

#[test]
fn landet_är_en_landskod_med_två_bokstäver() {
    let l = NyLeverantör {
        land: "Sverige".into(),
        ..kontorsvaror()
    };
    assert_eq!(koder(&l), [(Fält::Land, "land_ogiltigt")]);
}

#[test]
fn betaluppgifterna_prövas_som_företagets_betalsätt() {
    let l = NyLeverantör {
        bankgiro: "5050-1056".into(),
        bic: "ESSESESS".into(),
        ..kontorsvaror()
    };
    assert_eq!(
        koder(&l),
        [
            (Fält::Bankgiro, "bankgiro_kontrollsiffra"),
            (Fält::Iban, "iban_saknas"),
        ]
    );
    assert_eq!(Fält::Iban.namn(), "iban");
}

#[test]
fn standardkontot_är_ett_aktivt_konto_i_kontoplanen() {
    let med = |konto: &str| NyLeverantör {
        standardkonto: konto.into(),
        ..kontorsvaror()
    };
    assert_eq!(koder(&med("54")), [(Fält::Standardkonto, "konto_ogiltigt")]);
    assert_eq!(
        koder(&med("5460")),
        [(Fält::Standardkonto, "konto_finns_inte")]
    );
    assert_eq!(
        koder(&med("6110")),
        [(Fält::Standardkonto, "konto_inaktivt")]
    );
}

#[test]
fn omvänd_skattskyldighet_har_fem_val() {
    let koder: Vec<&str> = OmvändSkattskyldighet::ALLA
        .iter()
        .map(|o| o.kod())
        .collect();
    assert_eq!(
        koder,
        ["ingen", "eu_tjanst", "eu_vara", "bygg", "utanfor_eu"]
    );
    assert_eq!(OmvändSkattskyldighet::Bygg.namn(), "Bygg");
    let l = NyLeverantör {
        omvänd_skattskyldighet: "okänd".into(),
        ..kontorsvaror()
    };
    assert_eq!(
        self::koder(&l),
        [(Fält::OmvändSkattskyldighet, "val_saknas")]
    );
}
