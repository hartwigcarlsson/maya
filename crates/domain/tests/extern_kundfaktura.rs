//! Spärrarna och konteringen för en extern kundfaktura (#143).

use jiff::civil::date;
use maya_domain::extern_kundfaktura::{
    Fält, Förutsättningar, Krediterar, NyExternFaktura, NyExternRad, REGISTRERINGSSPÄRRAR, Radfält,
};
use maya_domain::foretag::Momsperiod;
use maya_domain::kontoplan::Kontonummer;
use maya_domain::kund::{self, NyKund, OmvändSkattskyldighet};
use maya_domain::kundfaktura::{Fakturakund, NY};
use maya_domain::rakenskapsar::Räkenskapsår;

fn nr(n: u16) -> Kontonummer {
    Kontonummer::new(n).unwrap()
}

fn kund() -> Fakturakund {
    Fakturakund {
        id: 7,
        namn: "Kund AB".into(),
        adress: "Storgatan 1".into(),
        betalningsvillkor: 30,
        omvänd_skattskyldighet: OmvändSkattskyldighet::Ingen,
        ..Fakturakund::default()
    }
}

fn förutsättningar() -> Förutsättningar {
    Förutsättningar {
        kunder: vec![kund()],
        konton: vec![
            (nr(1510), true),
            (nr(2611), true),
            (nr(2621), true),
            (nr(3001), true),
            (nr(3002), true),
            (nr(3004), true),
            (nr(3010), false),
            (nr(5410), true),
        ],
        räkenskapsår: vec![Räkenskapsår::befintligt(
            date(2026, 1, 1),
            date(2026, 12, 31),
        )],
        momsperioder: vec![(date(2026, 1, 1), Momsperiod::Kvartal)],
        ..Förutsättningar::default()
    }
}

fn rad(momssats: &str, konto: &str, belopp: &str) -> NyExternRad {
    NyExternRad {
        momssats: momssats.into(),
        konto: konto.into(),
        belopp: belopp.into(),
    }
}

fn faktura() -> NyExternFaktura {
    NyExternFaktura {
        kund: "7".into(),
        fakturanummer: "F-2026-17".into(),
        fakturadatum: "2026-03-10".into(),
        förfallodag: String::new(),
        rader: vec![
            rad("25", "3001", "8 000"),
            rad("12", "3002", "1 000,50"),
            NyExternRad::default(),
        ],
        ..NyExternFaktura::default()
    }
}

fn koder(f: &NyExternFaktura, fö: &Förutsättningar) -> Vec<&'static str> {
    f.kontrollera(fö)
        .err()
        .unwrap_or_default()
        .iter()
        .map(|s| s.kod)
        .collect()
}

#[test]
fn registreringen_konteras_som_ett_utfärdande_med_moms_per_sats() {
    let r = faktura().kontrollera(&förutsättningar()).unwrap();
    // 8 000 + 25 % = 2 000; 1 000,50 + 12 % = 120,06.
    assert_eq!((r.netto, r.moms, r.total), (900_050, 212_006, 1_112_056));
    assert_eq!(
        r.förfallodag,
        date(2026, 4, 9),
        "fakturadatum plus 30 dagar"
    );
    let rader: Vec<(u16, i64, i64)> = r
        .kontering()
        .iter()
        .map(|r| (r.konto.get(), r.debet.öre(), r.kredit.öre()))
        .collect();
    assert_eq!(
        rader,
        vec![
            (1510, 1_112_056, 0),
            (3001, 0, 800_000),
            (3002, 0, 100_050),
            (2611, 0, 200_000),
            (2621, 0, 12_006),
        ]
    );
    assert_eq!(r.beskrivning(), "Extern kundfaktura F-2026-17, Kund AB");
}

#[test]
fn ett_nummer_som_finns_bland_de_externa_fakturorna_spärras_oavsett_skiftläge() {
    let mut fö = förutsättningar();
    fö.upptagna = vec!["f-2026-17".into()];
    assert_eq!(koder(&faktura(), &fö), vec!["dubblett"]);
    fö.upptagna = vec!["F-2026-18".into()];
    assert!(faktura().kontrollera(&fö).is_ok());
}

