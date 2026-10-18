//! Den installationsövergripande loggen: CLI-händelser och radering av företag, med aktör och
//! tid. Append-only: `core` har ingen kodväg som ändrar eller raderar en post, och schemat
//! avvisar `UPDATE` och `DELETE` (H3).

use std::collections::HashSet;

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

    /// Vad händelsen rör, som avgör vem som ser den (story 36).
    fn rör(self) -> Rör {
        match self {
            Händelse::AnvändareSkapad
            | Händelse::AnvändareÅterställd
            | Händelse::AnvändareAvaktiverad
            | Händelse::AnvändareÅteraktiverad => Rör::Användaren,
            Händelse::FöretagRaderat => Rör::Företaget,
            Händelse::DomänBytt | Händelse::BackupVerifierad | Händelse::ÅterställningGjord => {
                Rör::Installationen
            }
        }
    }

    fn från_namn(namn: &str) -> Option<Händelse> {
        Self::ALLA.into_iter().find(|h| h.namn() == namn)
    }

    const ALLA: [Händelse; 8] = [
        Händelse::AnvändareSkapad,
        Händelse::FöretagRaderat,
        Händelse::AnvändareÅterställd,
        Händelse::AnvändareAvaktiverad,
        Händelse::AnvändareÅteraktiverad,
        Händelse::DomänBytt,
        Händelse::BackupVerifierad,
        Händelse::ÅterställningGjord,
    ];
}

enum Rör {
    /// Användaren vars e-postadress är postens objekt.
    Användaren,
    /// Ett raderat företag; vilka som hade åtkomst står i `installationslogg_atkomst`.
    Företaget,
    /// Hela installationen, utan uppgifter om enskilda användare eller företag.
    Installationen,
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

/// Skriver en post i samma transaktion som händelsen och ger postens id.
pub(crate) async fn skriv(
    conn: &mut SqliteConnection,
    tid: Timestamp,
    aktör: &Aktör,
    händelse: Händelse,
    objekt: &str,
    beskrivning: serde_json::Value,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO installationslogg (tid, kanal, anvandare_id, typ, objekt, beskrivning)
         VALUES (?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(tid.to_string())
    .bind(aktör.kanal.namn())
    .bind(aktör.användare.map(|a| a.0))
    .bind(händelse.namn())
    .bind(objekt)
    .bind(beskrivning.to_string())
    .fetch_one(conn)
    .await
}

/// Skriver att `användare` hade åtkomst till företaget som posten `logg_id` raderade.
pub(crate) async fn skriv_åtkomst(
    conn: &mut SqliteConnection,
    logg_id: i64,
    användare: &[i64],
) -> Result<(), sqlx::Error> {
    for id in användare {
        sqlx::query("INSERT INTO installationslogg_atkomst (logg_id, anvandare_id) VALUES (?, ?)")
            .bind(logg_id)
            .bind(id)
            .execute(&mut *conn)
            .await?;
    }
    Ok(())
}

/// Hela loggen i den ordning posterna skrevs.
pub async fn läs(db: &Databas) -> Result<Vec<Loggpost>, DatabasFel> {
    let mut conn = db.pool().acquire().await?;
    Ok(läs_i(&mut conn)
        .await?
        .into_iter()
        .map(|(_, post)| post)
        .collect())
}

/// Loggen som `aktör` får se, i den ordning posterna skrevs. CLI:t ser hela. En användare ser
/// poster om sig själv och om användare hen delar ett företag med, raderingen av företag hen
/// hade åtkomst till och händelser som rör hela installationen (story 36).
pub async fn läs_för(db: &Databas, aktör: &Aktör) -> Result<Vec<Loggpost>, DatabasFel> {
    let Some(användare) = aktör.användare else {
        return läs(db).await;
    };
    let mut tx = db.pool().begin().await?;
    let eposter: HashSet<String> = sqlx::query_scalar(
        "SELECT epost FROM anvandare
         WHERE id = ?1 OR id IN (SELECT a.anvandare_id FROM atkomst a
                                 JOIN atkomst egen ON egen.foretag_id = a.foretag_id
                                 WHERE egen.anvandare_id = ?1)",
    )
    .bind(användare.0)
    .fetch_all(&mut *tx)
    .await?
    .into_iter()
    .collect();
    let raderade: HashSet<i64> =
        sqlx::query_scalar("SELECT logg_id FROM installationslogg_atkomst WHERE anvandare_id = ?")
            .bind(användare.0)
            .fetch_all(&mut *tx)
            .await?
            .into_iter()
            .collect();
    let poster = läs_i(&mut tx).await?;
    tx.commit().await?;
    Ok(poster
        .into_iter()
        .filter(
            |(id, post)| match Händelse::från_namn(&post.typ).map(Händelse::rör) {
                Some(Rör::Användaren) => eposter.contains(&post.objekt),
                Some(Rör::Företaget) => raderade.contains(id),
                Some(Rör::Installationen) => true,
                None => false,
            },
        )
        .map(|(_, post)| post)
        .collect())
}

async fn läs_i(conn: &mut SqliteConnection) -> Result<Vec<(i64, Loggpost)>, DatabasFel> {
    type Rad = (
        i64,
        String,
        String,
        Option<i64>,
        Option<String>,
        String,
        String,
        String,
    );
    let rader: Vec<Rad> = sqlx::query_as(
        "SELECT l.id, l.tid, l.kanal, l.anvandare_id, u.visningsnamn, l.typ, l.objekt, l.beskrivning
         FROM installationslogg l LEFT JOIN anvandare u ON u.id = l.anvandare_id
         ORDER BY l.id",
    )
    .fetch_all(conn)
    .await?;
    rader
        .into_iter()
        .map(
            |(id, tid, kanal, användare, användarnamn, typ, objekt, beskrivning)| {
                let fel = |vad: &str| DatabasFel::Korrupt(format!("installationsloggen: {vad}"));
                let post = Loggpost {
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
                };
                Ok((id, post))
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
