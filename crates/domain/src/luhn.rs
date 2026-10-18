//! Luhn-algoritmen (modulus 10) som organisationsnummer, personnummer och OCR-referenser
//! räknar sin kontrollsiffra med.

/// Kontrollsiffran för `siffror`: varannan siffra från höger, med början i den sista,
/// dubbleras och siffersumman tas mod 10.
pub fn kontrollsiffra(siffror: &[u32]) -> u32 {
    let summa: u32 = siffror
        .iter()
        .rev()
        .enumerate()
        .map(|(i, &d)| {
            let v = if i % 2 == 0 { d * 2 } else { d };
            v / 10 + v % 10
        })
        .sum();
    (10 - summa % 10) % 10
}
