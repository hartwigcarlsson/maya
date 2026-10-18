//! SIE4-formatet (SIE filformat utgåva 4C): skrivare och läsare utan databas. `maya-core`
//! bygger en [`Sie4`] ur bokföringen, skriver den med [`skriv`], läser tillbaka den med [`läs`]
//! och stämmer av mot databasen (A4). Etapp 5:s import bygger på samma läsare.
//!
//! Filen är radorienterad: en post per rad, `#ETIKETT` och fält skilda av blanksteg, och
//! verifikationens `#TRANS` inom `{` och `}` på egna rader. Skrivaren
//!
//! - kodar i Codepage 437 (`#FORMAT PC8`) med CRLF som radslut, som SIE-gruppens exempelfil,
//! - citerar alltid fritext (namn, texter, signaturer, program) och skriver tal, datum och koder
//!   utan citattecken; `"` och `\` i en text skrivs `\"` och `\\`,
//! - skriver belopp i kronor med punkt och två decimaler, kredit negativt, och datum `ÅÅÅÅMMDD`,
//! - lägger `#KSUMMA` direkt efter `#FLAGGA` och `#KSUMMA <summa>` sist. Summan är CRC-32
//!   (polynom `EDB88320`, start `FFFFFFFF`, slutlig invertering) över etiketterna och fältens
//!   innehåll i CP437, utan blanksteg, citattecken och klamrar, för posterna mellan de två, och
//!   skrivs som ett tal utan tecken. `#FLAGGA` står utanför, eftersom mottagaren sätter den.
//!
//! Belopp är i öre och kontonummer som i filen, så att läsaren också tar emot andra systems
//! filer.

use jiff::civil::Date;

mod cp437;
mod las;
mod skriv;

pub use las::{LäsFel, läs};
pub use skriv::skriv;

/// En 4E-fil: identifikation, kontoplan, saldon och verifikationer. Posterna skrivs i
/// vektorernas ordning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sie4 {
    /// `#PROGRAM`: namn och version.
    pub program: String,
    pub version: String,
    /// `#GEN`: datum och signatur.
    pub genererad: Date,
    pub genererad_av: Option<String>,
    /// `#ORGNR`, med bindestreck efter sjätte siffran.
    pub orgnr: Option<String>,
    /// `#FTYP`, t.ex. `AB`.
    pub företagsform: Option<String>,
    /// `#FNAMN`.
    pub företagsnamn: String,
    /// `#KPTYP`, t.ex. `BAS2026`.
    pub kontoplanstyp: Option<String>,
    /// `#RAR`.
    pub räkenskapsår: Vec<Räkenskapsår>,
    /// `#KONTO`, med `#SRU` direkt efter.
    pub konton: Vec<Konto>,
    /// `#IB`, `#UB` och `#RES`.
    pub ingående: Vec<Saldopost>,
    pub utgående: Vec<Saldopost>,
    pub resultat: Vec<Saldopost>,
    /// `#VER` med `#TRANS`.
    pub verifikat: Vec<Verifikation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Räkenskapsår {
    /// 0 för exportåret, -1 för året före.
    pub årsnr: i32,
    pub start: Date,
    pub slut: Date,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Konto {
    pub nummer: u32,
    pub namn: String,
    /// SRU-koden, fältet i INK2 som kontot förs till.
    pub sru: Option<u32>,
}

/// Ett saldo för ett konto och år, i öre med debet positivt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Saldopost {
    pub årsnr: i32,
    pub konto: u32,
    pub öre: i64,
}

/// En verifikation. `regdatum` är dagen då den registrerades, `sign` vem som gjorde det.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verifikation {
    pub serie: String,
    pub nummer: u32,
    pub datum: Date,
    pub text: String,
    pub regdatum: Option<Date>,
    pub sign: Option<String>,
    pub transaktioner: Vec<Transaktion>,
}

/// En rad i en verifikation, i öre med debet positivt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transaktion {
    pub konto: u32,
    pub öre: i64,
    pub text: Option<String>,
}
