//! Den proportionella fördelningen (kontantmetoden, delkredit, kundförlust och T2): delarna
//! summerar exakt till beloppet och varje del avviker högst ett öre från sin exakta andel.
//! Egenskaperna prövas med en enkel slumpgenerator över många fall.

use maya_domain::fordelning::fördela;

#[test]
fn ett_exempel_fördelas_efter_andelarna_och_summerar_exakt() {
    // 100 kr på 3001 (8 000) och 2611 (2 000): 80 % och 20 %.
    assert_eq!(fördela(10_000, &[8_000, 2_000]), vec![8_000, 2_000]);
    // 1 öre på tre lika delar: den första får öret.
    assert_eq!(fördela(1, &[1, 1, 1]), vec![1, 0, 0]);
    // 100 öre på tre lika delar: 33,33… var, resten till den första.
    assert_eq!(fördela(100, &[1, 1, 1]), vec![34, 33, 33]);
    // Ett negativt belopp fördelas med samma tecken.
    assert_eq!(fördela(-100, &[1, 1, 1]), vec![-34, -33, -33]);
}

#[test]
fn den_största_resten_får_öret() {
    // 10 öre på 1:2 ger 3,33 och 6,67: den andra delens rest är störst.
    assert_eq!(fördela(10, &[1, 2]), vec![3, 7]);
}

#[test]
fn utan_andelar_eller_med_summan_noll_blir_allt_noll() {
    assert_eq!(fördela(500, &[]), Vec::<i64>::new());
    assert_eq!(fördela(500, &[0, 0]), vec![0, 0]);
    assert_eq!(fördela(0, &[3, 4]), vec![0, 0]);
}

#[test]
fn negativa_andelar_ger_delar_med_motsatt_tecken() {
    // En faktura med en rabattrad: 120 och -20 på totalen 100.
    let delar = fördela(50, &[120, -20]);
    assert_eq!(delar, vec![60, -10]);
}

/// xorshift64*, så att testet inte behöver något beroende.
struct Slump(u64);

impl Slump {
    fn nästa(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn mellan(&mut self, lägst: i64, högst: i64) -> i64 {
        lägst + (self.nästa() % (högst - lägst + 1) as u64) as i64
    }
}

#[test]
fn delarna_summerar_exakt_och_avviker_högst_ett_öre_från_sin_exakta_andel() {
    let mut slump = Slump(0x9e37_79b9_7f4a_7c15);
    for _ in 0..20_000 {
        let antal = slump.mellan(1, 8) as usize;
        let vikter: Vec<i64> = (0..antal)
            .map(|_| slump.mellan(-50_000, 5_000_000))
            .collect();
        let belopp = slump.mellan(-10_000_000, 10_000_000);
        let delar = fördela(belopp, &vikter);
        assert_eq!(delar.len(), vikter.len());
        let summa_vikter: i128 = vikter.iter().map(|&v| i128::from(v)).sum();
        if summa_vikter == 0 {
            assert!(delar.iter().all(|&d| d == 0), "{belopp} {vikter:?} {delar:?}");
            continue;
        }
        assert_eq!(delar.iter().sum::<i64>(), belopp, "{belopp} {vikter:?} {delar:?}");
        for (del, vikt) in delar.iter().zip(&vikter) {
            // |del − belopp·vikt/summa| ≤ 1 öre, utan avrundning: |del·summa − belopp·vikt| ≤ |summa|.
            let avvikelse = i128::from(*del) * summa_vikter - i128::from(belopp) * i128::from(*vikt);
            assert!(
                avvikelse.abs() <= summa_vikter.abs(),
                "{belopp} {vikter:?} {delar:?}"
            );
        }
    }
}

#[test]
fn med_positiva_andelar_följer_varje_del_beloppets_tecken() {
    let mut slump = Slump(42);
    for _ in 0..5_000 {
        let vikter: Vec<i64> = (0..slump.mellan(1, 6)).map(|_| slump.mellan(0, 100_000)).collect();
        let belopp = slump.mellan(-1_000_000, 1_000_000);
        for del in fördela(belopp, &vikter) {
            assert!(del == 0 || del.signum() == belopp.signum(), "{belopp} {vikter:?}");
        }
    }
}
