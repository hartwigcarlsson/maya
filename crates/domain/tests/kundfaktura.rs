use jiff::civil::date;
use maya_domain::faktura::Momssats;
use maya_domain::foretag::Momsperiod;
use maya_domain::kontoplan::Kontonummer;
use maya_domain::kund::{self, NyKund, OmvändSkattskyldighet};
use maya_domain::kundfaktura::{
    Fakturaföretag, Fakturakund, Fält, Förutsättningar, Momsläge, NY, NyFakturarad, NyKundfaktura,
    OMVÄND_BETALNINGSSKYLDIGHET, Radfält, Spärr, UTFÄRDANDESPÄRRAR, intäktskonto,
};
use maya_domain::rakenskapsar::Räkenskapsår;
use maya_domain::verifikat::Årsval;

fn nr(n: u16) -> Kontonummer {
    Kontonummer::new(n).unwrap()
}

fn kund() -> Fakturakund {
    Fakturakund {
        id: 7,
        namn: "Kund AB".into(),
        identitet: "556677-8899".into(),
        adress: "Storgatan 1\n111 22 Stockholm".into(),
        epost: "ekonomi@kund.se".into(),
        momsregnr: "SE556677889901".into(),
        betalningsvillkor: 30,
        omvänd_skattskyldighet: OmvändSkattskyldighet::Ingen,
    }
}

fn fakturaföretag() -> Fakturaföretag {
    Fakturaföretag {
        namn: "Exempel AB".into(),
        orgnr: "556036-0793".into(),
        gatuadress: "Exempelgatan 1".into(),
        postnummer: "123 45".into(),
        postort: "Exempelstad".into(),
        säte: "Stockholm".into(),
        momsregnr: "SE556036079301".into(),
        f_skatt: true,
        bankgiro: "5050-1055".into(),
        plusgiro: String::new(),
        iban: String::new(),
        bic: String::new(),
        ocr: false,
    }
}

fn förutsättningar() -> Förutsättningar {
    Förutsättningar {
        kunder: vec![kund()],
        fakturaföretag: fakturaföretag(),
        konton: [
            1510, 2611, 2621, 2631, 3001, 3002, 3003, 3004, 3041, 3231, 3308,
        ]
        .into_iter()
        .map(|n| (nr(n), true))
        .chain([(nr(3011), false), (nr(1930), true)])
        .collect(),
        räkenskapsår: vec![Räkenskapsår::befintligt(
            date(2026, 1, 1),
            date(2026, 12, 31),
        )],
        momsperioder: vec![(date(2026, 1, 1), Momsperiod::Kvartal)],
        nästa_nummer: None,
        startnummer: 1,
    }
}

fn rad(beskrivning: &str, antal: &str, à_pris: &str, sats: &str, konto: &str) -> NyFakturarad {
    NyFakturarad {
        beskrivning: beskrivning.into(),
        antal: antal.into(),
        enhet: "st".into(),
        à_pris: à_pris.into(),
        rabatt: String::new(),
        momssats: sats.into(),
        konto: konto.into(),
    }
}

fn faktura() -> NyKundfaktura {
    NyKundfaktura {
        kund: "7".into(),
        fakturadatum: "2026-03-10".into(),
        leveransdatum: String::new(),
        förfallodag: String::new(),
        er_referens: "Bo Ek".into(),
        startnummer: String::new(),
        omvänd_skattskyldighet: String::new(),
        rader: vec![
            rad("Konsulttimmar", "10", "950", "25", "3001"),
            rad("Bok", "2", "200", "6", "3003"),
            rad("Utlägg", "1", "50", "0", "3004"),
        ],
        ny_kund: NyKund::default(),
        krediterar: None,
    }
}

fn spärrar(f: &NyKundfaktura, ff: &Förutsättningar) -> Vec<(Fält, &'static str)> {
    f.kontrollera(ff)
        .unwrap_err()
        .iter()
        .map(|s: &Spärr| (s.fält, s.kod))
        .collect()
}

