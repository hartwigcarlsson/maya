//! Integrationstesterna i en enda testbinär. Varje binär länkar in hela beroendeträdet, så
//! en binär per fil gjorde att bygget tog längre tid än körningen. Ett nytt test läggs i en
//! fil här bredvid, med en `mod` nedan.

mod assets;
mod atkomst;
mod att_gora;
mod att_gora_fakturor;
mod att_gora_folj_upp;
mod behandlingshistorik;
mod betalning;
mod bokforing;
mod client_ip;
mod drift;
mod externa_kundfakturor;
mod fakturauppgifter;
mod foretag;
mod ingaende_balans;
mod inloggning;
mod installningar;
mod kontering;
mod konto;
mod kontoplan;
mod kreditfaktura;
mod kunder;
mod kundfakturor;
mod kundforlust;
mod leverantorer;
mod leverantorsbetalning;
mod leverantorsfakturor;
mod leverantorskredit;
mod rapporter;
mod rattelse;
mod reskontra;
mod router;
mod sidodata;
mod sie4;
mod sok;
mod stilmall;
mod systemdokumentation;
mod t2;
mod underlag;
mod utskick;
mod utskrift;
mod verifikat;
