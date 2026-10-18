//! Urvalet av konton i huvudboken: enskilda konton och intervall, skilda med komma, t.ex.
//! `1930, 3000–3999`. Ett tomt urval omfattar alla konton.

use std::fmt;

use crate::kontoplan::Kontonummer;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Kontourval {
    /// Intervallen, med start ≤ slut. Tomt = alla konton.
    intervall: Vec<(Kontonummer, Kontonummer)>,
}

impl Kontourval {
    /// Alla konton.
    pub fn alla() -> Self {
        Kontourval::default()
    }

    /// Tolkar urvalet som användaren skrev det. `-` och `–` skiljer ett intervall; ett
    /// baklänges intervall vänds. `None` om en del inte är ett kontonummer.
    pub fn tolka(text: &str) -> Option<Self> {
        if text.trim().is_empty() {
            return Some(Kontourval::alla());
        }
        let nummer = |t: &str| t.trim().parse::<u16>().ok().and_then(Kontonummer::new);
        let intervall = text
            .split(',')
            .map(|del| match del.split_once(['-', '–']) {
                Some((a, b)) => {
                    let (a, b) = (nummer(a)?, nummer(b)?);
                    Some((a.min(b), a.max(b)))
                }
                None => nummer(del).map(|n| (n, n)),
            })
            .collect::<Option<_>>()?;
        Some(Kontourval { intervall })
    }

    /// Om kontot ingår i urvalet.
    pub fn omfattar(&self, konto: Kontonummer) -> bool {
        self.intervall.is_empty()
            || self
                .intervall
                .iter()
                .any(|(a, b)| *a <= konto && konto <= *b)
    }
}

/// Urvalet som det skrivs, med tankstreck i intervallen; tomt för alla konton.
impl fmt::Display for Kontourval {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, (a, b)) in self.intervall.iter().enumerate() {
            if i > 0 {
                f.write_str(", ")?;
            }
            if a == b {
                write!(f, "{a}")?;
            } else {
                write!(f, "{a}–{b}")?;
            }
        }
        Ok(())
    }
}
