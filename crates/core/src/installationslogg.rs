//! Den installationsövergripande loggen: CLI-händelser och radering av företag, med aktör och
//! tid. Append-only: `core` har ingen kodväg som ändrar eller raderar en post, och schemat
//! avvisar `UPDATE` och `DELETE` (H3).

use jiff::Timestamp;
use sqlx::SqliteConnection;

use crate::aktor::{Aktör, AnvändarId, Kanal};
use crate::{Databas, DatabasFel};

/// Händelsetyperna i loggen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Händelse {
    AnvändareSkapad,
    FöretagRaderat,
    AnvändareÅterställd,
    AnvändareAvaktiverad,
    AnvändareÅteraktiverad,
    DomänBytt,
    BackupVerifierad,
    ÅterställningGjord,
}

impl Händelse {
    pub fn namn(self) -> &'static str {
        match self {
            Händelse::AnvändareSkapad => "användare skapad",
            Händelse::FöretagRaderat => "företag raderat",
            Händelse::AnvändareÅterställd => "användare återställd",
            Händelse::AnvändareAvaktiverad => "användare avaktiverad",
            Händelse::AnvändareÅteraktiverad => "användare återaktiverad",
            Händelse::DomänBytt => "domän bytt",
            Händelse::BackupVerifierad => "backup verifierad",
            Händelse::ÅterställningGjord => "återställning gjord",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Loggpost {
    pub tid: Timestamp,
    pub aktör: Aktör,
    /// Användarens visningsnamn, när aktören är en användare.
    pub användarnamn: Option<String>,
    /// Händelsens typ, t.ex. `användare skapad`.
    pub typ: String,
    /// Det händelsen rör, t.ex. en e-postadress.
    pub objekt: String,
    /// Strukturerad beskrivning som JSON.
    pub beskrivning: serde_json::Value,
}

/// Skriver en post i samma transaktion som händelsen.
pub(crate) async fn skriv(
    conn: &mut SqliteConnection,
    tid: Timestamp,
    aktör: &Aktör,
    händelse: Händelse,
    objekt: &str,
    beskrivning: serde_json::Value,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO installationslogg (tid, kanal, anvandare_id, typ, objekt, beskrivning)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
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

/// Hela loggen i den ordning posterna skrevs.
pub async fn läs(db: &Databas) -> Result<Vec<Loggpost>, DatabasFel> {
    type Rad = (
        String,
        String,
        Option<i64>,
        Option<String>,
        String,
        String,
        String,
    );
    let rader: Vec<Rad> = sqlx::query_as(
        "SELECT l.tid, l.kanal, l.anvandare_id, u.visningsnamn, l.typ, l.objekt, l.beskrivning
         FROM installationslogg l LEFT JOIN anvandare u ON u.id = l.anvandare_id
         ORDER BY l.id",
    )
    .fetch_all(db.pool())
    .await?;
    rader
        .into_iter()
        .map(
            |(tid, kanal, användare, användarnamn, typ, objekt, beskrivning)| {
                let fel = |vad: &str| DatabasFel::Korrupt(format!("installationsloggen: {vad}"));
                Ok(Loggpost {
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

impl Loggpost {
    /// Aktören som den visas: `Maya`, användarens namn, eller kanalen (`cli`).
    pub fn aktörsnamn(&self) -> String {
        match (self.aktör.kanal, &self.användarnamn) {
            (Kanal::Maya, _) => "Maya".into(),
            (_, Some(namn)) => namn.clone(),
            (kanal, None) => kanal.namn().into(),
        }
    }

    /// Beskrivningen som text: `nyckel: värde; …`.
    pub fn text(&self) -> String {
        crate::behandlingshistorik::fält_som_text(&self.beskrivning)
    }
}
