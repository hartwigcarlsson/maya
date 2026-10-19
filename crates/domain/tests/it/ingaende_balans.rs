//! Kontrollen av den ingående balansen. Formuläret och servern kör samma kontroll.

use maya_domain::belopp::Saldo;
use maya_domain::ingaende_balans::{Fält, NyIngåendeBalans, NySaldorad};
use maya_domain::kontoplan::Kontonummer;

fn konto(n: u16) -> Kontonummer {
    Kontonummer::new(n).unwrap()
}

/// 1930, 2081 och 3001 är aktiva och 1950 inaktivt.
fn konton() -> Vec<(Kontonummer, bool)> {
    vec![
        (konto(1930), true),
        (konto(1950), false),
        (konto(2081), true),
        (konto(3001), true),
    ]
}

fn rad(konto: &str, debet: &str, kredit: &str) -> NySaldorad {
    NySaldorad {
        konto: konto.into(),
        debet: debet.into(),
        kredit: kredit.into(),
    }
}

fn balans(rader: Vec<NySaldorad>) -> NyIngåendeBalans {
    NyIngåendeBalans { rader }
}

#[test]
fn en_ingående_balans_vars_summa_är_noll_ger_saldon_med_debet_som_positivt() {
    let ib = balans(vec![
        rad("1930", "25 000", ""),
        rad("2081", "", "25 000,00"),
    ])
    .kontrollera(&konton())
    .unwrap();
    assert_eq!(
        ib.saldon,
        vec![
            (konto(1930), Saldo::från_öre(2_500_000)),
            (konto(2081), Saldo::från_öre(-2_500_000)),
        ]
    );
}

#[test]
fn rader_utan_belopp_räknas_inte_och_saldona_kommer_i_kontoordning() {
    let ib = balans(vec![
        rad("2081", "", "100"),
        rad("1950", "", ""),
        rad("", "", ""),
        rad("1930", "100", ""),
    ])
    .kontrollera(&konton())
    .unwrap();
    assert_eq!(
        ib.saldon,
        vec![
            (konto(1930), Saldo::från_öre(10_000)),
            (konto(2081), Saldo::från_öre(-10_000)),
        ]
    );
}

#[test]
fn en_ingående_balans_vars_summa_inte_är_noll_nekas_med_differensen() {
    let spärrar = balans(vec![rad("1930", "25 000", ""), rad("2081", "", "24 000")])
        .kontrollera(&konton())
        .unwrap_err();
    assert_eq!(spärrar.len(), 1);
    assert_eq!(
        (spärrar[0].fält, spärrar[0].kod),
        (Fält::Summa, "differens")
    );
    assert!(spärrar[0].text.contains("1 000,00"), "{}", spärrar[0].text);
}

#[test]
fn bara_aktiva_balanskonton_i_kontoplanen_får_ett_saldo() {
    let spärrar = balans(vec![
        rad("3001", "", "200"),
        rad("1950", "100", ""),
        rad("1999", "100", ""),
        rad("1930", "", "100"),
        rad("1930", "100", ""),
    ])
    .kontrollera(&konton())
    .unwrap_err();
    let koder: Vec<_> = spärrar.iter().map(|s| (s.fält, s.kod)).collect();
    assert_eq!(
        koder,
        vec![
            (Fält::Konto(0), "konto_inte_balanskonto"),
            (Fält::Konto(1), "konto_inaktivt"),
            (Fält::Konto(2), "konto_okant"),
            (Fält::Konto(4), "konto_dubblett"),
        ]
    );
}

#[test]
fn ett_belopp_som_inte_går_att_tolka_eller_står_i_båda_kolumnerna_nekas() {
    let spärrar = balans(vec![rad("1930", "12,345", ""), rad("2081", "5", "5")])
        .kontrollera(&konton())
        .unwrap_err();
    let koder: Vec<_> = spärrar.iter().map(|s| (s.fält, s.kod)).collect();
    assert_eq!(
        koder,
        vec![
            (Fält::Belopp(0), "belopp_ogiltigt"),
            (Fält::Belopp(1), "belopp_debet_och_kredit"),
        ]
    );
}

#[test]
fn ett_saldo_visas_med_minustecken_när_det_är_kredit() {
    assert_eq!(Saldo::från_öre(-123_450).to_string(), "-1 234,50");
    assert_eq!(Saldo::från_öre(5).to_string(), "0,05");
    assert_eq!(Saldo::från_öre(-5).to_string(), "-0,05");
}
