use jiff::civil::date;
use maya_domain::belopp::Saldo;
use maya_domain::kontoplan::{Inaktiveringsspärr, Kontonummer, kan_inaktiveras, Årssaldo};

fn nr(n: u16) -> Kontonummer {
    Kontonummer::new(n).unwrap()
}

fn år_2026(öre: i64) -> Årssaldo {
    Årssaldo {
        start: date(2026, 1, 1),
        slut: date(2026, 12, 31),
        saldo: Saldo::från_öre(öre),
    }
}

#[test]
fn ett_konto_utan_saldo_kan_inaktiveras() {
    assert_eq!(kan_inaktiveras(nr(6540), &[]), Ok(()));
    assert_eq!(kan_inaktiveras(nr(6540), &[år_2026(0)]), Ok(()));
}

#[test]
fn ett_saldo_i_ett_öppet_år_nekar_med_skäl_och_saldo() {
    let spärr = kan_inaktiveras(nr(1910), &[år_2026(0), år_2026(-123_456_778)]).unwrap_err();

    assert_eq!(spärr, Inaktiveringsspärr::Saldo(år_2026(-123_456_778)));
    assert_eq!(spärr.kod(), "har_saldo");
    assert_eq!(
        spärr.to_string(),
        "Saldo -1 234 567,78 kr i räkenskapsåret 2026-01-01–2026-12-31, som inte är stängt. \
         Bara konton med nollsaldo kan inaktiveras."
    );
}

#[test]
fn flödenas_konton_kan_aldrig_inaktiveras_inte_ens_med_nollsaldo() {
    let spärr = kan_inaktiveras(nr(2099), &[]).unwrap_err();

    assert_eq!(
        spärr,
        Inaktiveringsspärr::Flöde("Preliminär ingående balans")
    );
    assert_eq!(spärr.kod(), "flodets_konto");
    assert_eq!(
        spärr.to_string(),
        "Används av flödet Preliminär ingående balans"
    );
}
