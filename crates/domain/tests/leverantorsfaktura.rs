//! Leverantörsfakturans regler: moms per rad, konteringen vid registrering under
//! fakturametoden, spärrarna och varningen för en trolig dubblett.

use jiff::civil::date;
use maya_domain::belopp::Belopp;
use maya_domain::foretag::Momsperiod;
use maya_domain::kontoplan::Kontonummer;
use maya_domain::leverantor::{NyLeverantör, OmvändSkattskyldighet};
use maya_domain::leverantorsfaktura::{
    Fält, Förutsättningar, Leverantörsval, NyLeverantörsfaktura, NyRad, REGISTRERINGSSPÄRRAR,
    Registrerad, moms, Åtgärd,
};
use maya_domain::rakenskapsar::Räkenskapsår;

fn nr(n: u16) -> Kontonummer {
    Kontonummer::new(n).unwrap()
}

fn förutsättningar() -> Förutsättningar {
    Förutsättningar {
        konton: vec![
            (nr(2440), true),
            (nr(2641), true),
            (nr(2645), true),
            (nr(2647), true),
            (nr(2614), true),
            (nr(2624), true),
            (nr(2634), true),
            (nr(5410), true),
            (nr(6110), true),
            (nr(6212), false),
        ],
        räkenskapsår: vec![Räkenskapsår::befintligt(
            date(2026, 1, 1),
            date(2026, 12, 31),
        )],
        momsperioder: vec![(date(2026, 1, 1), Momsperiod::Kvartal)],
        leverantörer: vec![Leverantörsval {
            id: "7".into(),
            namn: "Kontorsvaror AB".into(),
            standardkonto: Some(nr(5410)),
            omvänd_skattskyldighet: OmvändSkattskyldighet::Ingen,
        }],
        registrerade: Vec::new(),
    }
}

fn rad(konto: &str, netto: &str, sats: &str) -> NyRad {
    NyRad {
        konto: konto.into(),
        netto: netto.into(),
        momssats: sats.into(),
        moms: String::new(),
    }
}

fn faktura() -> NyLeverantörsfaktura {
    NyLeverantörsfaktura {
        leverantör: "7".into(),
        fakturanummer: "8812".into(),
        fakturadatum: "2026-03-14".into(),
        förfallodag: "2026-04-13".into(),
        referens: "4455667788".into(),
        totalbelopp: "1 250,00".into(),
        rader: vec![rad("5410", "1000", "25")],
        ..NyLeverantörsfaktura::default()
    }
}

fn koder(f: &NyLeverantörsfaktura, fö: &Förutsättningar) -> Vec<&'static str> {
    f.kontrollera(fö)
        .err()
        .unwrap_or_default()
        .iter()
        .map(|s| s.kod)
        .collect()
}

/// Konto, debet och kredit i öre.
fn kontering(f: &NyLeverantörsfaktura, fö: &Förutsättningar) -> Vec<(u16, i64, i64)> {
    f.kontrollera(fö)
        .unwrap()
        .kontering()
        .iter()
        .map(|r| (r.konto.get(), r.debet.öre(), r.kredit.öre()))
        .collect()
}

#[test]
fn momsen_räknas_per_rad_och_avrundas_till_öre() {
    assert_eq!(
        moms(Belopp::från_öre(100_000), 25),
        Belopp::från_öre(25_000)
    );
    // 10,01 × 12 % = 1,2012 → 1,20; 0,50 × 25 % = 0,125 → 0,13.
    assert_eq!(moms(Belopp::från_öre(1_001), 12), Belopp::från_öre(120));
    assert_eq!(moms(Belopp::från_öre(50), 25), Belopp::från_öre(13));
    assert_eq!(moms(Belopp::från_öre(1_000), 0), Belopp::NOLL);
}

#[test]
fn registreringen_bokför_kostnad_per_rad_och_2641_mot_2440() {
    let mut f = faktura();
    f.totalbelopp = "1 362,00".into();
    f.rader.push(rad("6110", "100", "12"));
    assert_eq!(
        kontering(&f, &förutsättningar()),
        [
            (5410, 100_000, 0),
            (6110, 10_000, 0),
            (2641, 26_200, 0),
            (2440, 0, 136_200)
        ]
    );
}

