//! Kontantmetoden för fakturor (#149): konteringen när fakturans fordran eller skuld inte är
//! bokförd på 1510 eller 2440.
//!
//! Varje faktura vet om dess fordran eller skuld är bokförd. Den är bokförd från utfärdandet
//! eller registreringen under fakturametoden och från T2-verifikatet under kontantmetoden.
//! Bokföringsmetoden på fakturadatumet avgör om utfärdandet bokförs, så ett metodbyte kräver
//! ingen övergångsbokning. När fordran eller skulden inte är bokförd bokförs betalningen, den
//! del av en kredit som överstiger det obetalda och T2:s obetalda belopp mot fakturans intäkt
//! eller kostnad och moms, i proportion till deras andel av fakturans total
//! ([`crate::fordelning::fördela`]). Den fiktiva momsen vid omvänd skattskyldighet ingår inte i
//! totalen och fördelas i samma proportion, lika stor på båda sidor. En kundförlust ger bara
//! status, och en rest som stängs mot öresutjämning bokförs inte.
//!
//! [`Obokförd`] är fakturans kontering under fakturametoden utan reskontrakontot, som andelarna
//! räknas ur.

use crate::belopp::Saldo;
use crate::fordelning::fördela;
use crate::kontoplan::Kontonummer;
use crate::verifikat;

/// Kontona för fiktiv moms vid omvänd skattskyldighet: ingående 2645 och 2647, utgående
/// 2614, 2624 och 2634. De summerar till noll på en faktura.
const FIKTIV_MOMS: [u16; 5] = [2645, 2647, 2614, 2624, 2634];

/// Fakturans kontering när fordran eller skulden inte är bokförd: raderna som motsvarar
/// reskontrakontot (intäkt eller kostnad och moms) och den fiktiva momsen, med belopp i öre,
/// debet positivt.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Obokförd {
    reskontrakonto: u16,
    /// Reskontrakontots belopp under fakturametoden: positivt för en kundfaktura, negativt
    /// för en leverantörsfaktura, och omvänt för en kreditfaktura.
    total: i64,
    rader: Vec<(u16, i64)>,
    fiktiv: Vec<(u16, i64)>,
}

impl Obokförd {
    /// Ur fakturans kontering under fakturametoden (utfärdandet eller registreringen), med
    /// raden på `reskontrakonto` som totalen.
    pub fn av(kontering: &[verifikat::Rad], reskontrakonto: Kontonummer) -> Self {
        let mut o = Obokförd {
            reskontrakonto: reskontrakonto.get(),
            total: 0,
            rader: Vec::new(),
            fiktiv: Vec::new(),
        };
        for r in kontering {
            let öre = r.debet.öre() - r.kredit.öre();
            let nr = r.konto.get();
            if r.konto == reskontrakonto {
                o.total += öre;
            } else if FIKTIV_MOMS.contains(&nr) {
                o.fiktiv.push((nr, öre));
            } else {
                o.rader.push((nr, öre));
            }
        }
        o
    }

    /// Reskontrakontots belopp under fakturametoden: fakturans total, positiv för en kund-
    /// och negativ för en leverantörsfaktura.
    pub fn total(&self) -> i64 {
        self.total
    }

    /// Raderna för `andel` öre av totalen (med totalens tecken): varje rad i proportion till
    /// sin andel av totalen, avrundat så att raderna summerar exakt till `-andel`, och den
    /// fiktiva momsen i samma proportion, avrundad till närmaste öre och lika stor på båda
    /// sidor. Rader som blir noll tas inte med.
    pub fn andel(&self, andel: i64) -> Vec<verifikat::Rad> {
        let vikter: Vec<i64> = self.rader.iter().map(|(_, b)| *b).collect();
        let mut ut: Vec<(u16, i64)> = self
            .rader
            .iter()
            .zip(fördela(-andel, &vikter))
            .map(|((nr, _), b)| (*nr, b))
            .collect();
        let fiktiv: i64 = self.fiktiv.iter().map(|(_, b)| (*b).max(0)).sum();
        if fiktiv != 0 && self.total != 0 {
            // fiktiv * andel / total, avrundat till närmaste öre med halva bort från noll.
            let (täljare, nämnare) = (
                i128::from(fiktiv) * i128::from(andel),
                i128::from(self.total),
            );
            let tecken = täljare.signum() * nämnare.signum();
            let kvot = (2 * täljare.abs() + nämnare.abs()) / (2 * nämnare.abs());
            let del = i64::try_from(tecken * kvot).unwrap_or(0);
            let (plus, minus): (Vec<_>, Vec<_>) = self.fiktiv.iter().partition(|(_, b)| *b > 0);
            for (sida, belopp) in [(plus, del), (minus, -del)] {
                let vikter: Vec<i64> = sida.iter().map(|(_, b)| *b).collect();
                ut.extend(
                    sida.iter()
                        .zip(fördela(belopp, &vikter))
                        .map(|((nr, _), b)| (*nr, b)),
                );
            }
        }
        ut.into_iter()
            .filter(|(_, b)| *b != 0)
            .map(|(nr, b)| rad(nr, b))
            .collect()
    }

    /// Fakturans kontering för `andel` öre av totalen: reskontrakontot med `andel` och
    /// raderna ur [`Obokförd::andel`]. T2 bokför det obetalda beloppet så.
    pub fn kontering(&self, andel: i64) -> Vec<verifikat::Rad> {
        let mut ut = Vec::new();
        if andel != 0 {
            ut.push(rad(self.reskontrakonto, andel));
        }
        ut.extend(self.andel(andel));
        ut
    }
}

/// Den del av en kredit på `kredit` öre (positivt) som bokförs när originalets fordran eller
/// skuld inte är bokförd: det som överstiger det obetalda (`obetalt`, det som återstod av
/// originalet före krediten). Kredit av en obetald del bokförs inte.
pub fn kreditens_bokförda_del(kredit: i64, obetalt: i64) -> i64 {
    (kredit - obetalt.max(0)).clamp(0, kredit.max(0))
}

fn rad(konto: u16, öre: i64) -> verifikat::Rad {
    let saldo = Saldo::från_öre(öre);
    verifikat::Rad {
        konto: Kontonummer::konstant(konto),
        debet: saldo.debet(),
        kredit: saldo.kredit(),
        text: None,
    }
}
