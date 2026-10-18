//! Behandlingshistoriken per företag: varje ändring som påverkar postbehandlingen, med tid och
//! aktör (H1, H2). Append-only i två lager (H3): `core` har ingen kodväg som ändrar eller
//! raderar en post, och schemat avvisar `UPDATE` och `DELETE`.

use jiff::Timestamp;
use sqlx::SqliteConnection;

use crate::aktor::{Aktör, AnvändarId, Handling, Kanal};
use crate::atkomst::{self, ÅtkomstFel};
use crate::foretag::FöretagsId;
use crate::{Databas, DatabasFel};

/// Händelsetyperna i behandlingshistoriken. Lägg till nya här.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Händelse {
    FöretagSkapat,
    KontoSkapat,
    KontoNamnbytt,
    KontoAktiverat,
    KontoInaktiverat,
    BasTillägg,
    RäkenskapsårSkapat,
    VerifikatBokfört,
}

impl Händelse {
    pub fn namn(self) -> &'static str {
        match self {
            Händelse::FöretagSkapat => "företag skapat",
            Händelse::KontoSkapat => "konto skapat",
            Händelse::KontoNamnbytt => "konto namnbytt",
            Händelse::KontoAktiverat => "konto aktiverat",
            Händelse::KontoInaktiverat => "konto inaktiverat",
            Händelse::BasTillägg => "BAS-tillägg",
            Händelse::RäkenskapsårSkapat => "räkenskapsår skapat",
            Händelse::VerifikatBokfört => "verifikat bokfört",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Post {
    pub tid: Timestamp,
    pub aktör: Aktör,
    /// Användarens visningsnamn, när aktören är en användare.
    pub användarnamn: Option<String>,
    /// Händelsens typ, t.ex. `företag skapat`.
    pub typ: String,
    /// Det händelsen rör, t.ex. ett organisationsnummer eller ett kontonummer.
    pub objekt: String,
    /// Strukturerad beskrivning av ändringen som JSON.
    pub beskrivning: serde_json::Value,
}

/// Skriver en post i samma transaktion som ändringen. Användningsfallen går genom
/// [`atkomst::Ärende::logga`].
pub(crate) async fn skriv(
    conn: &mut SqliteConnection,
    företag: FöretagsId,
    tid: Timestamp,
    aktör: &Aktör,
    händelse: Händelse,
    objekt: &str,
    beskrivning: serde_json::Value,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO behandlingshistorik
             (foretag_id, tid, kanal, anvandare_id, typ, objekt, beskrivning)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(företag.0)
    .bind(tid.to_string())
    .bind(aktör.kanal.namn())
    .bind(aktör.användare.map(|a| a.0))
    .bind(händelse.namn())
    .bind(objekt)
    .bind(beskrivning.to_string())
    .execute(conn)
    .await?;
    Ok(())
}

/// Företagets behandlingshistorik i den ordning posterna skrevs.
pub async fn läs(
    db: &Databas,
    aktör: &Aktör,
    företag: FöretagsId,
) -> Result<Vec<Post>, ÅtkomstFel> {
    let mut tx = db.pool().begin().await?;
    atkomst::kräv(&mut tx, aktör, företag, Handling::LäsaFöretag).await?;
    let rader = sqlx::query_as(
        "SELECT h.tid, h.kanal, h.anvandare_id, u.visningsnamn, h.typ, h.objekt, h.beskrivning
         FROM behandlingshistorik h LEFT JOIN anvandare u ON u.id = h.anvandare_id
         WHERE h.foretag_id = ? ORDER BY h.id",
    )
    .bind(företag.0)
    .fetch_all(&mut *tx)
    .await?;
    Ok(poster(rader)?)
}

/// Posterna som rör ett konto: när det kom till (företaget skapat med en kopia av BAS, eller ett
/// BAS-tillägg) och varje ändring av det.
pub(crate) async fn läs_konto(
    conn: &mut SqliteConnection,
    företag: FöretagsId,
    nummer: u16,
) -> Result<Vec<Post>, DatabasFel> {
    let rader = sqlx::query_as(
        "SELECT h.tid, h.kanal, h.anvandare_id, u.visningsnamn, h.typ, h.objekt, h.beskrivning
         FROM behandlingshistorik h
         LEFT JOIN anvandare u ON u.id = h.anvandare_id
         JOIN konto k ON k.foretag_id = h.foretag_id AND k.nummer = ?2
         WHERE h.foretag_id = ?1
           AND ((h.typ LIKE 'konto %' AND h.objekt = CAST(?2 AS TEXT))
             OR (h.typ = 'BAS-tillägg'
                 AND ?2 IN (SELECT value FROM json_each(h.beskrivning, '$.konton')))
             OR (h.typ = 'företag skapat' AND k.ursprung = 'bas' AND k.bas_tillagg = 0))
         ORDER BY h.id",
    )
    .bind(företag.0)
    .bind(nummer)
    .fetch_all(conn)
    .await?;
    poster(rader)
}

/// Posterna som rör ett verifikat, i ordning: de vars beskrivning har `"verifikat": id`.
pub(crate) async fn läs_om_verifikat(
    conn: &mut SqliteConnection,
    företag: FöretagsId,
    verifikat: i64,
) -> Result<Vec<Post>, DatabasFel> {
    let rader = sqlx::query_as(
        "SELECT h.tid, h.kanal, h.anvandare_id, u.visningsnamn, h.typ, h.objekt, h.beskrivning
         FROM behandlingshistorik h LEFT JOIN anvandare u ON u.id = h.anvandare_id
         WHERE h.foretag_id = ? AND h.beskrivning ->> '$.verifikat' = ? ORDER BY h.id",
    )
    .bind(företag.0)
    .bind(verifikat)
    .fetch_all(conn)
    .await?;
    poster(rader)
}

type Rad = (
    String,
    String,
    Option<i64>,
    Option<String>,
    String,
    String,
    String,
);

fn poster(rader: Vec<Rad>) -> Result<Vec<Post>, DatabasFel> {
    let fel = |vad: &str| DatabasFel::Korrupt(format!("behandlingshistoriken: {vad}"));
    rader
        .into_iter()
        .map(
            |(tid, kanal, användare, användarnamn, typ, objekt, beskrivning)| {
                Ok(Post {
                    tid: tid.parse().map_err(|_| fel("tid"))?,
                    aktör: Aktör {
                        kanal: Kanal::från_namn(&kanal).ok_or_else(|| fel("kanal"))?,
                        användare: användare.map(AnvändarId),
                    },
                    användarnamn,
                    typ,
                    objekt,
                    beskrivning: serde_json::from_str(&beskrivning)
                        .map_err(|_| fel("beskrivning"))?,
                })
            },
        )
        .collect()
}