#[test]
fn ett_överskrivet_momsbelopp_bokförs_som_det_står() {
    let mut f = faktura();
    f.rader[0].moms = "249,99".into();
    f.totalbelopp = "1 249,99".into();
    assert_eq!(
        kontering(&f, &förutsättningar()),
        [(5410, 100_000, 0), (2641, 24_999, 0), (2440, 0, 124_999)]
    );
}

#[test]
fn ett_företag_utan_momsregistrering_bokför_momsen_på_kostnadskontot() {
    let mut fö = förutsättningar();
    fö.momsperioder
        .push((date(2026, 3, 1), Momsperiod::EjMomsregistrerad));
    assert_eq!(
        kontering(&faktura(), &fö),
        [(5410, 125_000, 0), (2440, 0, 125_000)]
    );
    // Momsperioden gäller på fakturadatumet.
    let mut f = faktura();
    f.fakturadatum = "2026-02-27".into();
    assert_eq!(
        kontering(&f, &fö),
        [(5410, 100_000, 0), (2641, 25_000, 0), (2440, 0, 125_000)]
    );
}

#[test]
fn leverantörens_standardkonto_gäller_på_rader_utan_konto() {
    let mut f = faktura();
    f.rader[0].konto = String::new();
    assert_eq!(kontering(&f, &förutsättningar())[0], (5410, 100_000, 0));
}

#[test]
fn en_ny_leverantör_från_utkastet_prövas_med_registrets_regler() {
    let mut f = faktura();
    f.leverantör = "ny".into();
    f.rader[0].konto = String::new();
    f.ny_leverantör = NyLeverantör {
        namn: "Tryckeriet AB".into(),
        bankgiro: "123".into(),
        standardkonto: "6110".into(),
        ..NyLeverantör::default()
    };
    let spärrar = f.kontrollera(&förutsättningar()).unwrap_err();
    assert_eq!(spärrar.len(), 1, "{spärrar:?}");
    assert_eq!(spärrar[0].kod, "ny_leverantor");
    f.ny_leverantör.bankgiro = "5050-1055".into();
    // Den nya leverantörens standardkonto fyller tomma rader.
    assert_eq!(kontering(&f, &förutsättningar())[0], (6110, 100_000, 0));
}

#[test]
fn leverantör_saknas_spärrar() {
    let mut f = faktura();
    f.leverantör = String::new();
    assert!(koder(&f, &förutsättningar()).contains(&"leverantor_saknas"));
    f.leverantör = "99".into();
    assert!(koder(&f, &förutsättningar()).contains(&"leverantor_saknas"));
}

#[test]
fn obligatoriska_fält_spärrar() {
    let f = NyLeverantörsfaktura {
        leverantör: "7".into(),
        rader: vec![NyRad {
            momssats: "25".into(),
            ..NyRad::default()
        }],
        ..NyLeverantörsfaktura::default()
    };
    let koder = koder(&f, &förutsättningar());
    for kod in [
        "fakturanummer_saknas",
        "fakturadatum_saknas",
        "forfallodag_saknas",
        "total_saknas",
        "rader_saknas",
    ] {
        assert!(koder.contains(&kod), "{kod} saknas i {koder:?}");
    }
}

#[test]
fn förfallodag_före_fakturadatum_spärrar() {
    let mut f = faktura();
    f.förfallodag = "2026-03-13".into();
    assert_eq!(
        koder(&f, &förutsättningar()),
        ["forfallodag_fore_fakturadatum"]
    );
}

#[test]
fn rader_och_moms_som_inte_summerar_till_totalen_spärrar() {
    let mut f = faktura();
    f.totalbelopp = "1 200".into();
    let spärrar = f.kontrollera(&förutsättningar()).unwrap_err();
    assert_eq!(spärrar[0].kod, "summerar_inte");
    assert_eq!(spärrar[0].fält, Fält::Rader);
    assert_eq!(
        spärrar[0].text,
        "Raderna med moms blir 1 250,00 kr, totalen är 1 200,00 kr. Differens 50,00."
    );
}

#[test]
fn ett_inaktivt_konto_spärrar_med_aktivera() {
    let mut f = faktura();
    f.rader[0].konto = "6212".into();
    let spärrar = f.kontrollera(&förutsättningar()).unwrap_err();
    assert_eq!(spärrar[0].kod, "konto_inaktivt");
    assert_eq!(spärrar[0].fält, Fält::Konto(0));
    assert_eq!(spärrar[0].åtgärd, Some(Åtgärd::Aktivera(nr(6212))));
    f.rader[0].konto = "9999".into();
    assert_eq!(koder(&f, &förutsättningar()), ["konto_okant"]);
    f.rader[0].konto = "6990".into();
    assert_eq!(koder(&f, &förutsättningar()), ["konto_okant"]);
}