#[test]
fn en_ifylld_faktura_kan_utfärdas_med_netto_moms_per_sats_och_total() {
    let u = faktura().kontrollera(&förutsättningar()).unwrap();

    assert_eq!(u.kund.namn, "Kund AB");
    assert_eq!(u.fakturadatum, date(2026, 3, 10));
    // Förfallodagen är fakturadatumet plus kundens 30 dagar, leveransdatumet fakturadatumet.
    assert_eq!(u.förfallodag, date(2026, 4, 9));
    assert_eq!(u.leveransdatum, "2026-03-10");
    let netto: Vec<i64> = u.rader.iter().map(|r| r.netto).collect();
    assert_eq!(netto, [950_000, 40_000, 5_000]);
    let moms: Vec<(u8, i64, i64)> = u
        .momsrader
        .iter()
        .map(|m| (m.sats.procent(), m.underlag, m.moms))
        .collect();
    assert_eq!(
        moms,
        [(25, 950_000, 237_500), (6, 40_000, 2_400), (0, 5_000, 0)]
    );
    assert_eq!(u.netto, 995_000);
    assert_eq!(u.moms, 239_900);
    assert_eq!(u.total, 1_234_900);
    assert_eq!(
        u.räkenskapsår,
        Årsval::Befintligt(förutsättningar().räkenskapsår[0])
    );
}

#[test]
fn den_första_fakturan_får_startnumret_och_numret_är_referensen_utan_ocr() {
    let u = faktura().kontrollera(&förutsättningar()).unwrap();
    assert_eq!(u.nummer, 1);
    assert_eq!(u.referens, "1");

    let mut startnummer = faktura();
    startnummer.startnummer = "1007".into();
    let u = startnummer.kontrollera(&förutsättningar()).unwrap();
    assert_eq!(u.nummer, 1007);
    assert_eq!(u.referens, "1007");
}

#[test]
fn efter_det_första_utfärdandet_gäller_seriens_nästa_nummer_och_startnumret_bortses_från() {
    let ff = Förutsättningar {
        nästa_nummer: Some(1042),
        ..förutsättningar()
    };
    let mut f = faktura();
    f.startnummer = "5".into();
    assert_eq!(f.kontrollera(&ff).unwrap().nummer, 1042);
}

#[test]
fn med_ocr_på_är_referensen_numret_med_längdsiffra_och_luhn() {
    let mut ff = förutsättningar();
    ff.fakturaföretag.ocr = true;
    ff.nästa_nummer = Some(1007);
    let u = faktura().kontrollera(&ff).unwrap();
    assert_eq!(u.referens, "100768");
}

#[test]
fn konteringen_är_1510_totalen_mot_intäkt_per_konto_och_utgående_moms_per_sats() {
    let mut f = faktura();
    f.rader.push(rad("Fler timmar", "2", "950", "25", "3001"));
    let u = f.kontrollera(&förutsättningar()).unwrap();

    let rader: Vec<(u16, i64, i64)> = u
        .kontering()
        .iter()
        .map(|r| (r.konto.get(), r.debet.öre(), r.kredit.öre()))
        .collect();
    assert_eq!(
        rader,
        [
            (1510, 1_472_400, 0),
            (3001, 0, 1_140_000),
            (3003, 0, 40_000),
            (3004, 0, 5_000),
            // 0 % ger ingen momsrad.
            (2611, 0, 285_000),
            (2631, 0, 2_400),
        ]
    );
    assert_eq!(u.beskrivning(), "Kundfaktura 1, Kund AB");
}

#[test]
fn intäktskontot_följer_momssatsen() {
    let konto = |s| intäktskonto(s).get();
    assert_eq!(konto(Momssats::Tjugofem), 3001);
    assert_eq!(konto(Momssats::Tolv), 3002);
    assert_eq!(konto(Momssats::Sex), 3003);
    assert_eq!(konto(Momssats::Noll), 3004);
}

#[test]
fn rabatten_dras_från_radens_netto() {
    let mut f = faktura();
    f.rader = vec![NyFakturarad {
        rabatt: "10".into(),
        ..rad("Timmar", "10", "100", "25", "3001")
    }];
    let u = f.kontrollera(&förutsättningar()).unwrap();
    assert_eq!(u.rader[0].netto, 90_000);
    assert_eq!(u.total, 112_500);
}

#[test]
fn en_angiven_förfallodag_och_ett_leveransdatum_gäller_framför_standardvärdena() {
    let mut f = faktura();
    f.förfallodag = "2026-03-20".into();
    f.leveransdatum = "februari 2026".into();
    let u = f.kontrollera(&förutsättningar()).unwrap();
    assert_eq!(u.förfallodag, date(2026, 3, 20));
    assert_eq!(u.leveransdatum, "februari 2026");
}

#[test]
fn helt_tomma_rader_räknas_inte() {
    let mut f = faktura();
    f.rader.insert(1, NyFakturarad::default());
    let u = f.kontrollera(&förutsättningar()).unwrap();
    assert_eq!(u.rader.len(), 3);
}

