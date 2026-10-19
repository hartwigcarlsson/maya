//! Kundregistrets regler: samma kontroll i formuläret och på servern.

use maya_domain::kund::{self, Fält, NyKund, OmvändSkattskyldighet, Raderingsspärr};

fn formulär() -> NyKund {
    NyKund {
        namn: " Kund AB ".into(),
        identitet: "5560360793".into(),
        adress: "Storgatan 1\n111 22 Stockholm".into(),
        epost: " Faktura@Kund.SE ".into(),
        momsregnr: "se 5560 3607 9301".into(),
        betalningsvillkor: "30".into(),
        omvänd_skattskyldighet: "ingen".into(),
    }
}

fn koder(f: &NyKund) -> Vec<(Fält, &'static str)> {
    f.kontrollera()
        .unwrap_err()
        .into_iter()
        .map(|e| (e.fält, e.kod))
        .collect()
}

#[test]
fn en_ifylld_kund_skrivs_på_ett_enhetligt_sätt() {
    let k = formulär().kontrollera().unwrap();

    assert_eq!(k.namn, "Kund AB");
    assert_eq!(k.identitet, "556036-0793");
    assert_eq!(k.adress, "Storgatan 1\n111 22 Stockholm");
    assert_eq!(k.epost, "faktura@kund.se");
    assert_eq!(k.momsregnr, "SE556036079301");
    assert_eq!(k.betalningsvillkor, 30);
    assert_eq!(k.omvänd_skattskyldighet, OmvändSkattskyldighet::Ingen);
}

#[test]
fn bara_namnet_krävs() {
    let k = NyKund {
        namn: "Kund AB".into(),
        ..NyKund::default()
    }
    .kontrollera()
    .unwrap();

    assert_eq!(k.identitet, "");
    assert_eq!(k.epost, "");
    assert_eq!(k.betalningsvillkor, 30);
    assert_eq!(k.omvänd_skattskyldighet, OmvändSkattskyldighet::Ingen);
}

#[test]
fn en_kund_utan_namn_nekas() {
    let f = NyKund {
        namn: "  ".into(),
        ..formulär()
    };
    assert_eq!(koder(&f), [(Fält::Namn, "namn_saknas")]);
}

#[test]
fn ett_personnummer_godtas_med_tio_eller_tolv_siffror_och_samordningsnummer() {
    for (text, väntat) in [
        ("811218-9876", "811218-9876"),
        ("8112189876", "811218-9876"),
        ("19811218-9876", "19811218-9876"),
        ("198112189876", "19811218-9876"),
        ("701063-2391", "701063-2391"),
    ] {
        let k = NyKund {
            identitet: text.into(),
            ..formulär()
        }
        .kontrollera()
        .unwrap();
        assert_eq!(k.identitet, väntat, "{text}");
    }
}

#[test]
fn ett_nummer_med_fel_kontrollsiffra_längd_eller_datum_nekas() {
    for (text, kod) in [
        ("556036-0794", "identitet_kontrollsiffra"),
        ("811218-9875", "identitet_kontrollsiffra"),
        ("19811218-9875", "identitet_kontrollsiffra"),
        ("55603607", "identitet_format"),
        ("5560360793x", "identitet_format"),
        ("811318-9876", "personnr_datum"),
    ] {
        let f = NyKund {
            identitet: text.into(),
            ..formulär()
        };
        assert_eq!(koder(&f), [(Fält::Identitet, kod)], "{text}");
    }
}

#[test]
fn en_ogiltig_e_postadress_nekas() {
    let f = NyKund {
        epost: "faktura@".into(),
        ..formulär()
    };
    assert_eq!(koder(&f), [(Fält::Epost, "epost_ogiltig")]);
}

#[test]
fn betalningsvillkoret_är_ett_helt_antal_dagar() {
    for text in ["", "tretti", "-5", "2,5"] {
        let f = NyKund {
            betalningsvillkor: text.into(),
            ..formulär()
        };
        assert_eq!(
            koder(&f),
            [(Fält::Betalningsvillkor, "betalningsvillkor_ogiltigt")],
            "{text}"
        );
    }
    let k = NyKund {
        betalningsvillkor: " 0 ".into(),
        ..formulär()
    }
    .kontrollera()
    .unwrap();
    assert_eq!(k.betalningsvillkor, 0);
}

#[test]
fn omvänd_skattskyldighet_är_ingen_bygg_eller_eu_tjänst() {
    for (kod, väntad) in [
        ("ingen", OmvändSkattskyldighet::Ingen),
        ("bygg", OmvändSkattskyldighet::Bygg),
        ("eu_tjanst", OmvändSkattskyldighet::EuTjänst),
    ] {
        assert_eq!(OmvändSkattskyldighet::från_kod(kod), Some(väntad));
        assert_eq!(väntad.kod(), kod);
    }
    let f = NyKund {
        omvänd_skattskyldighet: "export".into(),
        ..formulär()
    };
    assert_eq!(koder(&f), [(Fält::OmvändSkattskyldighet, "val_saknas")]);
}

#[test]
fn felen_kommer_i_fältens_ordning() {
    let f = NyKund {
        namn: String::new(),
        identitet: "1".into(),
        epost: "x".into(),
        betalningsvillkor: "x".into(),
        ..formulär()
    };
    assert_eq!(
        koder(&f).into_iter().map(|(f, _)| f).collect::<Vec<_>>(),
        [
            Fält::Namn,
            Fält::Identitet,
            Fält::Epost,
            Fält::Betalningsvillkor
        ]
    );
}

#[test]
fn en_kund_med_fakturor_kan_inte_raderas() {
    assert_eq!(kund::kan_raderas(0), Ok(()));
    assert_eq!(kund::kan_raderas(1), Err(Raderingsspärr::HarFakturor));
    assert_eq!(Raderingsspärr::HarFakturor.kod(), "har_fakturor");
}
