//! Företagets kontoplan: en egen kopia av den inbyggda BAS-kontoplanen (ADR 0006, 0007).

use maya_domain::kontoplan::{self as regler, Kontonummer, bas};
use sqlx::SqliteConnection;

use crate::Databas;
use crate::DatabasFel;
use crate::aktor::{Aktör, Handling};
use crate::atkomst::{self, ÅtkomstFel};
use crate::foretag::FöretagsId;

/// Var ett konto kommer ifrån.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ursprung {
    Bas,
    Eget,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Konto {
    pub nummer: Kontonummer,
    pub namn: String,
    /// Namnet i BAS när kontot kopierades; `None` för ett eget konto.
    pub bas_namn: Option<String>,
    pub ursprung: Ursprung,
    pub aktiv: bool,
    /// BAS-versionen som kontot kom med; `None` för ett eget konto.
    pub bas_version: Option<String>,
}

/// Kopierar BAS till ett nytt företag. Grundkontona och flödenas konton är aktiva, övriga
/// inaktiva.
pub(crate) async fn kopiera_bas(
    conn: &mut SqliteConnection,
    företag: FöretagsId,
) -> Result<(), sqlx::Error> {
    let konton: Vec<_> = bas::konton()
        .iter()
        .map(|k| {
            let aktiv = k.grundkonto() || regler::flöde_som_bokför_på(k.nummer()).is_some();
            serde_json::json!([k.nummer().get(), k.namn(), aktiv])
        })
        .collect();
    sqlx::query(
        "INSERT INTO konto (foretag_id, nummer, namn, bas_namn, ursprung, aktiv, bas_version)
         SELECT ?1, j.value ->> 0, j.value ->> 1, j.value ->> 1, 'bas', j.value ->> 2, ?2
         FROM json_each(?3) j",
    )
    .bind(företag.0)
    .bind(bas::VERSION)
    .bind(serde_json::Value::Array(konton).to_string())
    .execute(conn)
    .await?;
    Ok(())
}

#[derive(sqlx::FromRow)]
struct KontoRad {
    nummer: i64,
    namn: String,
    bas_namn: Option<String>,
    ursprung: String,
    aktiv: bool,
    bas_version: Option<String>,
}

/// Företagets alla konton i nummerordning.
pub async fn konton(
    db: &Databas,
    aktör: &Aktör,
    företag: FöretagsId,
) -> Result<Vec<Konto>, ÅtkomstFel> {
    let mut tx = db.pool().begin().await?;
    atkomst::kräv(&mut tx, aktör, företag, Handling::LäsaFöretag).await?;
    let rader: Vec<KontoRad> = sqlx::query_as(
        "SELECT nummer, namn, bas_namn, ursprung, aktiv, bas_version
             FROM konto WHERE foretag_id = ? ORDER BY nummer",
    )
    .bind(företag.0)
    .fetch_all(&mut *tx)
    .await?;
    let fel = |vad: &str| DatabasFel::Korrupt(format!("kontoplanen: {vad}"));
    let konton = rader
        .into_iter()
        .map(|rad| {
            Ok(Konto {
                nummer: u16::try_from(rad.nummer)
                    .ok()
                    .and_then(Kontonummer::new)
                    .ok_or_else(|| fel("nummer"))?,
                namn: rad.namn,
                bas_namn: rad.bas_namn,
                ursprung: match rad.ursprung.as_str() {
                    "bas" => Ursprung::Bas,
                    "eget" => Ursprung::Eget,
                    _ => return Err(fel("ursprung")),
                },
                aktiv: rad.aktiv,
                bas_version: rad.bas_version,
            })
        })
        .collect::<Result<_, DatabasFel>>()?;
    Ok(konton)
}