#[test]
fn kund_som_saknas_eller_inte_finns_i_registret_spärrar_vid_kunden() {
    let mut f = faktura();
    f.kund = String::new();
    assert_eq!(
        spärrar(&f, &förutsättningar()),
        [(Fält::Kund, "kund_saknas")]
    );
    f.kund = "99".into();
    assert_eq!(
        spärrar(&f, &förutsättningar()),
        [(Fält::Kund, "kund_saknas")]
    );
}

#[test]
fn kundens_adress_krävs() {
    let mut ff = förutsättningar();
    ff.kunder[0].adress = String::new();
    assert_eq!(
        spärrar(&faktura(), &ff),
        [(Fält::Kund, "kund_adress_saknas")]
    );
}

#[test]
fn fakturadatumet_krävs_och_ska_ligga_i_ett_räkenskapsår() {
    let mut f = faktura();
    f.fakturadatum = String::new();
    assert_eq!(
        spärrar(&f, &förutsättningar()),
        [(Fält::Fakturadatum, "fakturadatum_saknas")]
    );
    f.fakturadatum = "10 mars".into();
    assert_eq!(
        spärrar(&f, &förutsättningar()),
        [(Fält::Fakturadatum, "fakturadatum_ogiltigt")]
    );
    f.fakturadatum = "2025-12-31".into();
    f.förfallodag = "2026-01-30".into();
    assert_eq!(
        spärrar(&f, &förutsättningar()),
        [(Fält::Fakturadatum, "fore_forsta_aret")]
    );
    f.fakturadatum = "2028-01-10".into();
    f.förfallodag = String::new();
    assert_eq!(
        spärrar(&f, &förutsättningar()),
        [(Fält::Fakturadatum, "saknat_rakenskapsar")]
    );
}

#[test]
fn ett_fakturadatum_i_året_efter_det_senaste_skapar_året() {
    let mut f = faktura();
    f.fakturadatum = "2027-01-10".into();
    let u = f.kontrollera(&förutsättningar()).unwrap();
    assert!(matches!(u.räkenskapsår, Årsval::Nytt(_)));
}

#[test]
fn förfallodagen_är_ett_datum_som_inte_är_före_fakturadatumet() {
    let mut f = faktura();
    f.förfallodag = "snart".into();
    assert_eq!(
        spärrar(&f, &förutsättningar()),
        [(Fält::Förfallodag, "forfallodag_ogiltig")]
    );
    f.förfallodag = "2026-03-09".into();
    assert_eq!(
        spärrar(&f, &förutsättningar()),
        [(Fält::Förfallodag, "forfallodag_fore_fakturadatum")]
    );
}

#[test]
fn en_faktura_utan_rader_spärras() {
    let mut f = faktura();
    f.rader = vec![NyFakturarad::default()];
    assert_eq!(
        spärrar(&f, &förutsättningar()),
        [(Fält::Rader, "rader_saknas")]
    );
}

#[test]
fn en_ofullständig_rad_spärras_vid_sitt_fält() {
    let mut f = faktura();
    f.rader = vec![
        rad("", "1", "100", "25", "3001"),
        rad("Timmar", "", "100", "25", "3001"),
        rad("Timmar", "1", "", "25", "3001"),
        rad("Timmar", "1", "100", "25", ""),
    ];
    assert_eq!(
        spärrar(&f, &förutsättningar()),
        [
            (Fält::Rad(0, Radfält::Beskrivning), "rad_beskrivning_saknas"),
            (Fält::Rad(1, Radfält::Antal), "rad_antal_saknas"),
            (Fält::Rad(2, Radfält::Pris), "rad_pris_saknas"),
            (Fält::Rad(3, Radfält::Konto), "rad_konto_saknas"),
        ]
    );
}

#[test]
fn felaktiga_tal_på_en_rad_spärras() {
    let mut f = faktura();
    f.rader = vec![
        rad("Timmar", "0", "100", "25", "3001"),
        rad("Timmar", "1", "-5", "25", "3001"),
        NyFakturarad {
            rabatt: "120".into(),
            ..rad("Timmar", "1", "100", "25", "3001")
        },
        rad("Timmar", "1", "100", "8", "3001"),
    ];
    assert_eq!(
        spärrar(&f, &förutsättningar()),
        [
            (Fält::Rad(0, Radfält::Antal), "rad_antal_ogiltigt"),
            (Fält::Rad(1, Radfält::Pris), "rad_pris_ogiltigt"),
            (Fält::Rad(2, Radfält::Rabatt), "rad_rabatt_ogiltig"),
            (Fält::Rad(3, Radfält::Momssats), "rad_momssats_ogiltig"),
        ]
    );
}

