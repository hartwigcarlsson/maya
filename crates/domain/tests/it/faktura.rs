use maya_domain::faktura::{Antal, Momsrad, Momssats, Rabatt, moms, momsrader, ocr, radnetto};

#[test]
fn ocr_referensen_är_numret_med_längdsiffra_och_kontrollsiffra() {
    // 1007: fyra siffror plus längdsiffran och kontrollsiffran blir sex, längdsiffran 6.
    // Luhn över 10076: 6·2=12→3, 7, 0·2=0, 0, 1·2=2 ger 12, kontrollsiffran 8.
    assert_eq!(ocr(1007), "100768");
    // 1: tre siffror, längdsiffran 3; Luhn över 13 ger 3.
    assert_eq!(ocr(1), "133");
    // Åtta siffror ger tio, och längdsiffran är tio modulo tio, 0.
    let åtta = ocr(12_345_678);
    assert_eq!(åtta.len(), 10);
    assert_eq!(&åtta[..9], "123456780");
    assert!(maya_domain::luhn::stämmer(&åtta));
}

#[test]
fn varje_ocr_referens_klarar_luhn_och_bär_sin_längd() {
    for nummer in [1, 9, 10, 99, 100, 1007, 54_321, 999_999, 1_000_000] {
        let referens = ocr(nummer);
        assert!(maya_domain::luhn::stämmer(&referens), "{referens}");
        let längdsiffra = referens.as_bytes()[referens.len() - 2] - b'0';
        assert_eq!(usize::from(längdsiffra), referens.len() % 10, "{referens}");
        assert!(referens.starts_with(&nummer.to_string()));
    }
}

#[test]
fn momssatserna_är_25_12_6_och_0_procent() {
    let satser: Vec<u8> = Momssats::ALLA.iter().map(|s| s.procent()).collect();
    assert_eq!(satser, [25, 12, 6, 0]);
    assert_eq!(Momssats::tolka("12"), Some(Momssats::Tolv));
    assert_eq!(Momssats::tolka(" 6 % "), Some(Momssats::Sex));
    assert_eq!(Momssats::tolka("8"), None);
    assert_eq!(Momssats::Tjugofem.to_string(), "25 %");
}

#[test]
fn utgående_moms_bokförs_på_2611_2621_och_2631() {
    let konto = |s: Momssats| s.utgående_momskonto().map(|k| k.get());
    assert_eq!(konto(Momssats::Tjugofem), Some(2611));
    assert_eq!(konto(Momssats::Tolv), Some(2621));
    assert_eq!(konto(Momssats::Sex), Some(2631));
    assert_eq!(konto(Momssats::Noll), None);
}

#[test]
fn momsen_avrundas_till_närmaste_öre_och_halva_öre_uppåt() {
    assert_eq!(moms(10_000, Momssats::Tjugofem), 2_500);
    // 0,02 kr · 25 % = 0,5 öre, som blir 1 öre.
    assert_eq!(moms(2, Momssats::Tjugofem), 1);
    // 0,05 kr · 6 % = 0,3 öre, som blir 0.
    assert_eq!(moms(5, Momssats::Sex), 0);
    // 1,13 kr · 12 % = 13,56 öre, som blir 14.
    assert_eq!(moms(113, Momssats::Tolv), 14);
    assert_eq!(moms(123_456, Momssats::Noll), 0);
    // Negativt (kreditfakturor): avrundningen är symmetrisk.
    assert_eq!(moms(-2, Momssats::Tjugofem), -1);
}

#[test]
fn antal_skrivs_med_högst_två_decimaler() {
    assert_eq!(Antal::tolka("3").unwrap().hundradelar(), 300);
    assert_eq!(Antal::tolka("1,5").unwrap().hundradelar(), 150);
    assert_eq!(Antal::tolka(" 0.25 ").unwrap().hundradelar(), 25);
    assert_eq!(Antal::tolka("1 000").unwrap().hundradelar(), 100_000);
    assert_eq!(Antal::tolka("-2").unwrap().hundradelar(), -200);
    assert_eq!(Antal::tolka("1,555"), None);
    assert_eq!(Antal::tolka("tre"), None);
    assert_eq!(Antal::tolka(""), None);
    assert_eq!(Antal::tolka("1,5").unwrap().to_string(), "1,5");
    assert_eq!(Antal::tolka("12").unwrap().to_string(), "12");
    assert_eq!(Antal::tolka("0,25").unwrap().to_string(), "0,25");
}

#[test]
fn rabatten_är_procent_från_0_till_100() {
    assert_eq!(Rabatt::tolka("").unwrap(), Rabatt::INGEN);
    assert_eq!(Rabatt::tolka("10").unwrap().hundradelar(), 1_000);
    assert_eq!(Rabatt::tolka("12,5 %").unwrap().hundradelar(), 1_250);
    assert_eq!(Rabatt::tolka("100").unwrap().hundradelar(), 10_000);
    assert_eq!(Rabatt::tolka("101"), None);
    assert_eq!(Rabatt::tolka("-5"), None);
    assert_eq!(Rabatt::tolka("12,5").unwrap().to_string(), "12,5");
}

#[test]
fn radens_netto_är_antal_gånger_à_pris_minus_rabatt_avrundat_till_öre() {
    let antal = |t| Antal::tolka(t).unwrap();
    let rabatt = |t| Rabatt::tolka(t).unwrap();
    // 3 timmar à 950 kr.
    assert_eq!(radnetto(antal("3"), 95_000, Rabatt::INGEN), 285_000);
    // 1,5 · 333,33 kr = 499,995 kr, som blir 500,00 kr.
    assert_eq!(radnetto(antal("1,5"), 33_333, Rabatt::INGEN), 50_000);
    // 2 · 100 kr med 12,5 % rabatt = 175 kr.
    assert_eq!(radnetto(antal("2"), 10_000, rabatt("12,5")), 17_500);
    // 0,33 · 1,01 kr = 0,3333 kr, som blir 33 öre.
    assert_eq!(radnetto(antal("0,33"), 101, Rabatt::INGEN), 33);
    assert_eq!(radnetto(antal("4"), 10_000, rabatt("100")), 0);
}

#[test]
fn momsen_räknas_per_sats_på_summan_av_radernas_netto() {
    // Tre rader à 0,02 kr med 25 %: per rad blir det 3 · 1 öre, på summan 0,06 · 25 % = 1,5
    // som blir 2 öre.
    let rader = momsrader([
        (Momssats::Tjugofem, 2),
        (Momssats::Sex, 10_000),
        (Momssats::Tjugofem, 2),
        (Momssats::Noll, 5_000),
        (Momssats::Tjugofem, 2),
    ]);
    assert_eq!(
        rader,
        [
            Momsrad {
                sats: Momssats::Tjugofem,
                underlag: 6,
                moms: 2
            },
            Momsrad {
                sats: Momssats::Sex,
                underlag: 10_000,
                moms: 600
            },
            Momsrad {
                sats: Momssats::Noll,
                underlag: 5_000,
                moms: 0
            },
        ]
    );
}
