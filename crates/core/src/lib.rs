//! Enda vägen in till Mayas data (ADR 0003). Webb, CLI och bakgrundsjobb är tunna adaptrar
//! som anropar användningsfallen här och aldrig kör sqlx själva.

pub mod aktor;
pub mod config;
mod databas;
pub mod inloggning;
pub mod installation;
pub mod installationslogg;
mod klocka;

#[cfg(feature = "test-support")]
pub use databas::TillfälligDatabas;
pub use databas::{Databas, DatabasFel, SCHEMAEPOK};
pub use klocka::Klocka;
