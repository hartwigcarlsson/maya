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

// #140 Kreditfakturor: belopp med tecken på fakturor.

#[test]
fn ett_fakturabelopp_kan_ha_ett_minustecken_först() {
    use maya_domain::belopp::tolka_med_tecken;
    assert_eq!(tolka_med_tecken("1 234,50"), Some(123_450));
    assert_eq!(tolka_med_tecken("-1 234,50"), Some(-123_450));
    assert_eq!(tolka_med_tecken(" −400 "), Some(-40_000));
    assert_eq!(tolka_med_tecken("- 5"), Some(-500));
    assert_eq!(tolka_med_tecken("-0"), Some(0));
    for text in ["", "-", "--5", "5-", "-12,345", "+5", "abc"] {
        assert_eq!(tolka_med_tecken(text), None, "{text:?}");
    }
}

#[test]
fn ett_fakturabelopp_skrivs_i_formuläret_utan_tusental_och_utan_noll_öre() {
    use maya_domain::belopp::{formulärtext, tolka_med_tecken};
    assert_eq!(formulärtext(123_450), "1234,50");
    assert_eq!(formulärtext(40_000), "400");
    assert_eq!(formulärtext(-150), "-1,50");
    assert_eq!(formulärtext(-5), "-0,05");
    assert_eq!(formulärtext(0), "0");
    for öre in [-123_456, -1, 0, 7, 99_999_999] {
        assert_eq!(tolka_med_tecken(&formulärtext(öre)), Some(öre));
    }
}
