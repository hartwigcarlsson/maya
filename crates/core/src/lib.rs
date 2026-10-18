//! Enda vägen in till Mayas data (ADR 0003). Webb, CLI och bakgrundsjobb är tunna adaptrar
//! som anropar användningsfallen här och aldrig kör sqlx själva.

pub mod aktor;
pub mod atkomst;
pub mod behandlingshistorik;
pub mod config;
mod databas;
pub mod foretag;
pub mod inloggning;
pub mod installation;
pub mod installationslogg;
mod klocka;
pub mod kontoplan;
#[cfg(feature = "test-support")]
pub mod testhjalp;
pub mod utskrift;
pub mod verifikat;

#[cfg(feature = "test-support")]
pub use databas::TillfälligDatabas;
pub use databas::{Databas, DatabasFel, SCHEMAEPOK};
pub use klocka::Klocka;
