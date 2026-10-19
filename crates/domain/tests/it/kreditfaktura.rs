//! Kreditfakturor till kund: utkastet ur originalet, spärrarna vid utfärdande och konteringen
//! som speglar originalet.

use jiff::civil::date;
use maya_domain::belopp::Belopp;
use maya_domain::foretag::Momsperiod;
use maya_domain::kontoplan::Kontonummer;
use maya_domain::kreditfaktura::{Hänvisning, KREDITERINGSSPÄRRAR, NyKreditering};
use maya_domain::kund::{NyKund, OmvändSkattskyldighet};
use maya_domain::kundfaktura::{
    Fakturaföretag, Fakturakund, Fält, Förutsättningar, Momsläge, NyFakturarad, NyKundfaktura,
    Radfält, Utfärdbar,
};
use maya_domain::rakenskapsar::Räkenskapsår;
use maya_domain::verifikat::Rad;

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

fn förutsättningar() -> Förutsättningar {
    Förutsättningar {
        kunder: vec![kund()],
        fakturaföretag: Fakturaföretag {
            namn: "Exempel AB".into(),
            orgnr: "556036-0793".into(),
            gatuadress: "Exempelgatan 1".into(),
            postnummer: "123 45".into(),
            postort: "Exempelstad".into(),
            säte: "Stockholm".into(),
            momsregnr: "SE556036079301".into(),
            f_skatt: true,
            bankgiro: "5050-1055".into(),
            ..Fakturaföretag::default()
        },
        konton: [1510, 2611, 2631, 3001, 3003, 3004, 3041, 3231]
            .into_iter()
            .map(|n| (nr(n), true))
            .collect(),
        räkenskapsår: vec![Räkenskapsår::befintligt(
            date(2026, 1, 1),
            date(2026, 12, 31),
        )],
        momsperioder: vec![(date(2026, 1, 1), Momsperiod::Kvartal)],
        nästa_nummer: Some(1008),
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

/// Originalet: utfärdat som faktura 1007.
fn original() -> Utfärdbar {
    let mut ff = förutsättningar();
    ff.nästa_nummer = Some(1007);
    NyKundfaktura {
        kund: "7".into(),
        fakturadatum: "2026-03-10".into(),
        er_referens: "Bo Ek".into(),
        rader: vec![
            rad("Konsulttimmar", "10", "950", "25", "3041"),
            NyFakturarad {
                rabatt: "10".into(),
                ..rad("Bok", "3", "200", "6", "3003")
            },
            rad("Utlägg", "1", "50", "0", "3004"),
        ],
        ny_kund: NyKund::default(),
        ..NyKundfaktura::default()
    }
    .kontrollera(&ff)
    .unwrap()
}

fn hänvisning(o: &Utfärdbar) -> Hänvisning {
    Hänvisning {
        faktura: 41,
        nummer: o.nummer,
        fakturadatum: o.fakturadatum.to_string(),
        momsläge: o.momsläge,
        kund: o.kund.clone(),
        total: o.total,
        kvar: o.total,
    }
}

fn kreditera(o: &Utfärdbar, h: &Hänvisning, kreditering: NyKreditering) -> NyKundfaktura {
    kreditering.utkast(h, &o.rader, date(2026, 4, 2)).unwrap()
}

fn hela() -> NyKreditering {
    NyKreditering {
        hela: true,
        antal: Vec::new(),
    }
}

/// Verifikatraden med debet och kredit bytta.
fn vänd(r: &Rad) -> Rad {
    Rad {
        debet: r.kredit,
        kredit: r.debet,
        ..r.clone()
    }
}

#[test]
fn hela_fakturan_ger_ett_utkast_med_negativa_rader_och_en_hänvisning() {
    let o = original();
    let h = hänvisning(&o);
    let utkast = kreditera(&o, &h, hela());
    assert_eq!(utkast.krediterar, Some(h.clone()));
    assert_eq!(utkast.kund, "7");
    assert_eq!(utkast.fakturadatum, "2026-04-02");
    let antal: Vec<&str> = utkast.rader.iter().map(|r| r.antal.as_str()).collect();
    assert_eq!(antal, ["-10", "-3", "-1"]);
    assert_eq!(utkast.rader[0].à_pris, "950");
    assert_eq!(utkast.rader[1].rabatt, "10");
    assert_eq!(utkast.rader[0].konto, "3041");

    let k = utkast.kontrollera(&förutsättningar()).unwrap();
    assert_eq!(k.nummer, 1008);
    assert_eq!(k.total, -o.total);
    assert_eq!(k.moms, -o.moms);
    assert_eq!(k.krediterar, Some(h));
    assert_eq!(
        k.beskrivning(),
        "Kreditfaktura 1008, Kund AB, krediterar faktura 1007"
    );
    // Konteringen speglar originalets: intäkterna och momsen i debet, 1510 i kredit.
    let speglad: Vec<Rad> = o.kontering().iter().map(vänd).collect();
    assert_eq!(k.kontering(), speglad);
}

#[test]
fn ett_antal_per_rad_krediterar_den_delen_av_raderna() {
    let o = original();
    let h = hänvisning(&o);
    let utkast = kreditera(
        &o,
        &h,
        NyKreditering {
            hela: false,
            antal: vec!["2".into(), "".into(), "1".into()],
        },
    );
    assert_eq!(utkast.rader.len(), 2);
    let k = utkast.kontrollera(&förutsättningar()).unwrap();
    let netto: Vec<i64> = k.rader.iter().map(|r| r.netto).collect();
    assert_eq!(netto, [-190_000, -5_000]);
    assert_eq!(k.moms, -47_500);
    assert_eq!(k.total, -242_500);
    let öre = |n: i64| Belopp::från_öre(n);
    let kontering: Vec<(u16, Belopp, Belopp)> = k
        .kontering()
        .iter()
        .map(|r| (r.konto.get(), r.debet, r.kredit))
        .collect();
    assert_eq!(
        kontering,
        [
            (1510, öre(0), öre(242_500)),
            (3041, öre(190_000), öre(0)),
            (3004, öre(5_000), öre(0)),
            (2611, öre(47_500), öre(0)),
        ]
    );
}

#[test]
fn kreditfakturan_får_originalets_momsläge_och_kund() {
    let mut o = original();
    o.momsläge = Momsläge::Bygg;
    let mut h = hänvisning(&o);
    h.kund.namn = "Kund AB (då)".into();
    let mut utkast = kreditera(&o, &h, hela());
    // Kunden finns inte längre bland de aktiva, och utkastets val av omvänd skattskyldighet
    // spelar ingen roll.
    let mut ff = förutsättningar();
    ff.kunder.clear();
    ff.momsperioder = vec![(date(2026, 1, 1), Momsperiod::EjMomsregistrerad)];
    utkast.omvänd_skattskyldighet = "ingen".into();
    assert_eq!(utkast.momsläge(&ff), Momsläge::Bygg);
    let k = utkast.kontrollera(&ff).unwrap();
    assert_eq!(k.momsläge, Momsläge::Bygg);
    assert_eq!(k.kund.namn, "Kund AB (då)");
}

#[test]
fn en_kreditfaktura_spärras_om_summan_inte_är_negativ_eller_krediterar_för_mycket() {
    let o = original();
    let h = hänvisning(&o);
    let koder = |f: &NyKundfaktura| -> Vec<(Fält, &'static str)> {
        f.kontrollera(&förutsättningar())
            .err()
            .unwrap_or_default()
            .iter()
            .map(|s| (s.fält, s.kod))
            .collect()
    };
    let mut positiv = kreditera(&o, &h, hela());
    positiv.rader[2].antal = "1".into();
    assert_eq!(
        koder(&positiv),
        [(Fält::Rad(2, Radfält::Antal), "rad_antal_ogiltigt")]
    );
    let mut noll = kreditera(&o, &h, hela());
    for r in &mut noll.rader {
        r.à_pris = "0".into();
    }
    assert_eq!(koder(&noll), [(Fält::Summa, "summa_inte_negativ")]);
    let mut för_mycket = kreditera(&o, &h, hela());
    för_mycket.krediterar.as_mut().unwrap().kvar = 100;
    assert_eq!(koder(&för_mycket), [(Fält::Summa, "kredit_overstiger")]);
}

#[test]
fn utkastet_sparas_med_hänvisningen_och_blir_samma_formulär_igen() {
    let o = original();
    let utkast = kreditera(&o, &hänvisning(&o), hela());
    let sparat = utkast.som_utkast().unwrap();
    assert_eq!(sparat.krediterar, utkast.krediterar);
    let igen = sparat.som_formulär();
    assert_eq!(igen.krediterar, utkast.krediterar);
    assert_eq!(igen.rader, utkast.rader);
}

#[test]
fn valet_i_panelen_prövas_mot_originalets_rader_och_det_som_finns_kvar() {
    let o = original();
    let h = hänvisning(&o);
    let koder = |k: NyKreditering, h: &Hänvisning| -> Vec<(Option<usize>, &'static str)> {
        k.utkast(h, &o.rader, date(2026, 4, 2))
            .err()
            .unwrap_or_default()
            .iter()
            .map(|s| (s.rad, s.kod))
            .collect()
    };
    let antal = |a: &[&str]| NyKreditering {
        hela: false,
        antal: a.iter().map(|s| s.to_string()).collect(),
    };
    assert_eq!(
        koder(antal(&["11", "x", "-1"]), &h),
        [
            (Some(0), "kredit_antal_ogiltigt"),
            (Some(1), "kredit_antal_ogiltigt"),
            (Some(2), "kredit_antal_ogiltigt"),
        ]
    );
    assert_eq!(koder(antal(&["0", "", ""]), &h), [(None, "kredit_saknas")]);
    let mut delvis = h.clone();
    delvis.kvar = 50_000;
    assert_eq!(koder(hela(), &delvis), [(None, "kredit_overstiger")]);
    let mut krediterad = h.clone();
    krediterad.kvar = 0;
    assert_eq!(koder(hela(), &krediterad), [(None, "redan_krediterad")]);
    assert_eq!(
        KREDITERINGSSPÄRRAR
            .iter()
            .map(|(k, _)| *k)
            .collect::<Vec<_>>(),
        [
            "redan_krediterad",
            "kredit_antal_ogiltigt",
            "kredit_saknas",
            "kredit_overstiger"
        ]
    );
    assert_eq!(h.text(), "Krediterar faktura 1007 daterad 2026-03-10");
}
