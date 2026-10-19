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

    /// Beloppet som det står i en debet- eller kreditkolumn: tomt för noll.
    pub fn eller_tomt(self) -> String {
        if self == Belopp::NOLL {
            String::new()
        } else {
            self.to_string()
        }
    }
}

impl fmt::Display for Belopp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        skriv_kronor(f, self.0.unsigned_abs())
    }
}

/// Tolkar ett belopp på en faktura, som kan vara negativt (en kreditfaktura): samma regler som
/// [`Belopp::tolka`] med ett minustecken (`-` eller `−`) först. Svaret är i öre med tecken.
pub fn tolka_med_tecken(text: &str) -> Option<i64> {
    let text = text.trim();
    match text.strip_prefix('-').or_else(|| text.strip_prefix('−')) {
        Some(rest) => Belopp::tolka(rest.trim_start()).map(|b| -b.0),
        None => Belopp::tolka(text).map(Belopp::öre),
    }
}

/// Ett belopp i öre med tecken som det står i ett formulärfält: utan tusentalsavgränsare, med
/// decimalkomma och utan `,00`, så att [`tolka_med_tecken`] läser tillbaka det.
pub fn formulärtext(öre: i64) -> String {
    let tecken = if öre < 0 { "-" } else { "" };
    let (kronor, rest) = (öre.unsigned_abs() / 100, öre.unsigned_abs() % 100);
    if rest == 0 {
        format!("{tecken}{kronor}")
    } else {
        format!("{tecken}{kronor},{rest:02}")
    }
}

/// Ett kontos saldo i öre, med debet som positivt och kredit som negativt.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Saldo(i64);

impl Saldo {
    pub const NOLL: Saldo = Saldo(0);

    pub fn från_öre(öre: i64) -> Self {
        Saldo(öre)
    }

    /// Saldot av ett belopp i debet och ett i kredit.
    pub fn av(debet: Belopp, kredit: Belopp) -> Self {
        Saldo(debet.0 - kredit.0)
    }

    pub fn öre(self) -> i64 {
        self.0
    }

    /// Beloppet i debet: saldot om det är positivt, annars noll.
    pub fn debet(self) -> Belopp {
        Belopp(self.0.max(0))
    }

    /// Beloppet i kredit: saldot med vänt tecken om det är negativt, annars noll.
    pub fn kredit(self) -> Belopp {
        Belopp((-self.0).max(0))
    }
}

impl std::ops::Add for Saldo {
    type Output = Saldo;
    fn add(self, other: Saldo) -> Saldo {
        Saldo(self.0 + other.0)
    }
}

impl std::ops::AddAssign for Saldo {
    fn add_assign(&mut self, other: Saldo) {
        self.0 += other.0;
    }
}

impl std::iter::Sum for Saldo {
    fn sum<I: Iterator<Item = Saldo>>(iter: I) -> Saldo {
        Saldo(iter.map(|s| s.0).sum())
    }
}

impl fmt::Display for Saldo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0 < 0 {
            f.write_str("-")?;
        }
        skriv_kronor(f, self.0.unsigned_abs())
    }
}

/// Kronor som Maya visar dem överallt: mellanslag mellan tusentalen och decimalkomma,
/// "1 234 567,78"; ett negativt saldo får ett vanligt minus framför.
fn skriv_kronor(f: &mut fmt::Formatter<'_>, öre: u64) -> fmt::Result {
    let kronor = (öre / 100).to_string();
    let mut med_avgränsare = String::new();
    for (i, c) in kronor.chars().enumerate() {
        if i > 0 && (kronor.len() - i).is_multiple_of(3) {
            med_avgränsare.push(' ');
        }
        med_avgränsare.push(c);
    }
    write!(f, "{med_avgränsare},{:02}", öre % 100)
}
