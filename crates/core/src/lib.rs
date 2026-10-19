//! Enda vägen in till Mayas data (ADR 0003). Webb, CLI och bakgrundsjobb är tunna adaptrar
//! som anropar användningsfallen här och aldrig kör sqlx själva.

pub mod aktor;
pub mod aterstallning;
pub mod atkomst;
pub mod att_gora;
pub mod backup;
pub mod behandlingshistorik;
pub mod betalning;
pub mod bokforing;
pub mod config;
mod databas;
pub mod drift;
pub mod export;
pub mod extern_kundfaktura;
pub mod fakturauppgifter;
pub(crate) mod fordran;
pub mod foretag;
pub mod inloggning;
pub mod installation;
pub mod installationslogg;
mod klocka;
pub mod kontering;
pub mod kontoplan;
pub mod kontrollsumma;
pub mod kund;
pub mod kundfaktura;
pub mod kundforlust;
pub mod leverantor;
pub mod leverantorsfaktura;
pub mod rapport;
pub mod reskontra;
pub mod saldo;
pub mod sie4;
pub mod smtp;
pub mod sok;
pub mod systemdokumentation;
pub mod t2;
#[cfg(feature = "test-support")]
pub mod testhjalp;
pub mod underlag;
pub mod utskick;
pub mod utskrift;
pub mod verifikat;

#[cfg(feature = "test-support")]
pub use databas::TillfälligDatabas;
pub use databas::{Databas, DatabasFel, SCHEMAEPOK, schemaepok_för};
pub use klocka::Klocka;
