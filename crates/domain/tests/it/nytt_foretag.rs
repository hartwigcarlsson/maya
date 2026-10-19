use jiff::civil::date;
use maya_domain::foretag::{Bokföringsmetod, Fält, Momsperiod, NyttFöretag};

fn formulär() -> NyttFöretag {
    NyttFöretag {
        orgnr: "556036-0793".into(),
        namn: "  Exempel AB ".into(),
        första_år_start: "2026-01-01".into(),
        första_år_slut: "2026-12-31".into(),
        bokföringsmetod: "faktura".into(),
        momsperiod: "kvartal".into(),
    }
}

#[test]
fn ett_giltigt_formulär_ger_företagets_uppgifter() {
    let giltigt = formulär().kontrollera().unwrap();

    assert_eq!(giltigt.orgnr.siffror(), "5560360793");
    assert_eq!(giltigt.namn, "Exempel AB");
    assert_eq!(giltigt.första_år.start(), date(2026, 1, 1));
    assert_eq!(giltigt.första_år.slut(), date(2026, 12, 31));
    assert_eq!(giltigt.bokföringsmetod, Bokföringsmetod::Faktura);
    assert_eq!(giltigt.momsperiod, Momsperiod::Kvartal);
}

#[test]
fn varje_fel_visas_vid_sitt_fält_med_en_kod() {
    let fel = NyttFöretag {
        orgnr: "556036-0794".into(),
        namn: " ".into(),
        första_år_start: "".into(),
        första_år_slut: "2026-12-30".into(),
        bokföringsmetod: "".into(),
        momsperiod: "vecka".into(),
    }
    .kontrollera()
    .unwrap_err();

    let koder: Vec<_> = fel.iter().map(|f| (f.fält, f.kod)).collect();
    assert_eq!(
        koder,
        [
            (Fält::Orgnr, "orgnr_kontrollsiffra"),
            (Fält::Namn, "namn_saknas"),
            (Fält::FörstaÅrStart, "datum_ogiltigt"),
            (Fält::Bokföringsmetod, "val_saknas"),
            (Fält::Momsperiod, "val_saknas"),
        ]
    );
    assert!(fel.iter().all(|f| !f.text.is_empty()));
}

#[test]
fn räkenskapsårets_fel_visas_vid_slutdatumet() {
    let mut f = formulär();
    f.första_år_slut = "2027-07-31".into();

    let fel = f.kontrollera().unwrap_err();

    assert_eq!(fel.len(), 1);
    assert_eq!(fel[0].fält, Fält::FörstaÅrSlut);
    assert_eq!(fel[0].kod, "ar_for_langt");
}

#[test]
fn ej_momsregistrerad_är_en_momsperiod() {
    let mut f = formulär();
    f.momsperiod = "ej_momsregistrerad".into();
    f.bokföringsmetod = "kontant".into();

    let giltigt = f.kontrollera().unwrap();

    assert_eq!(giltigt.momsperiod, Momsperiod::EjMomsregistrerad);
    assert_eq!(giltigt.bokföringsmetod, Bokföringsmetod::Kontant);
}

#[test]
fn valen_har_koder_och_namn() {
    for m in Bokföringsmetod::ALLA {
        assert_eq!(Bokföringsmetod::från_kod(m.kod()), Some(m));
        assert!(!m.namn().is_empty());
    }
    for m in Momsperiod::ALLA {
        assert_eq!(Momsperiod::från_kod(m.kod()), Some(m));
        assert!(!m.namn().is_empty());
    }
    assert_eq!(Momsperiod::ALLA.len(), 4);
}

#[test]
fn fälten_har_namn_som_i_formuläret() {
    assert_eq!(Fält::Orgnr.namn(), "orgnr");
    assert_eq!(Fält::FörstaÅrSlut.namn(), "slut");
}
