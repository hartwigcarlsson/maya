//! Integrationstesterna i en enda testbinär. Varje binär länkar in hela beroendeträdet, så
//! en binär per fil gjorde att bygget tog längre tid än körningen. Ett nytt test läggs i en
//! fil här bredvid, med en `mod` nedan.

mod arbetsdagar;
mod att_gora;
mod automatkontering;
mod backup;
mod bas;
mod belopp;
mod betalning;
mod betalsatt;
mod csv;
mod extern_kundfaktura;
mod faktura;
mod fakturauppgifter;
mod flodenas_konton;
mod fordelning;
mod inaktivering;
mod ingaende_balans;
mod ink2;
mod kontantmetoden;
mod kontering;
mod kontonummer;
mod kontourval;
mod kreditfaktura;
mod kund;
mod kundfaktura;
mod kundforlust;
mod leverantor;
mod leverantorsfaktura;
mod luhn;
mod nytt_foretag;
mod nytt_konto;
mod orgnr;
mod rakenskapsar;
mod reskontra;
mod systemdokumentation;
mod underlag;
mod utskick;
mod verifikat;
