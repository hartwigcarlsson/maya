//! Proportionell fördelning (kontantmetoden, delkredit, kundförlust och T2): ett belopp fördelas
//! på fakturans konton och momssatser i proportion till deras andel av fakturans total, avrundat
//! till öre så att summan stämmer exakt.
//!
//! Avrundningen är största resten: varje del avrundas först nedåt, och de öre som då fattas går
//! till delarna med störst rest, vid lika rest till den som kommer först. Ett negativt belopp
//! fördelas som det positiva med omvänt tecken, så att regeln är symmetrisk kring noll. Delarna
//! summerar alltid exakt till beloppet, och varje del avviker mindre än ett öre från sin exakta
//! andel (egenskapstesterna i `tests/fordelning.rs`).

/// `belopp` öre fördelat på `vikter` (t.ex. radernas netto och momsen per sats), en del per
/// vikt i samma ordning. Summerar vikterna till noll blir varje del noll.
pub fn fördela(belopp: i64, vikter: &[i64]) -> Vec<i64> {
    if belopp < 0 {
        return fördela_positivt(-i128::from(belopp), vikter)
            .into_iter()
            .map(|d| -d)
            .collect();
    }
    fördela_positivt(i128::from(belopp), vikter)
}

fn fördela_positivt(belopp: i128, vikter: &[i64]) -> Vec<i64> {
    let summa: i128 = vikter.iter().map(|&v| i128::from(v)).sum();
    if summa == 0 {
        return vec![0; vikter.len()];
    }
    // Med en negativ summa är andelen densamma med vikterna och summan negerade.
    let (tecken, summa) = if summa < 0 { (-1, -summa) } else { (1, summa) };
    let mut delar: Vec<(i128, i128)> = vikter
        .iter()
        .map(|&v| {
            let täljare = belopp * i128::from(v) * tecken;
            (täljare.div_euclid(summa), täljare.rem_euclid(summa))
        })
        .collect();
    let fattas = belopp - delar.iter().map(|(d, _)| d).sum::<i128>();
    let mut ordning: Vec<usize> = (0..delar.len()).collect();
    ordning.sort_by(|&a, &b| delar[b].1.cmp(&delar[a].1).then(a.cmp(&b)));
    for &i in ordning.iter().take(usize::try_from(fattas).unwrap_or(0)) {
        delar[i].0 += 1;
    }
    delar
        .into_iter()
        .map(|(d, _)| i64::try_from(d).unwrap_or(if d < 0 { i64::MIN } else { i64::MAX }))
        .collect()
}