#[test]
fn radens_konto_är_ett_aktivt_intäktskonto_i_klass_3() {
    let mut f = faktura();
    f.rader = vec![
        rad("Timmar", "1", "100", "25", "3011"),
        rad("Timmar", "1", "100", "25", "3999"),
        rad("Timmar", "1", "100", "25", "1930"),
        rad("Timmar", "1", "100", "25", "3041"),
    ];
    let fel = f.kontrollera(&förutsättningar()).unwrap_err();
    let koder: Vec<_> = fel.iter().map(|s| (s.fält, s.kod)).collect();
    assert_eq!(
        koder,
        [
            (Fält::Rad(0, Radfält::Konto), "konto_inaktivt"),
            (Fält::Rad(1, Radfält::Konto), "konto_okant"),
            (Fält::Rad(2, Radfält::Konto), "konto_inte_intakt"),
        ]
    );
    assert_eq!(
        fel[0].text,
        "Konto 3011 är inaktivt. Aktivera det för att bokföra på det."
    );
}

#[test]
fn en_faktura_vars_summa_är_noll_spärras() {
    let mut f = faktura();
    f.rader = vec![NyFakturarad {
        rabatt: "100".into(),
        ..rad("Gratis", "1", "100", "25", "3001")
    }];
    assert_eq!(
        spärrar(&f, &förutsättningar()),
        [(Fält::Summa, "summa_inte_positiv")]
    );
}

#[test]
fn företagets_fakturauppgifter_krävs() {
    let mut ff = förutsättningar();
    ff.fakturaföretag.gatuadress = String::new();
    ff.fakturaföretag.säte = String::new();
    ff.fakturaföretag.f_skatt = false;
    ff.fakturaföretag.momsregnr = String::new();
    ff.fakturaföretag.bankgiro = String::new();
    assert_eq!(
        spärrar(&faktura(), &ff),
        [
            (Fält::Företag, "foretaget_adress_saknas"),
            (Fält::Företag, "foretaget_sate_saknas"),
            (Fält::Företag, "foretaget_f_skatt_saknas"),
            (Fält::Företag, "foretaget_momsregnr_saknas"),
            (Fält::Företag, "betalsatt_saknas"),
        ]
    );
}

#[test]
fn ett_plusgiro_eller_iban_med_bic_räcker_som_betalsätt() {
    let mut ff = förutsättningar();
    ff.fakturaföretag.bankgiro = String::new();
    ff.fakturaföretag.plusgiro = "4711-7".into();
    assert!(faktura().kontrollera(&ff).is_ok());
    ff.fakturaföretag.plusgiro = String::new();
    ff.fakturaföretag.iban = "SE45 5000 0000 0583 9825 7466".into();
    assert_eq!(
        spärrar(&faktura(), &ff),
        [(Fält::Företag, "betalsatt_saknas")]
    );
    ff.fakturaföretag.bic = "ESSESESS".into();
    assert!(faktura().kontrollera(&ff).is_ok());
}

#[test]
fn momsregistreringsnumret_krävs_bara_när_företaget_är_momsregistrerat_på_fakturadatumet() {
    let mut ff = förutsättningar();
    ff.fakturaföretag.momsregnr = String::new();
    ff.momsperioder = vec![
        (date(2026, 1, 1), Momsperiod::EjMomsregistrerad),
        (date(2026, 4, 1), Momsperiod::Kvartal),
    ];
    let mut f = faktura();
    f.rader = vec![rad("Timmar", "1", "100", "0", "3004")];
    assert!(f.kontrollera(&ff).is_ok());
    f.fakturadatum = "2026-04-01".into();
    assert_eq!(
        spärrar(&f, &ff),
        [(Fält::Företag, "foretaget_momsregnr_saknas")]
    );
}

#[test]
fn startnumret_är_ett_positivt_heltal() {
    let mut f = faktura();
    f.startnummer = "0".into();
    assert_eq!(
        spärrar(&f, &förutsättningar()),
        [(Fält::Startnummer, "startnummer_ogiltigt")]
    );
    f.startnummer = "A12".into();
    assert_eq!(
        spärrar(&f, &förutsättningar()),
        [(Fält::Startnummer, "startnummer_ogiltigt")]
    );
}

