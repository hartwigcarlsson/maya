//! Luhn-algoritmen (modulo 10): kontrollsiffran i organisationsnummer, personnummer,
//! bankgiro- och plusgironummer och OCR-referenser.

/// Kontrollsiffran för `siffror`, eller `None` om texten är tom eller har annat än siffror.
pub fn kontrollsiffra(siffror: &str) -> Option<u8> {
    if siffror.is_empty() || !siffror.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let summa: u32 = siffror
        .bytes()
        .rev()
        .enumerate()
        .map(|(i, b)| {
            let d = u32::from(b - b'0');
            let v = if i % 2 == 0 { d * 2 } else { d };
            v / 10 + v % 10
        })
        .sum();
    Some(((10 - summa % 10) % 10) as u8)
}

/// Om den sista siffran i `siffror` är Luhn-kontrollsiffran för de andra.
pub fn stämmer(siffror: &str) -> bool {
    match siffror.as_bytes().split_last() {
        Some((sista, början)) if sista.is_ascii_digit() => std::str::from_utf8(början)
            .ok()
            .and_then(kontrollsiffra)
            .is_some_and(|väntad| väntad == sista - b'0'),
        _ => false,
    }
}
