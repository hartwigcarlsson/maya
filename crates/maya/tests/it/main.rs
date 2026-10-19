//! Integrationstesterna i en enda testbinär. Varje binär länkar in hela beroendeträdet, så
//! en binär per fil gjorde att bygget tog längre tid än körningen. Ett nytt test läggs i en
//! fil här bredvid, med en `mod` nedan.

mod backup;
mod domain;
mod hydrering;
mod klientnavigering;
mod restore;
mod rok;
mod serve;
mod stod;
mod user;
