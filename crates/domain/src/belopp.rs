//! Belopp i SEK, lagrade som heltal i öre.

use std::fmt;

/// Ett belopp som inte är negativt, i öre. Debet och kredit är var sitt belopp.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Belopp(i64);

impl Belopp {
    pub const NOLL: Belopp = Belopp(0);

    pub fn från_öre(öre: i64) -> Self {
        Belopp(öre)
    }

    pub fn öre(self) -> i64 {
        self.0
    }

    /// Tolkar kronor som användaren skrev dem: siffror med mellanslag som tusentalsavgränsare
    /// och högst två decimaler efter komma eller punkt. Negativa belopp nekas; en rad är debet
    /// eller kredit.
    pub fn tolka(text: &str) -> Option<Self> {
        let text: String = text
            .chars()
            .filter(|c| !matches!(c, ' ' | '\u{a0}' | '\u{202f}'))
            .collect();
        let (kronor, decimaler) = match text.split_once([',', '.']) {
            Some((kronor, decimaler)) => (kronor, decimaler),
            None => (text.as_str(), ""),
        };
        let siffror = |s: &str| s.chars().all(|c| c.is_ascii_digit());
        if kronor.is_empty() || !siffror(kronor) || !siffror(decimaler) || decimaler.len() > 2 {
            return None;
        }
        let kronor: i64 = kronor.parse().ok()?;
        let öre: i64 = format!("{decimaler:0<2}").parse().ok()?;
        kronor.checked_mul(100)?.checked_add(öre).map(Belopp)
    }

    pub fn checked_add(self, other: Belopp) -> Option<Belopp> {
        self.0.checked_add(other.0).map(Belopp)
    }
}

impl fmt::Display for Belopp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kronor = (self.0 / 100).to_string();
        let mut med_avgränsare = String::new();
        for (i, c) in kronor.chars().enumerate() {
            if i > 0 && (kronor.len() - i).is_multiple_of(3) {
                med_avgränsare.push(' ');
            }
            med_avgränsare.push(c);
        }
        write!(f, "{med_avgränsare},{:02}", self.0 % 100)
    }
}
