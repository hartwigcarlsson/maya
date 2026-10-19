//! Spärrarna vid bokföring. Formuläret och servern kör samma kontroll.

use jiff::civil::date;
use maya_domain::belopp::Belopp;
use maya_domain::kontoplan::Kontonummer;
use maya_domain::rakenskapsar::Räkenskapsår;
use maya_domain::verifikat::{Fält, Förutsättningar, NyRad, NyttVerifikat, Årsval};

fn konto(n: u16) -> Kontonummer {
    Kontonummer::new(n).unwrap()
}

/// Ett företag med kalenderåret 2026, där 1930 och 3001 är aktiva och 1950 inaktivt.
fn förutsättningar() -> Förutsättningar {
    Förutsättningar {
        konton: vec![
            (konto(1930), true),
            (konto(1950), false),
            (konto(3001), true),
        ],
        räkenskapsår: vec![Räkenskapsår::första(date(2026, 1, 1), date(2026, 12, 31)).unwrap()],
    }
}

fn rad(konto: &str, debet: &str, kredit: &str) -> NyRad {
    NyRad {
        konto: konto.into(),
        debet: debet.into(),
        kredit: kredit.into(),
        text: String::new(),
    }
}

fn försäljning() -> NyttVerifikat {
    NyttVerifikat {
        datum: "2026-03-14".into(),
        beskrivning: "Försäljning kontant".into(),
        rader: vec![rad("1930", "1 250,00", ""), rad("3001", "", "1250")],
    }
}

fn koder(v: &NyttVerifikat) -> Vec<(Fält, &'static str)> {
    v.kontrollera(&förutsättningar())
        .unwrap_err()
        .into_iter()
        .map(|s| (s.fält, s.kod))
        .collect()
}

#[test]
fn ett_balanserat_verifikat_på_aktiva_konton_går_att_bokföra() {
    let v = försäljning().kontrollera(&förutsättningar()).unwrap();

    assert_eq!(v.datum, date(2026, 3, 14));
    assert_eq!(v.beskrivning, "Försäljning kontant");
    assert_eq!(v.rader.len(), 2);
    assert_eq!(v.rader[0].konto, konto(1930));
    assert_eq!(v.rader[0].debet, Belopp::från_öre(125_000));
    assert_eq!(v.rader[0].kredit, Belopp::NOLL);
    assert_eq!(v.rader[1].kredit, Belopp::från_öre(125_000));
    assert_eq!(v.summa(), Belopp::från_öre(125_000));
    assert_eq!(
        v.räkenskapsår,
        Årsval::Befintligt(Räkenskapsår::befintligt(
            date(2026, 1, 1),
            date(2026, 12, 31)
        ))
    );
    assert_eq!(
        (v.period.start, v.period.slut),
        (date(2026, 3, 1), date(2026, 3, 31))
    );
}

#[test]
fn radtexten_följer_med_och_tomma_rader_räknas_inte() {
    let mut f = försäljning();
    f.rader[0].text = "  Kassan  ".into();
    f.rader.insert(1, rad("", "", ""));

    let v = f.kontrollera(&förutsättningar()).unwrap();

    assert_eq!(v.rader.len(), 2);
    assert_eq!(v.rader[0].text.as_deref(), Some("Kassan"));
    assert_eq!(v.rader[1].text, None);
}

#[test]
fn ett_verifikat_utan_datum_eller_beskrivning_spärras() {
    let mut f = försäljning();
    f.datum = " ".into();
    f.beskrivning = "  ".into();

    assert_eq!(
        koder(&f),
        [
            (Fält::Datum, "datum_saknas"),
            (Fält::Beskrivning, "beskrivning_saknas")
        ]
    );
    f.datum = "14/3".into();
    assert_eq!(koder(&f)[0], (Fält::Datum, "datum_ogiltigt"));
}

#[test]
fn ett_verifikat_med_färre_än_två_rader_spärras() {
    let mut f = försäljning();
    f.rader = vec![rad("", "", ""), rad("", "", "")];
    assert_eq!(koder(&f), [(Fält::Rader, "rader_for_fa")]);

    f.rader = vec![rad("1930", "10", ""), rad("", "", "")];
    assert_eq!(
        koder(&f),
        [(Fält::Rader, "rader_for_fa"), (Fält::Summa, "differens")]
    );
}

#[test]
fn debet_som_inte_är_lika_med_kredit_spärras_vid_summan() {
    let mut f = försäljning();
    f.rader[1].kredit = "1 200".into();

    let spärrar = f.kontrollera(&förutsättningar()).unwrap_err();
    assert_eq!(spärrar.len(), 1);
    assert_eq!(
        (spärrar[0].fält, spärrar[0].kod),
        (Fält::Summa, "differens")
    );
    assert!(spärrar[0].text.contains("50,00"), "{}", spärrar[0].text);
}

#[test]
fn ett_okänt_eller_inaktivt_konto_spärras_vid_raden() {
    let mut f = försäljning();
    f.rader.push(rad("1950", "10", ""));
    f.rader.push(rad("9999", "", "10"));
    f.rader.push(rad("abc", "1", ""));
    f.rader.push(rad("", "", "1"));

    assert_eq!(
        koder(&f),
        [
            (Fält::Konto(2), "konto_inaktivt"),
            (Fält::Konto(3), "konto_okant"),
            (Fält::Konto(4), "konto_okant"),
            (Fält::Konto(5), "konto_saknas"),
        ]
    );
}

#[test]
fn en_rad_har_ett_belopp_antingen_i_debet_eller_i_kredit() {
    let mut f = försäljning();
    f.rader.push(rad("1930", "", ""));
    f.rader.push(rad("1930", "5", "5"));
    f.rader.push(rad("1930", "5,555", ""));
    f.rader.push(rad("1930", "0", ""));

    assert_eq!(
        koder(&f),
        [
            (Fält::Belopp(2), "belopp_saknas"),
            (Fält::Belopp(3), "belopp_debet_och_kredit"),
            (Fält::Belopp(4), "belopp_ogiltigt"),
            (Fält::Belopp(5), "belopp_saknas"),
        ]
    );
}

#[test]
fn ett_datum_före_första_räkenskapsårets_start_spärras() {
    let mut f = försäljning();
    f.datum = "2025-12-31".into();

    assert_eq!(koder(&f), [(Fält::Datum, "fore_forsta_aret")]);
}

#[test]
fn ett_datum_i_året_efter_det_senaste_ger_ett_nytt_räkenskapsår() {
    let mut f = försäljning();
    f.datum = "2027-12-31".into();

    let v = f.kontrollera(&förutsättningar()).unwrap();

    assert_eq!(
        v.räkenskapsår,
        Årsval::Nytt(Räkenskapsår::befintligt(
            date(2027, 1, 1),
            date(2027, 12, 31)
        ))
    );
    assert_eq!(v.period.start, date(2027, 12, 1));
}

#[test]
fn ett_datum_längre_fram_än_året_efter_det_senaste_spärras() {
    let mut f = försäljning();
    f.datum = "2028-01-01".into();

    assert_eq!(koder(&f), [(Fält::Datum, "saknat_rakenskapsar")]);
}

#[test]
fn varje_spärr_har_en_text_för_användaren() {
    let mut f = försäljning();
    f.beskrivning.clear();
    let spärrar = f.kontrollera(&förutsättningar()).unwrap_err();
    assert_eq!(spärrar[0].text, "Krävs för att bokföra.");
}
