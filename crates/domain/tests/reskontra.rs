//! Status och gruppering i reskontran räknas fram ur fakturans total, det som återstår,
//! förfallodagen och dagens datum. Inget av det lagras.

use jiff::civil::date;
use maya_domain::reskontra::{Grupp, Märke, förfallotext, grupp, märke};

#[test]
fn märket_följer_det_som_återstår() {
    let idag = date(2026, 10, 19);
    let förfaller = date(2026, 11, 18);
    assert_eq!(märke(10_000, 10_000, förfaller, idag), Märke::Obetald);
    assert_eq!(märke(10_000, 2_500, förfaller, idag), Märke::Delbetald);
    assert_eq!(märke(10_000, 0, förfaller, idag), Märke::Betald);
    assert_eq!(Märke::Delbetald.namn(), "Delbetald");
    assert_eq!(Märke::Förfallen.kod(), "forfallen");
}

#[test]
fn en_obetald_rest_efter_förfallodagen_är_förfallen_men_inte_på_dagen() {
    let förfallodag = date(2026, 10, 19);
    assert_eq!(
        märke(10_000, 10_000, förfallodag, date(2026, 10, 19)),
        Märke::Obetald
    );
    assert_eq!(
        märke(10_000, 10_000, förfallodag, date(2026, 10, 20)),
        Märke::Förfallen
    );
    assert_eq!(
        märke(10_000, 1, förfallodag, date(2026, 10, 20)),
        Märke::Förfallen
    );
    assert_eq!(
        märke(10_000, 0, förfallodag, date(2026, 12, 1)),
        Märke::Betald
    );
}

#[test]
fn listan_grupperas_efter_förfallodagen() {
    let idag = date(2026, 10, 19);
    assert_eq!(grupp(1, date(2026, 10, 18), idag), Grupp::Förfallna);
    assert_eq!(grupp(1, date(2026, 10, 19), idag), Grupp::InomSjuDagar);
    assert_eq!(grupp(1, date(2026, 10, 26), idag), Grupp::InomSjuDagar);
    assert_eq!(grupp(1, date(2026, 10, 27), idag), Grupp::Senare);
    assert_eq!(grupp(0, date(2026, 10, 1), idag), Grupp::Avslutade);
    let rubriker: Vec<&str> = Grupp::ALLA.iter().map(|g| g.rubrik()).collect();
    assert_eq!(
        rubriker,
        [
            "Förfallna",
            "Förfaller inom 7 dagar",
            "Senare",
            "Utkast",
            "Avslutade"
        ]
    );
}

#[test]
fn förfallotexten_räknar_dagarna() {
    let idag = date(2026, 10, 19);
    assert_eq!(förfallotext(date(2026, 10, 19), idag), "förfaller i dag");
    assert_eq!(förfallotext(date(2026, 10, 20), idag), "förfaller om 1 dag");
    assert_eq!(förfallotext(date(2026, 10, 24), idag), "förfaller om 5 dagar");
    assert_eq!(förfallotext(date(2026, 10, 18), idag), "förföll för 1 dag sedan");
    assert_eq!(förfallotext(date(2026, 10, 9), idag), "förföll för 10 dagar sedan");
}