#[test]
fn ett_inaktivt_reskontrakonto_spärrar() {
    let mut fö = förutsättningar();
    fö.konton[0].1 = false;
    let spärrar = faktura().kontrollera(&fö).unwrap_err();
    assert_eq!(spärrar[0].kod, "konto_inaktivt");
    assert_eq!(spärrar[0].fält, Fält::Kontering);
}

#[test]
fn fakturadatum_utan_räkenskapsår_spärrar() {
    let mut f = faktura();
    f.fakturadatum = "2025-12-30".into();
    f.förfallodag = "2026-01-30".into();
    assert_eq!(koder(&f, &förutsättningar()), ["fore_forsta_aret"]);
    f.fakturadatum = "2028-01-02".into();
    f.förfallodag = "2028-02-02".into();
    assert_eq!(koder(&f, &förutsättningar()), ["saknat_rakenskapsar"]);
}

#[test]
fn samma_leverantör_och_nummer_spärrar_med_ett_suffix() {
    let mut fö = förutsättningar();
    for nummer in ["8812", "8812-2"] {
        fö.registrerade.push(Registrerad {
            id: nummer.into(),
            leverantör: "7".into(),
            fakturanummer: nummer.into(),
            fakturadatum: date(2026, 2, 1),
            total: Belopp::från_öre(5_000),
        });
    }
    let spärrar = faktura().kontrollera(&fö).unwrap_err();
    assert_eq!(spärrar[0].kod, "dubblett");
    assert_eq!(spärrar[0].fält, Fält::Fakturanummer);
    assert_eq!(
        spärrar[0].åtgärd,
        Some(Åtgärd::Fakturanummer("8812-3".into()))
    );
    // Samma nummer hos en annan leverantör är ingen dubblett.
    fö.registrerade
        .iter_mut()
        .for_each(|r| r.leverantör = "8".into());
    assert!(faktura().kontrollera(&fö).is_ok());
}

#[test]
fn samma_leverantör_belopp_och_datum_med_annat_nummer_varnar() {
    let mut fö = förutsättningar();
    fö.registrerade.push(Registrerad {
        id: "3".into(),
        leverantör: "7".into(),
        fakturanummer: "8811".into(),
        fakturadatum: date(2026, 3, 14),
        total: Belopp::från_öre(125_000),
    });
    let varningar = faktura().varningar(&fö);
    assert_eq!(varningar.len(), 1);
    assert_eq!(varningar[0].kod, "trolig_dubblett");
    assert!(varningar[0].text.contains("8811"), "{}", varningar[0].text);
    assert!(faktura().kontrollera(&fö).is_ok(), "varningen spärrar inte");
    let mut f = faktura();
    f.totalbelopp = "1 250,01".into();
    f.rader[0].moms = "250,01".into();
    assert!(f.varningar(&fö).is_empty());
}

#[test]
fn ogiltiga_belopp_och_momssatser_spärrar() {
    let mut f = faktura();
    f.rader[0].netto = "tusen".into();
    f.rader[0].momssats = "20".into();
    f.totalbelopp = "-5".into();
    let koder = koder(&f, &förutsättningar());
    for kod in ["netto_ogiltigt", "momssats_ogiltig", "total_ogiltig"] {
        assert!(koder.contains(&kod), "{kod} saknas i {koder:?}");
    }
}

