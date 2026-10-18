use maya_domain::belopp::Belopp;

#[test]
fn ett_belopp_tolkas_i_kronor_och_lagras_i_öre() {
    assert_eq!(Belopp::tolka("125").unwrap().öre(), 12_500);
    assert_eq!(Belopp::tolka("125,5").unwrap().öre(), 12_550);
    assert_eq!(Belopp::tolka("125,05").unwrap().öre(), 12_505);
    assert_eq!(Belopp::tolka("1 234,50").unwrap().öre(), 123_450);
    assert_eq!(Belopp::tolka(" 1234.50 ").unwrap().öre(), 123_450);
    assert_eq!(Belopp::tolka("0,01").unwrap().öre(), 1);
}

#[test]
fn ett_belopp_med_fler_än_två_decimaler_eller_annat_än_siffror_nekas() {
    for text in ["", "12,345", "abc", "1,2,3", "-5", "12kr", ",50", "1e3"] {
        assert_eq!(Belopp::tolka(text), None, "{text:?}");
    }
}

#[test]
fn ett_belopp_visas_med_tusentalsavgränsare_och_decimalkomma() {
    assert_eq!(Belopp::från_öre(123_450).to_string(), "1 234,50");
    assert_eq!(Belopp::från_öre(5).to_string(), "0,05");
    assert_eq!(Belopp::från_öre(100_000_000).to_string(), "1 000 000,00");
}
