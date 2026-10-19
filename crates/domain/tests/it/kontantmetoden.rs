//! Kontantmetoden (#149): när fakturans fordran eller skuld inte är bokförd fördelas en
//! betalning, en kredit eller T2:s obetalda belopp på fakturans intäkt eller kostnad och moms i
//! proportion till deras andel av totalen, och den fiktiva momsen följer med som exakta par.

use maya_domain::belopp::Belopp;
use maya_domain::kontantmetoden::{Obokförd, kreditens_bokförda_del};
use maya_domain::kontoplan::Kontonummer;
use maya_domain::verifikat::Rad;

fn konto(nr: u16) -> Kontonummer {
    Kontonummer::new(nr).unwrap()
}

/// En verifikatrad med `öre` i debet, eller i kredit när det är negativt.
fn rad(nr: u16, öre: i64) -> Rad {
    Rad {
        konto: konto(nr),
        debet: Belopp::från_öre(öre.max(0)),
        kredit: Belopp::från_öre((-öre).max(0)),
        text: None,
    }
}

fn belopp(rader: &[Rad]) -> Vec<(u16, i64)> {
    rader
        .iter()
        .map(|r| (r.konto.get(), r.debet.öre() - r.kredit.öre()))
        .collect()
}

/// Kundfakturan 12 240 kr: 3001 netto 8 000 kr med 25 % moms och 3002 netto 2 000 kr med 12 %.
fn kundfakturan() -> Obokförd {
    Obokförd::av(
        &[
            rad(1510, 1_224_000),
            rad(3001, -800_000),
            rad(3002, -200_000),
            rad(2611, -200_000),
            rad(2621, -24_000),
        ],
        konto(1510),
    )
}

#[test]
fn totalen_är_reskontrakontots_belopp() {
    assert_eq!(kundfakturan().total(), 1_224_000);
}

#[test]
fn hela_totalen_ger_fakturans_kontering_utan_reskontrakontot() {
    assert_eq!(
        belopp(&kundfakturan().andel(1_224_000)),
        [
            (3001, -800_000),
            (3002, -200_000),
            (2611, -200_000),
            (2621, -24_000)
        ]
    );
}

#[test]
fn en_delbetalning_fördelas_i_proportion_och_summerar_exakt() {
    // 1 000 kr av 12 240 kr: 653,59 / 163,40 / 163,40 / 19,61 före avrundningen.
    let delar = belopp(&kundfakturan().andel(100_000));
    assert_eq!(
        delar,
        [
            (3001, -65_359),
            (3002, -16_340),
            (2611, -16_340),
            (2621, -1_961)
        ]
    );
    assert_eq!(delar.iter().map(|(_, b)| b).sum::<i64>(), -100_000);
}

#[test]
fn konteringen_för_ett_belopp_har_reskontrakontot_med_beloppet() {
    let k = belopp(&kundfakturan().kontering(1_224_000));
    assert_eq!(k[0], (1510, 1_224_000));
    assert_eq!(k.iter().map(|(_, b)| b).sum::<i64>(), 0);
}

#[test]
fn en_kreditfakturas_andel_har_debet_och_kredit_bytta() {
    // Kreditfakturan -2 500 kr: 3001 2 000 och 2611 500 i debet, 1510 i kredit.
    let kredit = Obokförd::av(
        &[rad(1510, -250_000), rad(3001, 200_000), rad(2611, 50_000)],
        konto(1510),
    );
    assert_eq!(kredit.total(), -250_000);
    assert_eq!(
        belopp(&kredit.kontering(-100_000)),
        [(1510, -100_000), (3001, 80_000), (2611, 20_000)]
    );
}

/// Leverantörsfakturan med omvänd skattskyldighet (EU-tjänst): 5410 netto 1 000 kr, fiktiv moms
/// 250 kr på 2645 och 2614.
fn omvänd() -> Obokförd {
    Obokförd::av(
        &[
            rad(5410, 100_000),
            rad(2645, 25_000),
            rad(2440, -100_000),
            rad(2614, -25_000),
        ],
        konto(2440),
    )
}

#[test]
fn den_fiktiva_momsen_ingår_inte_i_totalen() {
    assert_eq!(omvänd().total(), -100_000);
}

#[test]
fn den_fiktiva_momsen_fördelas_som_ett_exakt_par() {
    // 333,33 kr av 1 000 kr: fiktiv moms 83,33 kr på båda sidor.
    assert_eq!(
        belopp(&omvänd().andel(-33_333)),
        [(5410, 33_333), (2645, 8_333), (2614, -8_333)]
    );
}

#[test]
fn bara_det_som_överstiger_det_obetalda_av_en_kredit_bokförs() {
    // Kredit 400 kr av en obetald faktura med 1 000 kr kvar: inget bokförs.
    assert_eq!(kreditens_bokförda_del(40_000, 100_000), 0);
    // 300 kr kvar: 100 kr av krediten är redan betalt och bokförs.
    assert_eq!(kreditens_bokförda_del(40_000, 30_000), 10_000);
    // Betald eller tillgodo: hela krediten.
    assert_eq!(kreditens_bokförda_del(40_000, 0), 40_000);
    assert_eq!(kreditens_bokförda_del(40_000, -5_000), 40_000);
}

#[test]
fn en_kredit_i_året_av_en_faktura_som_väntar_på_t2_bokförs_bara_utöver_det_obetalda_vid_årets_slut()
{
    use maya_domain::kontantmetoden::kreditens_bokförda_del_före_t2 as del;
    // 1 000 kr obetalt vid årets slut, helt betalt efter: en kredit på 400 kr ryms i det som T2
    // tar med och bokförs inte.
    assert_eq!(del(40_000, 0, 100_000), 0);
    // 600 kr betalt efter årets slut, kredit av hela fakturan: T2 tar inte längre med den, och de
    // 600 kr står redan på kontot som tillgodohavandet.
    assert_eq!(del(100_000, 40_000, 100_000), 0);
    // Ingen betalning, kredit av hela: inget står på kontot och inget bokförs.
    assert_eq!(del(100_000, 100_000, 100_000), 0);
}
