//! Codepage 437, den enda teckenuppsättningen SIE tillåter (`#FORMAT PC8`).

/// Tecknen för bytes 0x80–0xFF.
const ÖVRE: &str = "ÇüéâäàåçêëèïîìÄÅÉæÆôöòûùÿÖÜ¢£¥₧ƒáíóúñÑªº¿⌐¬½¼¡«»░▒▓│┤╡╢╖╕╣║╗╝╜╛┐\
                    └┴┬├─┼╞╟╚╔╩╦╠═╬╧╨╤╥╙╘╒╓╫╪┘┌█▄▌▐▀αßΓπΣσµτΦΘΩδ∞φε∩≡±≥≤⌠⌡÷≈°∙·√ⁿ²■\u{a0}";

/// Kodar om en text till CP437. Kontrolltecken (som radbrytning och tabb) blir blanksteg, några
/// vanliga typografiska tecken blir sina ASCII-motsvarigheter, och övriga tecken som saknas i
/// CP437 blir `?`.
pub(crate) fn koda(text: &str) -> Vec<u8> {
    text.chars().map(byte).collect()
}

fn byte(c: char) -> u8 {
    match c {
        '\u{20}'..='\u{7e}' => c as u8,
        c if c.is_control() => b' ',
        '–' | '—' | '‐' | '‑' | '−' => b'-',
        '‘' | '’' | '‚' | '′' => b'\'',
        '“' | '”' | '„' | '″' => b'"',
        '…' => b'.',
        c => ÖVRE
            .chars()
            .position(|ö| ö == c)
            .map_or(b'?', |i| 0x80 + i as u8),
    }
}

/// Avkodar CP437. Alla bytes har ett tecken; 0x00–0x7F läses som ASCII.
pub(crate) fn avkoda(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&b| {
            if b < 0x80 {
                b as char
            } else {
                ÖVRE.chars().nth(usize::from(b - 0x80)).unwrap_or('?')
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn svenska_tecken_har_sina_cp437_koder() {
        assert_eq!(ÖVRE.chars().count(), 128);
        assert_eq!(koda("åäöÅÄÖé"), [0x86, 0x84, 0x94, 0x8f, 0x8e, 0x99, 0x82]);
        assert_eq!(
            avkoda(&[0x86, 0x84, 0x94, 0x8f, 0x8e, 0x99, 0x82]),
            "åäöÅÄÖé"
        );
    }

    #[test]
    fn tecken_utanför_cp437_ersätts() {
        assert_eq!(koda("a–b\n€\t“c”"), b"a-b ? \"c\"");
    }
}