#[test]
fn varje_spärr_vid_utfärdande_är_dokumenterad_och_ingen_annan() {
    let mut tom_säljare = förutsättningar();
    tom_säljare.fakturaföretag = Fakturaföretag::default();
    tom_säljare.kunder[0].adress = String::new();
    let mut fall: Vec<(NyKundfaktura, Förutsättningar)> = vec![
        (NyKundfaktura::default(), förutsättningar()),
        (faktura(), tom_säljare),
    ];
    let mut f = faktura();
    f.kund = "99".into();
    f.fakturadatum = "10 mars".into();
    f.förfallodag = "snart".into();
    f.startnummer = "0".into();
    f.rader = vec![
        rad("", "", "", "8", ""),
        rad("x", "0", "-1", "25", "3011"),
        NyFakturarad {
            rabatt: "200".into(),
            ..rad("x", "1", "1", "25", "3999")
        },
        rad("x", "1", "1", "25", "1930"),
    ];
    fall.push((f, förutsättningar()));
    for (datum, förfallo) in [
        ("2025-12-31", ""),
        ("2028-01-01", ""),
        ("2026-03-10", "2026-03-01"),
    ] {
        let mut f = faktura();
        f.fakturadatum = datum.into();
        f.förfallodag = förfallo.into();
        fall.push((f, förutsättningar()));
    }
    let mut noll = faktura();
    noll.rader = vec![NyFakturarad {
        rabatt: "100".into(),
        ..rad("x", "1", "1", "25", "3001")
    }];
    fall.push((noll, förutsättningar()));
    let mut ny = faktura();
    ny.kund = NY.into();
    fall.push((ny, förutsättningar()));
    // #136: omvänd skattskyldighet utan köparens momsreg-nr, och ett okänt val.
    let mut omvänd = faktura();
    omvänd.omvänd_skattskyldighet = "bygg".into();
    let mut utan_momsregnr = förutsättningar();
    utan_momsregnr.kunder[0].momsregnr = String::new();
    fall.push((omvänd, utan_momsregnr));
    let mut okänd = faktura();
    okänd.omvänd_skattskyldighet = "export".into();
    fall.push((okänd, förutsättningar()));
    // #140: en kreditfaktura vars summa inte är negativ, och en som krediterar för mycket.
    let hänvisning = maya_domain::kreditfaktura::Hänvisning {
        faktura: 1,
        nummer: 1,
        fakturadatum: "2026-03-01".into(),
        momsläge: Momsläge::Moms,
        kund: kund(),
        total: 100,
        kvar: 100,
    };
    let mut kredit = faktura();
    kredit.krediterar = Some(hänvisning);
    kredit.rader = vec![rad("x", "-1", "0", "25", "3001")];
    fall.push((kredit.clone(), förutsättningar()));
    kredit.rader = vec![rad("x", "-1", "10", "25", "3001")];
    fall.push((kredit, förutsättningar()));

    let mut koder: Vec<&str> = fall
        .iter()
        .flat_map(|(f, ff)| f.kontrollera(ff).err().unwrap_or_default())
        .map(|s| s.kod)
        .collect();
    koder.sort();
    koder.dedup();
    let mut dokumenterade: Vec<&str> = UTFÄRDANDESPÄRRAR.iter().map(|(k, _)| *k).collect();
    dokumenterade.sort();
    assert_eq!(koder, dokumenterade);
}

#[test]
fn ett_utkast_sparas_med_det_som_går_att_tolka_och_blir_samma_formulär_igen() {
    let mut f = faktura();
    f.kund = String::new();
    f.förfallodag = String::new();
    f.rader = vec![rad("Timmar", "1,5", "", "12", "")];
    let u = f.som_utkast().unwrap();
    assert_eq!(u.kund, None);
    assert_eq!(u.fakturadatum, Some(date(2026, 3, 10)));
    assert_eq!(u.rader.len(), 1);
    assert_eq!(u.rader[0].antal.unwrap().hundradelar(), 150);
    assert_eq!(u.rader[0].à_pris, None);
    assert_eq!(u.rader[0].momssats, Momssats::Tolv);

    let igen = u.som_formulär();
    assert_eq!(igen.rader[0].antal, "1,5");
    assert_eq!(igen.rader[0].momssats, "12");
    assert_eq!(igen.fakturadatum, "2026-03-10");
    assert_eq!(igen.som_utkast().unwrap(), u);
}

#[test]
fn ett_utkast_med_något_som_inte_går_att_lagra_spärras() {
    let mut f = faktura();
    f.fakturadatum = "imorgon".into();
    f.rader = vec![rad("Timmar", "många", "100", "25", "3001")];
    let koder: Vec<_> = f
        .som_utkast()
        .unwrap_err()
        .iter()
        .map(|s| (s.fält, s.kod))
        .collect();
    assert_eq!(
        koder,
        [
            (Fält::Fakturadatum, "fakturadatum_ogiltigt"),
            (Fält::Rad(0, Radfält::Antal), "rad_antal_ogiltigt"),
        ]
    );
}

