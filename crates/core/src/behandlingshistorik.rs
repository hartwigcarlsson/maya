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
    FöretagÄndrat,
    RäkenskapsårSkapat,
    RäkenskapsårÄndrat,
    BokföringsmetodBytt,
    MomsperiodBytt,
    VerifikatBokfört,
}

impl Händelse {
    pub fn namn(self) -> &'static str {
        match self {
            Händelse::FöretagSkapat => "företag skapat",
            Händelse::FöretagÄndrat => "företag ändrat",
            Händelse::RäkenskapsårSkapat => "räkenskapsår skapat",
            Händelse::RäkenskapsårÄndrat => "räkenskapsår ändrat",
            Händelse::BokföringsmetodBytt => "bokföringsmetod bytt",
            Händelse::MomsperiodBytt => "momsperiod bytt",
            Händelse::VerifikatBokfört => "verifikat bokfört",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Post {
    pub tid: Timestamp,
    pub aktör: Aktör,
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
        "SELECT tid, kanal, anvandare_id, typ, objekt, beskrivning
         FROM behandlingshistorik WHERE foretag_id = ? ORDER BY id",
    )
    .bind(företag.0)
    .fetch_all(&mut *tx)
    .await?;
    Ok(tolka(rader)?)
}

/// Posterna som rör ett verifikat, i ordning: de vars beskrivning har `"verifikat": id`.
pub(crate) async fn läs_om_verifikat(
    conn: &mut SqliteConnection,
    företag: FöretagsId,
    verifikat: i64,
) -> Result<Vec<Post>, DatabasFel> {
    let rader = sqlx::query_as(
        "SELECT tid, kanal, anvandare_id, typ, objekt, beskrivning
         FROM behandlingshistorik
         WHERE foretag_id = ? AND beskrivning ->> '$.verifikat' = ? ORDER BY id",
    )
    .bind(företag.0)
    .bind(verifikat)
    .fetch_all(conn)
    .await?;
    tolka(rader)
}

type PostRad = (String, String, Option<i64>, String, String, String);

fn tolka(rader: Vec<PostRad>) -> Result<Vec<Post>, DatabasFel> {
    let fel = |vad: &str| DatabasFel::Korrupt(format!("behandlingshistoriken: {vad}"));
    rader
        .into_iter()
        .map(|(tid, kanal, användare, typ, objekt, beskrivning)| {
            Ok(Post {
                tid: tid.parse().map_err(|_| fel("tid"))?,
                aktör: Aktör {
                    kanal: Kanal::från_namn(&kanal).ok_or_else(|| fel("kanal"))?,
                    användare: användare.map(AnvändarId),
                },
                typ,
                objekt,
                beskrivning: serde_json::from_str(&beskrivning).map_err(|_| fel("beskrivning"))?,
            })
        })
        .collect()
}
