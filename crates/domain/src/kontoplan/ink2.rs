//! INK2-kopplingstabellen: kontointervall → fält och rad i INK2R, tolkad vid bygget ur BAS fria
//! kopplingstabell. Tabellen versioneras per blankettutgåva ([`UTGÅVA`]), oberoende av
//! BAS-året, och är densamma för alla företag. Egna konton får sin rad ur intervallen.

use super::Kontonummer;

/// En rad i INK2R, med blankettens benämning och rubrikerna den står under.
#[derive(Debug, PartialEq, Eq)]
pub struct Ink2Rad {
    rad: &'static str,
    benämning: &'static str,
    rubriker: &'static [&'static str],
}

impl Ink2Rad {
    /// Radnumret, till exempel "2.26".
    pub fn rad(&self) -> &'static str {
        self.rad
    }

    pub fn benämning(&self) -> &'static str {
        self.benämning
    }

    /// Rubrikerna över raden, från sidan (Balansräkning, Resultaträkning) och nedåt.
    pub fn rubriker(&self) -> &'static [&'static str] {
        self.rubriker
    }
}

/// Ett fält i INK2R, med fältkoden som SIE4-exportens `#SRU` använder.
#[derive(Debug, PartialEq, Eq)]
pub struct Fält {
    kod: u16,
    rad: u8,
}

impl Fält {
    pub fn kod(&self) -> u16 {
        self.kod
    }

    pub fn rad(&self) -> &'static Ink2Rad {
        &RADER[self.rad as usize]
    }
}

/// Ett kontos koppling. De flesta konton har ett fält; för andra avgör saldots tecken om
/// plus- eller minusfältet används (t.ex. 899x på 3.26 eller 3.27).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Koppling {
    plus: &'static Fält,
    minus: &'static Fält,
}

impl Koppling {
    /// Fältet när nettot är positivt (`plus`) respektive negativt, i tabellens mening ("Om netto
    /// +", "+ 899x"). Ett konto som bara står med ett av dem har det fältet åt båda håll.
    pub fn fält(&self, plus: bool) -> &'static Fält {
        if plus { self.plus } else { self.minus }
    }
}

include!(concat!(env!("OUT_DIR"), "/ink2.rs"));

/// INK2R:s rader i blankettens ordning.
pub fn rader() -> &'static [Ink2Rad] {
    RADER
}

/// Kopplingen för kontonumret, om tabellen täcker det.
pub fn koppling(nummer: Kontonummer) -> Option<Koppling> {
    let nr = nummer.get();
    let i = INTERVALL.partition_point(|&(_, till, _, _)| till < nr);
    let &(från, _, plus, minus) = INTERVALL.get(i)?;
    (från <= nr).then(|| Koppling {
        plus: &FÄLT[plus as usize],
        minus: &FÄLT[minus as usize],
    })
}
