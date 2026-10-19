//! Mayas regler utan IO. Kompilerar också till WASM, så att formulären validerar med samma
//! regler som servern (ADR 0003).

pub mod arbetsdagar;
pub mod att_gora;
pub mod backup;
pub mod belopp;
pub mod betalning;
pub mod betalsatt;
pub mod csv;
pub mod drift;
pub mod epost;
pub mod faktura;
pub mod fakturauppgifter;
pub mod fordelning;
pub mod foretag;
pub mod ingaende_balans;
pub mod kontering;
pub mod kontoplan;
pub mod kontourval;
pub mod kund;
pub mod kundfaktura;
pub mod kundforlust;
pub mod leverantor;
pub mod leverantorsfaktura;
pub mod luhn;
pub mod orgnr;
pub mod rakenskapsar;
pub mod rapport;
pub mod rattelse;
pub mod reskontra;
pub mod sok;
pub mod systemdokumentation;
pub mod tid;
pub mod underlag;
pub mod utskick;
pub mod verifikat;
