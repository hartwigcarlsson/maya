//! Status och gruppering i reskontran räknas fram ur fakturans total, det som återstår,
//! förfallodagen och dagens datum. Inget av det lagras.

use jiff::civil::date;
use maya_domain::reskontra::{
    Grupp, Märke, förfallotext, grupp, kopieringschips, kundfakturans_märke, märke,
};

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
    assert_eq!(
        förfallotext(date(2026, 10, 24), idag),
        "förfaller om 5 dagar"
    );
    assert_eq!(
        förfallotext(date(2026, 10, 18), idag),
        "förföll för 1 dag sedan"
    );
    assert_eq!(
        förfallotext(date(2026, 10, 9), idag),
        "förföll för 10 dagar sedan"
    );
}

// #141 Kundförlust
#[test]
fn en_gällande_kundförlust_ger_märket_kundförlust_och_fakturan_är_avslutad() {
    let idag = date(2026, 10, 19);
    let förföll = date(2026, 4, 9);
    assert_eq!(
        kundfakturans_märke(10_000, 0, 0, true, förföll, idag),
        Märke::Kundförlust
    );
    assert_eq!(Märke::Kundförlust.namn(), "Kundförlust");
    assert_eq!(Märke::Kundförlust.kod(), "kundforlust");
    assert_eq!(grupp(0, förföll, idag), Grupp::Avslutade);
    // En återkallad förlust räknas inte: fakturan är förfallen igen.
    assert_eq!(
        kundfakturans_märke(10_000, 0, 10_000, false, förföll, idag),
        Märke::Förfallen
    );
}

// #152 Att göra: kopieringschipsen för en leverantörsfaktura att betala
#[test]
fn kopieringschipsen_visar_betalsätten_och_referensen_som_ocr_eller_meddelande() {
    assert_eq!(
        kopieringschips("5050-1055", "", "", "8812 0044 711"),
        [
            ("BG", "5050-1055".to_string()),
            ("OCR", "88120044711".to_string())
        ]
    );
    assert_eq!(
        kopieringschips(
            "",
            "12 34 56-7",
            "SE45 5000 0000 0583 9825 7466",
            " Hyra nov, objekt 0412 "
        ),
        [
            ("PG", "12 34 56-7".to_string()),
            ("IBAN", "SE45 5000 0000 0583 9825 7466".to_string()),
            ("Meddelande", "Hyra nov, objekt 0412".to_string())
        ]
    );
    assert!(kopieringschips(" ", "", "", "  ").is_empty());
}

// #140 Kreditfakturor

#[test]
fn en_krediterad_faktura_och_ett_tillgodohavande_får_egna_märken() {
    let idag = date(2026, 10, 19);
    let förfaller = date(2026, 10, 1);
    // Hela krediterad, inget betalt: inget återstår och inget är förfallet.
    assert_eq!(
        kundfakturans_märke(10_000, 10_000, 0, false, förfaller, idag),
        Märke::Krediterad
    );
    // Betald och sedan krediterad: kunden har pengar tillgodo.
    assert_eq!(
        kundfakturans_märke(10_000, 4_000, -4_000, false, förfaller, idag),
        Märke::Tillgodo
    );
    // Tillgodohavandet återbetalt: fakturan är delkrediterad och betald.
    assert_eq!(
        kundfakturans_märke(10_000, 4_000, 0, false, förfaller, idag),
        Märke::Betald
    );
    // Delkrediterad och obetald: resten är obetald, inte delbetald.
    assert_eq!(
        kundfakturans_märke(10_000, 4_000, 6_000, false, date(2026, 11, 1), idag),
        Märke::Obetald
    );
    assert_eq!(
        kundfakturans_märke(10_000, 4_000, 1_000, false, date(2026, 11, 1), idag),
        Märke::Delbetald
    );
    assert_eq!(
        kundfakturans_märke(10_000, 4_000, 6_000, false, förfaller, idag),
        Märke::Förfallen
    );
    assert_eq!(märke(10_000, -1, förfaller, idag), Märke::Tillgodo);
    assert_eq!(Märke::Tillgodo.namn(), "Tillgodo");
    assert_eq!(Märke::Krediterad.kod(), "krediterad");
    assert_eq!(Märke::Kreditfaktura.namn(), "Kreditfaktura");
    // #140 och #141: en kundförlust på en delkrediterad faktura gäller det som återstod.
    assert_eq!(
        kundfakturans_märke(10_000, 4_000, 0, true, förfaller, idag),
        Märke::Kundförlust
    );
}