#[test]
fn varje_spärrkod_är_dokumenterad() {
    let dokumenterade: Vec<&str> = REGISTRERINGSSPÄRRAR.iter().map(|(k, _)| *k).collect();
    let mut sedda = Vec::new();
    let mut fö = förutsättningar();
    fö.konton[0].1 = false;
    fö.momsperioder
        .push((date(2026, 12, 1), Momsperiod::EjMomsregistrerad));
    fö.registrerade.push(Registrerad {
        id: "1".into(),
        leverantör: "7".into(),
        fakturanummer: "8812".into(),
        fakturadatum: date(2026, 1, 1),
        total: Belopp::NOLL,
    });
    let fall: Vec<NyLeverantörsfaktura> = vec![
        NyLeverantörsfaktura::default(),
        NyLeverantörsfaktura {
            leverantör: "ny".into(),
            fakturadatum: "x".into(),
            förfallodag: "x".into(),
            totalbelopp: "x".into(),
            rader: vec![
                rad("", "x", "7"),
                rad("6212", "", "25"),
                rad("1", "1", "25"),
            ],
            ..faktura()
        },
        NyLeverantörsfaktura {
            fakturadatum: "2025-01-01".into(),
            totalbelopp: "1".into(),
            rader: vec![NyRad {
                moms: "x".into(),
                ..rad("6110", "1", "25")
            }],
            ..faktura()
        },
        NyLeverantörsfaktura {
            fakturadatum: "2030-01-01".into(),
            förfallodag: "2029-01-01".into(),
            ..faktura()
        },
        NyLeverantörsfaktura {
            leverantör: "7".into(),
            rader: vec![rad("", "1", "25")],
            ..faktura()
        },
        NyLeverantörsfaktura {
            omvänd_skattskyldighet: "okänd".into(),
            ..faktura()
        },
        NyLeverantörsfaktura {
            omvänd_skattskyldighet: "eu_tjanst".into(),
            fakturadatum: "2026-12-01".into(),
            förfallodag: "2026-12-31".into(),
            ..faktura()
        },
    ];
    let mut utan_standardkonto = förutsättningar();
    utan_standardkonto.leverantörer[0].standardkonto = None;
    for f in &fall {
        for fö in [&fö, &utan_standardkonto] {
            sedda.extend(
                f.kontrollera(fö)
                    .err()
                    .unwrap_or_default()
                    .iter()
                    .map(|s| s.kod),
            );
        }
    }
    for kod in &sedda {
        assert!(dokumenterade.contains(kod), "{kod} är inte dokumenterad");
    }
    for kod in &dokumenterade {
        assert!(sedda.contains(kod), "{kod} prövas inte här");
    }
}

// #146 omvänd skattskyldighet

/// En faktura med omvänd skattskyldighet: totalen är nettot, utan moms från leverantören.
fn omvänd(kod: &str) -> NyLeverantörsfaktura {
    NyLeverantörsfaktura {
        omvänd_skattskyldighet: kod.into(),
        totalbelopp: "1 100,00".into(),
        rader: vec![rad("5410", "1000", "25"), rad("6110", "100", "12")],
        ..faktura()
    }
}

#[test]
fn eu_tjänst_bokför_fiktiv_moms_på_2645_mot_2614_och_2624_efter_momssats() {
    assert_eq!(
        kontering(&omvänd("eu_tjanst"), &förutsättningar()),
        [
            (5410, 100_000, 0),
            (6110, 10_000, 0),
            (2645, 26_200, 0),
            (2440, 0, 110_000),
            (2614, 0, 25_000),
            (2624, 0, 1_200),
        ]
    );
}

#[test]
fn eu_vara_och_tjänst_från_land_utanför_eu_bokför_fiktiv_moms_på_2645() {
    for kod in ["eu_vara", "utanfor_eu"] {
        let mut f = omvänd(kod);
        f.rader[1].momssats = "6".into();
        assert_eq!(
            kontering(&f, &förutsättningar()),
            [
                (5410, 100_000, 0),
                (6110, 10_000, 0),
                (2645, 25_600, 0),
                (2440, 0, 110_000),
                (2614, 0, 25_000),
                (2634, 0, 600),
            ],
            "{kod}"
        );
    }
}

#[test]
fn bygg_bokför_fiktiv_ingående_moms_på_2647() {
    let mut f = omvänd("bygg");
    f.totalbelopp = "1 000".into();
    f.rader.truncate(1);
    assert_eq!(
        kontering(&f, &förutsättningar()),
        [
            (5410, 100_000, 0),
            (2647, 25_000, 0),
            (2440, 0, 100_000),
            (2614, 0, 25_000),
        ]
    );
}

#[test]
fn vid_omvänd_skattskyldighet_är_totalen_nettot_och_momsen_är_fiktiv() {
    let mut f = omvänd("eu_tjanst");
    // Den fiktiva momsen kan skrivas över, och räknas inte in i totalen.
    f.rader[0].moms = "249,99".into();
    assert_eq!(kontering(&f, &förutsättningar())[2], (2645, 26_199, 0));
    f.totalbelopp = "1 362,00".into();
    let spärrar = f.kontrollera(&förutsättningar()).unwrap_err();
    assert_eq!(spärrar[0].kod, "summerar_inte");
    assert_eq!(
        spärrar[0].text,
        "Raderna blir 1 100,00 kr, totalen är 1 362,00 kr. Differens 262,00."
    );
}

