//! Enda vägen in till Mayas data (ADR 0003). Webb, CLI och bakgrundsjobb är tunna adaptrar
//! som anropar användningsfallen här och aldrig kör sqlx själva.

pub mod aktor;
pub mod aterstallning;
pub mod atkomst;
pub mod att_gora;
pub mod backup;
pub mod behandlingshistorik;
pub mod bokforing;
pub mod config;
mod databas;
pub mod drift;
pub mod export;
pub mod foretag;
pub mod inloggning;
pub mod installation;
pub mod installationslogg;
mod klocka;
pub mod kontoplan;
pub mod kontrollsumma;
pub mod kund;
pub mod rapport;
pub mod saldo;
pub mod sie4;
mod smtp;
pub mod systemdokumentation;
#[cfg(feature = "test-support")]
pub mod testhjalp;
pub mod underlag;
pub mod utskrift;
pub mod verifikat;

#[cfg(feature = "test-support")]
pub use databas::TillfälligDatabas;
pub use databas::{Databas, DatabasFel, SCHEMAEPOK, schemaepok_för};
pub use klocka::Klocka;