#[test]
fn beräkningen_visar_summorna_också_när_något_saknas() {
    let mut f = faktura();
    f.kund = String::new();
    f.rader.push(rad("Halv rad", "2", "", "25", "3001"));
    let b = f.beräkna();
    assert_eq!(b.rader, [Some(950_000), Some(40_000), Some(5_000), None]);
    assert_eq!(b.total, 1_234_900);
}

// #137 Ny kund från fakturautkastet

fn ny_kund() -> NyKund {
    NyKund {
        namn: "Nya Kunden AB".into(),
        adress: "Lillgatan 2\n222 33 Malmö".into(),
        epost: "faktura@nya.se".into(),
        betalningsvillkor: "10".into(),
        ..NyKund::default()
    }
}

fn med_ny_kund(k: NyKund) -> NyKundfaktura {
    NyKundfaktura {
        kund: NY.into(),
        ny_kund: k,
        krediterar: None,
        ..faktura()
    }
}

#[test]
fn en_ny_kund_från_utkastet_utfärdas_med_sina_egna_uppgifter() {
    let u = med_ny_kund(ny_kund())
        .kontrollera(&förutsättningar())
        .unwrap();

    assert_eq!(u.kund.namn, "Nya Kunden AB");
    assert_eq!(u.kund.adress, "Lillgatan 2\n222 33 Malmö");
    assert_eq!(u.kund.epost, "faktura@nya.se");
    // Förfallodagen följer den nya kundens 10 dagar.
    assert_eq!(u.förfallodag, date(2026, 3, 20));
    assert_eq!(u.ny_kund.map(|k| k.namn).as_deref(), Some("Nya Kunden AB"));
    // En kund ur registret är ingen ny kund.
    assert_eq!(
        faktura().kontrollera(&förutsättningar()).unwrap().ny_kund,
        None
    );
}

#[test]
fn en_ny_kund_som_inte_klarar_kundregistrets_regler_spärras_vid_sitt_fält() {
    let mut k = ny_kund();
    k.epost = "inte en adress".into();
    assert_eq!(
        spärrar(&med_ny_kund(k), &förutsättningar()),
        [(Fält::NyKund(kund::Fält::Epost), "ny_kund")]
    );
    assert_eq!(
        spärrar(&med_ny_kund(NyKund::default()), &förutsättningar()),
        [(Fält::NyKund(kund::Fält::Namn), "ny_kund")]
    );
    let mut utan_adress = ny_kund();
    utan_adress.adress = String::new();
    assert_eq!(
        spärrar(&med_ny_kund(utan_adress), &förutsättningar()),
        [(Fält::NyKund(kund::Fält::Adress), "kund_adress_saknas")]
    );
}

#[test]
fn ett_utkast_med_en_ny_kund_sparar_kunden_och_blir_samma_formulär_igen() {
    let u = med_ny_kund(ny_kund()).som_utkast().unwrap();
    assert_eq!(u.kund, None);
    assert_eq!(
        u.ny_kund.as_ref().map(|k| k.namn.as_str()),
        Some("Nya Kunden AB")
    );
    let igen = u.som_formulär();
    assert_eq!(igen.kund, NY);
    assert_eq!(igen.ny_kund.namn, "Nya Kunden AB");
    assert_eq!(igen.ny_kund.betalningsvillkor, "10");
    assert_eq!(igen.som_utkast().unwrap(), u);

    // Utan namn finns ingen ny kund att spara, och utkastet saknar kund.
    let tom = med_ny_kund(NyKund::default()).som_utkast().unwrap();
    assert_eq!((tom.kund, tom.ny_kund), (None, None));

    let mut fel = ny_kund();
    fel.identitet = "123".into();
    let koder: Vec<_> = med_ny_kund(fel)
        .som_utkast()
        .unwrap_err()
        .iter()
        .map(|s| (s.fält, s.kod))
        .collect();
    assert_eq!(koder, [(Fält::NyKund(kund::Fält::Identitet), "ny_kund")]);
}

#[test]
fn kunden_är_den_valda_i_registret_eller_den_nya() {
    let f = förutsättningar();
    assert_eq!(faktura().kunden(&f).map(|k| k.namn), Some("Kund AB".into()));
    assert_eq!(
        med_ny_kund(ny_kund())
            .kunden(&f)
            .map(|k| k.betalningsvillkor),
        Some(10)
    );
    assert_eq!(med_ny_kund(NyKund::default()).kunden(&f), None);
}

// #136 Omvänd skattskyldighet och företag utan momsregistrering