#[test]
fn noll_procent_ger_ingen_fiktiv_moms() {
    let mut f = omvänd("eu_tjanst");
    f.totalbelopp = "1 000".into();
    f.rader = vec![rad("5410", "1000", "0")];
    assert_eq!(
        kontering(&f, &förutsättningar()),
        [(5410, 100_000, 0), (2440, 0, 100_000)]
    );
}

#[test]
fn leverantörens_förval_föreslås_när_inget_är_valt() {
    let mut fö = förutsättningar();
    fö.leverantörer[0].omvänd_skattskyldighet = OmvändSkattskyldighet::Bygg;
    let mut f = omvänd("");
    assert_eq!(f.omvänd_skattskyldighet(&fö), OmvändSkattskyldighet::Bygg);
    assert_eq!(kontering(&f, &fö)[2], (2647, 26_200, 0));
    // Ett val på fakturan går före förvalet.
    f.omvänd_skattskyldighet = "ingen".into();
    assert_eq!(f.omvänd_skattskyldighet(&fö), OmvändSkattskyldighet::Ingen);
    // En ny leverantör från utkastet föreslår sitt eget förval.
    let mut ny = omvänd("");
    ny.leverantör = "ny".into();
    ny.ny_leverantör = NyLeverantör {
        namn: "Bygg Oy".into(),
        omvänd_skattskyldighet: "eu_tjanst".into(),
        ..NyLeverantör::default()
    };
    assert_eq!(
        ny.omvänd_skattskyldighet(&fö),
        OmvändSkattskyldighet::EuTjänst
    );
}

#[test]
fn ett_företag_utan_momsregistrering_kan_inte_välja_omvänd_skattskyldighet() {
    let mut fö = förutsättningar();
    fö.momsperioder = vec![(date(2026, 1, 1), Momsperiod::EjMomsregistrerad)];
    fö.leverantörer[0].omvänd_skattskyldighet = OmvändSkattskyldighet::EuTjänst;
    // Förvalet föreslås inte: fakturan bokförs med momsen på kostnadskontot.
    let f = faktura();
    assert_eq!(f.omvänd_skattskyldighet(&fö), OmvändSkattskyldighet::Ingen);
    assert_eq!(kontering(&f, &fö), [(5410, 125_000, 0), (2440, 0, 125_000)]);
    // Ett uttryckligt val spärrar.
    let spärrar = omvänd("eu_tjanst").kontrollera(&fö).unwrap_err();
    assert_eq!(spärrar[0].kod, "omvand_ej_momsregistrerad");
    assert_eq!(spärrar[0].fält, Fält::OmvändSkattskyldighet);
}

#[test]
fn ett_inaktivt_konto_för_fiktiv_moms_spärrar_med_aktivera() {
    let mut fö = förutsättningar();
    for (k, aktiv) in &mut fö.konton {
        if *k == nr(2647) || *k == nr(2614) {
            *aktiv = false;
        }
    }
    let mut f = omvänd("bygg");
    f.rader[1].momssats = "25".into();
    f.totalbelopp = "1 100".into();
    let spärrar = f.kontrollera(&fö).unwrap_err();
    let åtgärder: Vec<_> = spärrar
        .iter()
        .map(|s| (s.kod, s.fält, s.åtgärd.clone()))
        .collect();
    assert_eq!(
        åtgärder,
        [
            (
                "konto_inaktivt",
                Fält::Kontering,
                Some(Åtgärd::Aktivera(nr(2647)))
            ),
            (
                "konto_inaktivt",
                Fält::Kontering,
                Some(Åtgärd::Aktivera(nr(2614)))
            ),
        ]
    );
}

#[test]
fn förhandsvisningen_visar_den_fiktiva_momsen() {
    let f = omvänd("eu_tjanst");
    let rader: Vec<_> = f
        .förhandsvisning(&förutsättningar())
        .iter()
        .map(|r| (r.konto.get(), r.debet.öre(), r.kredit.öre()))
        .collect();
    assert_eq!(rader, kontering(&f, &förutsättningar()));
}
