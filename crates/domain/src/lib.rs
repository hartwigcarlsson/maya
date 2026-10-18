//! Mayas regler utan IO. Kompilerar också till WASM, så att formulären validerar med samma
//! regler som servern (ADR 0003).

pub mod arbetsdagar;
pub mod att_gora;
pub mod backup;
pub mod belopp;
pub mod betalsatt;
pub mod csv;
pub mod drift;
pub mod epost;
pub mod foretag;
pub mod ingaende_balans;
pub mod kontoplan;
pub mod kontourval;
pub mod luhn;
pub mod orgnr;
pub mod rakenskapsar;
pub mod rapport;
pub mod rattelse;
pub mod sok;
pub mod systemdokumentation;
pub mod tid;
pub mod underlag;
pub mod verifikat;
