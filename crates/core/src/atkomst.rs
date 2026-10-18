//! Åtkomst: om en aktör får röra ett företag. Varje användningsfall som rör ett företag börjar
//! här, så att `core` auktoriserar och inte bara webben (ADR 0003).
//!
//! Ett användningsfall som skriver börjar ett [`Ärende`]: kanalpolicyn och åtkomsten kontrolleras,
//! och ändringen och dess post i behandlingshistoriken görs i samma SQLite-transaktion. Ett
//! användningsfall som bara läser anropar [`kräv`] i sin egen transaktion.

use jiff::Timestamp;
use sqlx::{Sqlite, SqliteConnection, Transaction};

use crate::aktor::{self, Aktör, Handling};
use crate::behandlingshistorik::{self, Händelse};
use crate::foretag::FöretagsId;
use crate::{Databas, DatabasFel, Klocka};

#[derive(Debug, thiserror::Error)]
pub enum ÅtkomstFel {
    #[error("kanalen får inte göra det här")]
    KanalenFårInte,
    /// Också när företaget inte finns, så att ingen kan pröva sig fram till vilka företag som
    /// finns.
    #[error("du har inte åtkomst till företaget")]
    IngenÅtkomst,
    #[error(transparent)]
    Databas(#[from] DatabasFel),
}

impl From<sqlx::Error> for ÅtkomstFel {
    fn from(e: sqlx::Error) -> Self {
        ÅtkomstFel::Databas(e.into())
    }
}

/// Kontrollerar kanalpolicyn för `handling` och att aktören är en aktiv användare med åtkomst
/// till företaget.
pub(crate) async fn kräv(
    conn: &mut SqliteConnection,
    aktör: &Aktör,
    företag: FöretagsId,
    handling: Handling,
) -> Result<(), ÅtkomstFel> {
    if !aktor::tillåter(aktör.kanal, handling) {
        return Err(ÅtkomstFel::KanalenFårInte);
    }
    let Some(användare) = aktör.användare else {
        return Err(ÅtkomstFel::IngenÅtkomst);
    };
    let har: bool = sqlx::query_scalar(
        "SELECT EXISTS (
             SELECT 1 FROM atkomst a JOIN anvandare u ON u.id = a.anvandare_id
             WHERE a.foretag_id = ? AND a.anvandare_id = ? AND u.avaktiverad IS NULL)",
    )
    .bind(företag.0)
    .bind(användare.0)
    .fetch_one(conn)
    .await?;
    if har {
        Ok(())
    } else {
        Err(ÅtkomstFel::IngenÅtkomst)
    }
}

/// Ett användningsfall som skriver i ett företag: en transaktion där åtkomsten är kontrollerad.
/// Ändringarna och posterna i behandlingshistoriken sparas tillsammans i [`Ärende::slutför`],
/// eller inte alls.
pub(crate) struct Ärende {
    tx: Transaction<'static, Sqlite>,
    pub aktör: Aktör,
    pub företag: FöretagsId,
    /// Tidpunkten som ärendet gäller, från klockan.
    pub nu: Timestamp,
}

impl Ärende {
    /// Börjar en skrivande transaktion (`BEGIN IMMEDIATE`, så att SQLite serialiserar
    /// skrivningarna) och kontrollerar kanalpolicyn och åtkomsten.
    #[allow(dead_code)] // Används av användningsfallen som ändrar ett befintligt företag.
    pub async fn börja(
        db: &Databas,
        klocka: &Klocka,
        aktör: &Aktör,
        företag: FöretagsId,
        handling: Handling,
    ) -> Result<Self, ÅtkomstFel> {
        let mut tx = db.pool().begin_with("BEGIN IMMEDIATE").await?;
        kräv(&mut tx, aktör, företag, handling).await?;
        Ok(Ärende {
            tx,
            aktör: *aktör,
            företag,
            nu: klocka.nu(),
        })
    }

    /// Ett ärende för ett företag som just skapades i `tx`, där åtkomsten redan är given.
    pub fn för_nytt_företag(
        tx: Transaction<'static, Sqlite>,
        aktör: &Aktör,
        företag: FöretagsId,
        nu: Timestamp,
    ) -> Self {
        Ärende {
            tx,
            aktör: *aktör,
            företag,
            nu,
        }
    }

    #[allow(dead_code)] // Används av användningsfallen som ändrar ett befintligt företag.
    pub fn conn(&mut self) -> &mut SqliteConnection {
        &mut self.tx
    }

    /// Skriver en post i företagets behandlingshistorik, med ärendets aktör och tid.
    pub async fn logga(
        &mut self,
        händelse: Händelse,
        objekt: &str,
        beskrivning: serde_json::Value,
    ) -> Result<(), sqlx::Error> {
        behandlingshistorik::skriv(
            &mut self.tx,
            self.företag,
            self.nu,
            &self.aktör,
            händelse,
            objekt,
            beskrivning,
        )
        .await
    }

    pub async fn slutför(self) -> Result<(), sqlx::Error> {
        self.tx.commit().await
    }
}