#[test]
fn varje_fält_som_saknas_spärras_vid_sitt_fält() {
    let f = NyExternFaktura {
        rader: vec![rad("25", "", ""), rad("7", "3010", "x")],
        ..NyExternFaktura::default()
    };
    let spärrar = f.kontrollera(&förutsättningar()).unwrap_err();
    let fält: Vec<(Fält, &str)> = spärrar.iter().map(|s| (s.fält, s.kod)).collect();
    assert_eq!(
        fält,
        vec![
            (Fält::Kund, "kund_saknas"),
            (Fält::Fakturanummer, "fakturanummer_saknas"),
            (Fält::Fakturadatum, "fakturadatum_saknas"),
            (Fält::Rad(1, Radfält::Momssats), "rad_momssats_ogiltig"),
            (Fält::Rad(1, Radfält::Konto), "konto_inaktivt"),
            (Fält::Rad(1, Radfält::Belopp), "rad_belopp_ogiltigt"),
        ]
    );
    let f = NyExternFaktura {
        rader: vec![rad("25", "5410", "")],
        förfallodag: "2026-03-01".into(),
        ..faktura()
    };
    assert_eq!(
        koder(&f, &förutsättningar()),
        vec![
            "forfallodag_fore_fakturadatum",
            "konto_inte_intakt",
            "rad_belopp_saknas"
        ]
    );
    let f = NyExternFaktura {
        rader: vec![],
        fakturadatum: "2025-12-31".into(),
        ..faktura()
    };
    assert_eq!(
        koder(&f, &förutsättningar()),
        vec!["fore_forsta_aret", "rader_saknas"]
    );
}

#[test]
fn en_vanlig_faktura_har_en_positiv_summa() {
    let f = NyExternFaktura {
        rader: vec![rad("25", "3001", "-100")],
        ..faktura()
    };
    assert_eq!(koder(&f, &förutsättningar()), vec!["summa_inte_positiv"]);
}

#[test]
fn ett_företag_utan_momsregistrering_har_0_procent_på_varje_rad() {
    let mut fö = förutsättningar();
    fö.momsperioder = vec![(date(2026, 1, 1), Momsperiod::EjMomsregistrerad)];
    assert_eq!(
        koder(&faktura(), &fö),
        vec!["momssats_ej_momsregistrerad", "momssats_ej_momsregistrerad"]
    );
    let f = NyExternFaktura {
        rader: vec![rad("0", "3004", "500")],
        ..faktura()
    };
    let r = f.kontrollera(&fö).unwrap();
    assert_eq!((r.moms, r.total), (0, 50_000));
}

#[test]
fn en_extern_kreditfaktura_är_negativ_och_krediterar_högst_det_som_finns_kvar() {
    let mut fö = förutsättningar();
    fö.kunder.clear();
    fö.krediterar = Some(Krediterar {
        faktura: 3,
        fakturanummer: "F-2026-17".into(),
        kund: kund(),
        kvar: 125_000,
    });
    let kredit = |belopp: &str| NyExternFaktura {
        kund: String::new(),
        fakturanummer: "K-4".into(),
        rader: vec![rad("25", "3001", belopp)],
        ..faktura()
    };
    let r = kredit("-1 000").kontrollera(&fö).unwrap();
    assert_eq!(r.total, -125_000);
    assert_eq!(r.kund.namn, "Kund AB", "originalets kund");
    let rader: Vec<(u16, i64, i64)> = r
        .kontering()
        .iter()
        .map(|r| (r.konto.get(), r.debet.öre(), r.kredit.öre()))
        .collect();
    assert_eq!(
        rader,
        vec![(1510, 0, 125_000), (3001, 100_000, 0), (2611, 25_000, 0)]
    );
    assert_eq!(
        r.beskrivning(),
        "Extern kreditfaktura K-4, Kund AB, krediterar faktura F-2026-17"
    );
    assert_eq!(koder(&kredit("100"), &fö), vec!["summa_inte_negativ"]);
    assert_eq!(koder(&kredit("-1 000,01"), &fö), vec!["kredit_overstiger"]);
}

#[test]
fn en_upplåst_faktura_får_inte_en_total_under_det_som_betalats() {
    let mut fö = förutsättningar();
    fö.betalt = 1_112_057;
    assert_eq!(koder(&faktura(), &fö), vec!["under_betalt"]);
    fö.betalt = 1_112_056;
    assert!(faktura().kontrollera(&fö).is_ok());
}