fn omvänd(kod: &str) -> NyKundfaktura {
    NyKundfaktura {
        omvänd_skattskyldighet: kod.into(),
        rader: vec![
            rad("Elinstallation", "1", "62000", "0", "3231"),
            rad("Material", "2", "500", "0", "3231"),
        ],
        ..faktura()
    }
}

fn ej_momsregistrerat() -> Förutsättningar {
    let mut ff = förutsättningar();
    ff.momsperioder = vec![(date(2026, 1, 1), Momsperiod::EjMomsregistrerad)];
    ff.fakturaföretag.momsregnr = String::new();
    ff
}

#[test]
fn intäktskontot_vid_omvänd_skattskyldighet_är_3231_för_bygg_och_3308_för_eu_tjänst() {
    for sats in Momssats::ALLA {
        assert_eq!(Momsläge::Bygg.intäktskonto(sats), nr(3231));
        assert_eq!(Momsläge::EuTjänst.intäktskonto(sats), nr(3308));
        assert_eq!(Momsläge::UtanMoms.intäktskonto(sats), nr(3004));
        assert_eq!(Momsläge::Moms.intäktskonto(sats), intäktskonto(sats));
    }
}

#[test]
fn omvänd_skattskyldighet_ger_noll_procent_på_varje_rad_och_ingen_momsrad_i_konteringen() {
    let mut f = omvänd("bygg");
    // En sats som står kvar i formuläret gäller inte: raderna får 0 %.
    f.rader[1].momssats = "25".into();
    let u = f.kontrollera(&förutsättningar()).unwrap();
    assert_eq!(u.momsläge, Momsläge::Bygg);
    assert!(u.rader.iter().all(|r| r.momssats == Momssats::Noll));
    assert_eq!(u.netto, 6_300_000);
    assert_eq!(u.moms, 0);
    assert_eq!(u.total, 6_300_000);
    let kontering: Vec<(u16, i64, i64)> = u
        .kontering()
        .iter()
        .map(|r| (r.konto.get(), r.debet.öre(), r.kredit.öre()))
        .collect();
    assert_eq!(kontering, [(1510, 6_300_000, 0), (3231, 0, 6_300_000)]);

    let mut eu = omvänd("eu_tjanst");
    for r in &mut eu.rader {
        r.konto = "3308".into();
    }
    let u = eu.kontrollera(&förutsättningar()).unwrap();
    assert_eq!(u.momsläge, Momsläge::EuTjänst);
    let konton: Vec<u16> = u.kontering().iter().map(|r| r.konto.get()).collect();
    assert_eq!(konton, [1510, 3308]);
}

#[test]
fn fakturan_med_omvänd_skattskyldighet_får_texten_om_omvänd_betalningsskyldighet() {
    for läge in [Momsläge::Bygg, Momsläge::EuTjänst] {
        assert!(
            läge
                .fakturatext()
                .unwrap()
                .starts_with(OMVÄND_BETALNINGSSKYLDIGHET)
        );
    }
    assert_eq!(OMVÄND_BETALNINGSSKYLDIGHET, "Omvänd betalningsskyldighet");
    assert_eq!(Momsläge::Moms.fakturatext(), None);
    assert_eq!(Momsläge::UtanMoms.fakturatext(), None);
}

#[test]
fn omvänd_skattskyldighet_spärras_vid_kunden_när_köparens_momsregistreringsnummer_saknas() {
    let mut ff = förutsättningar();
    ff.kunder[0].momsregnr = "  ".into();
    assert_eq!(
        spärrar(&omvänd("eu_tjanst"), &ff),
        [(Fält::Kund, "kund_momsregnr_saknas")]
    );
    let text = &omvänd("bygg").kontrollera(&ff).unwrap_err()[0].text;
    assert_eq!(
        text,
        "Omvänd skattskyldighet kräver köparens momsreg-nr. Kund AB saknar det. Fyll i det i \
         kundregistret."
    );
    // Utan omvänd skattskyldighet krävs inte köparens momsreg-nr.
    assert!(faktura().kontrollera(&ff).is_ok());
}

#[test]
fn ett_okänt_val_av_omvänd_skattskyldighet_spärras_vid_fältet() {
    let f = omvänd("export");
    assert_eq!(
        spärrar(&f, &förutsättningar()),
        [(Fält::OmvändSkattskyldighet, "omvand_ogiltig")]
    );
    let koder: Vec<_> = f
        .som_utkast()
        .unwrap_err()
        .iter()
        .map(|s| (s.fält, s.kod))
        .collect();
    assert_eq!(koder, [(Fält::OmvändSkattskyldighet, "omvand_ogiltig")]);
}

