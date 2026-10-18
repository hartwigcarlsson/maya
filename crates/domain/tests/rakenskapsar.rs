use jiff::civil::{Date, date};
use maya_domain::rakenskapsar::{Räkenskapsår, RäkenskapsårsFel};

fn år(start: Date, slut: Date) -> Result<Räkenskapsår, RäkenskapsårsFel> {
    Räkenskapsår::första(start, slut)
}

#[test]
fn ett_kalenderår_har_tolv_hela_perioder() {
    let år = år(date(2026, 1, 1), date(2026, 12, 31)).unwrap();

    let perioder = år.perioder();
    assert_eq!(perioder.len(), 12);
    assert_eq!(perioder[0].start, date(2026, 1, 1));
    assert_eq!(perioder[0].slut, date(2026, 1, 31));
    assert_eq!(perioder[1].slut, date(2026, 2, 28));
    assert_eq!(perioder[11].start, date(2026, 12, 1));
    assert_eq!(perioder[11].slut, date(2026, 12, 31));
}

#[test]
fn den_första_perioden_kan_vara_en_del_av_en_månad() {
    let år = år(date(2026, 3, 17), date(2027, 6, 30)).unwrap();

    let perioder = år.perioder();
    assert_eq!(perioder.len(), 16);
    assert_eq!(perioder[0].start, date(2026, 3, 17));
    assert_eq!(perioder[0].slut, date(2026, 3, 31));
    assert_eq!(perioder[1].start, date(2026, 4, 1));
    assert_eq!(perioder[15].slut, date(2027, 6, 30));
}

#[test]
fn året_ska_sluta_vid_ett_månadsslut() {
    assert_eq!(
        år(date(2026, 1, 1), date(2026, 12, 30)),
        Err(RäkenskapsårsFel::SlutInteMånadsslut)
    );
}

#[test]
fn det_första_året_är_högst_18_månader() {
    assert!(år(date(2026, 1, 1), date(2027, 6, 30)).is_ok());
    assert!(år(date(2026, 1, 15), date(2027, 6, 30)).is_ok());
    assert_eq!(
        år(date(2026, 1, 1), date(2027, 7, 31)),
        Err(RäkenskapsårsFel::FörLångt)
    );
    assert_eq!(
        år(date(2025, 12, 31), date(2027, 6, 30)),
        Err(RäkenskapsårsFel::FörLångt)
    );
}

#[test]
fn det_första_året_är_minst_en_månad() {
    // BFL 3:3: mellan en och arton månader.
    assert!(år(date(2026, 10, 1), date(2026, 10, 31)).is_ok());
    assert!(år(date(2026, 10, 15), date(2026, 11, 30)).is_ok());
    assert_eq!(
        år(date(2026, 10, 31), date(2026, 10, 31)),
        Err(RäkenskapsårsFel::FörKort)
    );
    assert_eq!(
        år(date(2026, 10, 15), date(2026, 10, 31)),
        Err(RäkenskapsårsFel::FörKort)
    );
    assert_eq!(RäkenskapsårsFel::FörKort.kod(), "ar_for_kort");
    assert_eq!(
        RäkenskapsårsFel::från_kod("ar_for_kort"),
        Some(RäkenskapsårsFel::FörKort)
    );
}

#[test]
fn året_kan_inte_sluta_före_starten() {
    assert_eq!(
        år(date(2026, 7, 1), date(2026, 6, 30)),
        Err(RäkenskapsårsFel::SlutFöreStart)
    );
}

#[test]
fn varje_fel_har_en_kod() {
    for (fel, kod) in [
        (RäkenskapsårsFel::SlutInteMånadsslut, "ar_slut_manadsslut"),
        (RäkenskapsårsFel::FörLångt, "ar_for_langt"),
        (RäkenskapsårsFel::SlutFöreStart, "ar_slut_fore_start"),
    ] {
        assert_eq!(fel.kod(), kod);
        assert_eq!(RäkenskapsårsFel::från_kod(kod), Some(fel));
        assert!(!fel.to_string().is_empty());
    }
}

#[test]
fn nästa_räkenskapsår_är_tolv_månader_med_samma_slutmånad() {
    let första = år(date(2026, 3, 17), date(2027, 6, 30)).unwrap();

    let nästa = första.nästa();
    assert_eq!(
        (nästa.start(), nästa.slut()),
        (date(2027, 7, 1), date(2028, 6, 30))
    );
    assert_eq!(nästa.perioder().len(), 12);
    let tredje = nästa.nästa();
    assert_eq!(
        (tredje.start(), tredje.slut()),
        (date(2028, 7, 1), date(2029, 6, 30))
    );
}

#[test]
fn nästa_räkenskapsår_efter_ett_år_som_slutar_i_februari_slutar_den_sista_februari() {
    let första = år(date(2027, 3, 1), date(2028, 2, 29)).unwrap();

    let nästa = första.nästa();
    assert_eq!(
        (nästa.start(), nästa.slut()),
        (date(2028, 3, 1), date(2029, 2, 28))
    );
    assert_eq!(nästa.nästa().slut(), date(2030, 2, 28));
}