#[test]
fn ett_utkast_sparas_ofullständigt_och_läses_tillbaka_som_formulär() {
    let f = NyExternFaktura {
        kund: String::new(),
        fakturadatum: String::new(),
        rader: vec![rad("25", "3010", "-12,50"), NyExternRad::default()],
        ..faktura()
    };
    let u = f.som_utkast().unwrap();
    assert_eq!(u.som_formulär().rader, vec![rad("25", "3010", "-12,50")]);
    assert_eq!(u.som_formulär().fakturanummer, "F-2026-17");
    let fel = NyExternFaktura {
        fakturadatum: "10 mars".into(),
        rader: vec![rad("25", "x", "1")],
        ..faktura()
    };
    let koder: Vec<&str> = fel
        .som_utkast()
        .unwrap_err()
        .iter()
        .map(|s| s.kod)
        .collect();
    assert_eq!(koder, vec!["fakturadatum_ogiltigt", "konto_okant"]);
}

#[test]
fn varje_spärrkod_står_i_systemdokumentationens_tabell() {
    for kod in [
        "kund_saknas",
        "fakturanummer_saknas",
        "dubblett",
        "under_betalt",
        "kredit_overstiger",
        "momssats_ej_momsregistrerad",
    ] {
        assert!(REGISTRERINGSSPÄRRAR.iter().any(|(k, _)| *k == kod), "{kod}");
    }
}

// #153 Ny kund från det externa utkastet

fn med_ny_kund(ny: NyKund) -> NyExternFaktura {
    NyExternFaktura {
        kund: NY.into(),
        ny_kund: ny,
        ..faktura()
    }
}

#[test]
fn en_ny_kund_från_utkastet_registreras_med_kundregistrets_regler() {
    let ny = NyKund {
        namn: "Ny Kund AB".into(),
        betalningsvillkor: "10".into(),
        ..Default::default()
    };
    let r = med_ny_kund(ny).kontrollera(&förutsättningar()).unwrap();
    assert_eq!(r.kund.namn, "Ny Kund AB");
    assert_eq!(
        r.kund.id, 0,
        "inget id förrän core lägger kunden i registret"
    );
    assert_eq!(
        r.ny_kund.as_ref().map(|k| k.namn.as_str()),
        Some("Ny Kund AB")
    );
    assert_eq!(r.förfallodag, date(2026, 3, 20), "den nya kundens villkor");
    assert_eq!(r.beskrivning(), "Extern kundfaktura F-2026-17, Ny Kund AB");

    let fel = med_ny_kund(NyKund {
        epost: "inte en adress".into(),
        ..Default::default()
    })
    .kontrollera(&förutsättningar())
    .unwrap_err();
    let fält: Vec<(Fält, &str)> = fel.iter().map(|s| (s.fält, s.kod)).collect();
    assert_eq!(
        fält,
        vec![
            (Fält::NyKund(kund::Fält::Namn), "ny_kund"),
            (Fält::NyKund(kund::Fält::Epost), "ny_kund"),
        ]
    );
    assert!(REGISTRERINGSSPÄRRAR.iter().any(|(k, _)| *k == "ny_kund"));
}

#[test]
fn en_ny_kund_sparas_med_utkastet_och_kommer_tillbaka_i_formuläret() {
    let ny = NyKund {
        namn: "Ny Kund AB".into(),
        epost: "a@ny.se".into(),
        betalningsvillkor: "30".into(),
        ..Default::default()
    };
    let u = med_ny_kund(ny.clone()).som_utkast().unwrap();
    assert_eq!(u.kund, None);
    assert_eq!(
        u.ny_kund.as_ref().map(|k| k.epost.as_str()),
        Some("a@ny.se")
    );
    let tillbaka = u.som_formulär();
    assert_eq!(tillbaka.kund, NY);
    assert_eq!(tillbaka.ny_kund.namn, "Ny Kund AB");
    // En ny kund utan namn är inget att spara.
    let tom = med_ny_kund(NyKund::default()).som_utkast().unwrap();
    assert_eq!((tom.kund, tom.ny_kund), (None, None));
    // En kreditfaktura går alltid till originalets kund.
    let mut fö = förutsättningar();
    fö.krediterar = Some(Krediterar {
        faktura: 3,
        fakturanummer: "F-1".into(),
        kund: kund(),
        kvar: 10_000_000,
    });
    let kredit = NyExternFaktura {
        rader: vec![rad("25", "3001", "-100")],
        ..med_ny_kund(ny)
    };
    let r = kredit.kontrollera(&fö).unwrap();
    assert_eq!((r.kund.id, r.ny_kund), (7, None));
}