#[test]
fn ett_företag_utan_momsregistrering_får_en_faktura_utan_moms_och_utan_momsregistreringsnummer() {
    let ff = ej_momsregistrerat();
    // Momssatserna i formuläret gäller inte, och omvänd skattskyldighet kan inte väljas.
    let mut f = faktura();
    f.omvänd_skattskyldighet = "bygg".into();
    assert_eq!(f.momsläge(&ff), Momsläge::UtanMoms);
    let u = f.kontrollera(&ff).unwrap();
    assert_eq!(u.momsläge, Momsläge::UtanMoms);
    assert!(u.rader.iter().all(|r| r.momssats == Momssats::Noll));
    assert_eq!(u.moms, 0);
    assert_eq!(u.total, 995_000);
    assert!(
        u.kontering()
            .iter()
            .all(|r| !(2600..2700).contains(&r.konto.get()))
    );
    // Företagets momsreg-nr står inte på fakturan, också om det finns kvar i inställningarna.
    let mut fakturaföretag = fakturaföretag();
    assert_eq!(u.företaget_på_fakturan(&fakturaföretag).momsregnr, "");
    fakturaföretag.momsregnr = String::new();
    assert_eq!(u.företaget_på_fakturan(&fakturaföretag), fakturaföretag);
    assert_eq!(f.beräkna_med(f.momsläge(&ff)).moms, 0);
}

#[test]
fn momsläget_följer_momsperiodens_historik_på_fakturadatumet() {
    let mut ff = förutsättningar();
    ff.momsperioder = vec![
        (date(2026, 1, 1), Momsperiod::EjMomsregistrerad),
        (date(2026, 4, 1), Momsperiod::Kvartal),
    ];
    let mut f = omvänd("bygg");
    assert_eq!(f.momsläge(&ff), Momsläge::UtanMoms);
    f.fakturadatum = "2026-04-01".into();
    assert_eq!(f.momsläge(&ff), Momsläge::Bygg);
    f.omvänd_skattskyldighet = "ingen".into();
    assert_eq!(f.momsläge(&ff), Momsläge::Moms);
    // Tom text är ingen omvänd skattskyldighet.
    f.omvänd_skattskyldighet = String::new();
    assert_eq!(f.momsläge(&ff), Momsläge::Moms);
}

#[test]
fn kundens_förvalda_omvända_skattskyldighet_fylls_i_när_kunden_väljs() {
    let mut ff = förutsättningar();
    ff.kunder.push(Fakturakund {
        id: 8,
        namn: "Bergström Bygg AB".into(),
        omvänd_skattskyldighet: OmvändSkattskyldighet::Bygg,
        ..kund()
    });
    let mut f = NyKundfaktura {
        kund: String::new(),
        rader: vec![
            rad("Timmar", "1", "100", "25", "3001"),
            NyFakturarad::default(),
        ],
        ..faktura()
    };
    f.välj_kund("8", &ff);
    assert_eq!(f.kund, "8");
    assert_eq!(f.omvänd_skattskyldighet, "bygg");
    // Raderna följer: 0 % och 3231 där kontot var satsens konto.
    assert!(
        f.rader
            .iter()
            .all(|r| r.momssats == "0" && r.konto == "3231")
    );

    f.välj_kund("7", &ff);
    assert_eq!(f.omvänd_skattskyldighet, "ingen");
    assert!(
        f.rader
            .iter()
            .all(|r| r.momssats == "25" && r.konto == "3001")
    );
}

#[test]
fn ett_eget_konto_på_en_rad_står_kvar_när_momsläget_byts() {
    let mut f = faktura();
    f.rader = vec![rad("Hyra", "1", "100", "25", "3041")];
    f.byt_momsläge(Momsläge::Moms, Momsläge::EuTjänst);
    assert_eq!(f.rader[0].konto, "3041");
    assert_eq!(f.rader[0].momssats, "0");
    assert_eq!(NyFakturarad::för(Momsläge::EuTjänst).konto, "3308");
    assert_eq!(NyFakturarad::för(Momsläge::Moms), NyFakturarad::default());
}

#[test]
fn ett_utkast_med_omvänd_skattskyldighet_sparas_med_noll_procent_och_blir_samma_formulär_igen() {
    let mut f = omvänd("bygg");
    f.rader[0].momssats = "25".into();
    let u = f.som_utkast().unwrap();
    assert_eq!(u.omvänd_skattskyldighet, OmvändSkattskyldighet::Bygg);
    assert!(u.rader.iter().all(|r| r.momssats == Momssats::Noll));
    let igen = u.som_formulär();
    assert_eq!(igen.omvänd_skattskyldighet, "bygg");
    assert_eq!(igen.som_utkast().unwrap(), u);
}
