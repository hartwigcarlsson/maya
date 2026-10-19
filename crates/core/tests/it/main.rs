//! Integrationstesterna i en enda testbinär. Varje binär länkar in hela beroendeträdet, så
//! en binär per fil gjorde att bygget tog längre tid än körningen. Ett nytt test läggs i en
//! fil här bredvid, med en `mod` nedan.
//!
//! `tests/klocka.rs` är en egen binär, eftersom testet ändrar processens `TZ`.

mod anvandare;
mod aterstallning;
mod atkomst;
mod att_gora;
mod att_gora_fakturor;
mod att_gora_folj_upp;
mod automatkonteringar;
mod backup;
mod backup_s3;
mod behandlingshistorik;
mod betalning;
mod config;
mod databas;
mod drift;
mod externa_kundfakturor;
mod fakturauppgifter;
mod foretag;
mod foretag_radering;
mod foretagsinstallningar;
mod grundbok_huvudbok;
mod ingaende_balans;
mod inloggning;
mod installation;
mod installationslogg;
mod kontantmetoden;
mod kontering;
mod kontoplan;
mod kontrollsumma;
mod kreditfaktura;
mod kundfaktura;
mod kundfaktura_moms;
mod kundforlust;
mod kundregister;
mod larm;
mod leverantorsbetalning;
mod leverantorsfaktura;
mod leverantorskredit;
mod leverantorsregister;
mod migrering;
mod passkeys;
mod rapport;
mod rattelse;
mod reskontra;
mod reskontra_invariant;
mod sie4;
mod sok;
mod sok_fakturor;
mod systemdokumentation;
mod t2;
mod underlag;
mod utskick;
mod utskrift;
mod verifikat;
mod verifikat_las;
mod verifikatutkast;
