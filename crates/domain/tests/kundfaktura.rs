use jiff::civil::date;
use maya_domain::faktura::Momssats;
use maya_domain::foretag::Momsperiod;
use maya_domain::kontoplan::Kontonummer;
use maya_domain::kundfaktura::{
    Fakturakund, Fält, Förutsättningar, NyFakturarad, NyKundfaktura, Radfält, Spärr, Säljare,
    UTFÄRDANDESPÄRRAR, intäktskonto,
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
    }
}

fn säljare() -> Säljare {
    Säljare {
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
        säljare: säljare(),
        konton: [1510, 2611, 2621, 2631, 3001, 3002, 3003, 3004, 3041]
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
        rader: vec![
            rad("Konsulttimmar", "10", "950", "25", "3001"),
            rad("Bok", "2", "200", "6", "3003"),
            rad("Utlägg", "1", "50", "0", "3004"),
        ],
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
    ff.säljare.ocr = true;
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
    ff.säljare.gatuadress = String::new();
    ff.säljare.säte = String::new();
    ff.säljare.f_skatt = false;
    ff.säljare.momsregnr = String::new();
    ff.säljare.bankgiro = String::new();
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
    ff.säljare.bankgiro = String::new();
    ff.säljare.plusgiro = "4711-7".into();
    assert!(faktura().kontrollera(&ff).is_ok());
    ff.säljare.plusgiro = String::new();
    ff.säljare.iban = "SE45 5000 0000 0583 9825 7466".into();
    assert_eq!(
        spärrar(&faktura(), &ff),
        [(Fält::Företag, "betalsatt_saknas")]
    );
    ff.säljare.bic = "ESSESESS".into();
    assert!(faktura().kontrollera(&ff).is_ok());
}

#[test]
fn momsregistreringsnumret_krävs_bara_när_företaget_är_momsregistrerat_på_fakturadatumet() {
    let mut ff = förutsättningar();
    ff.säljare.momsregnr = String::new();
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
    tom_säljare.säljare = Säljare::default();
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
