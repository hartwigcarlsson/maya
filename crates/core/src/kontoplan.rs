//! Företagets kontoplan: en egen kopia av den inbyggda BAS-kontoplanen (ADR 0006, 0007). Ett
//! konto raderas aldrig och byter aldrig nummer; bara namnet och aktiv ändras, och varje ändring
//! loggas i behandlingshistoriken (H2).

use maya_domain::kontoplan::{
    self as regler, Inaktiveringsspärr, Kontofel, Kontonummer, NyttKonto, bas,
};
use sqlx::SqliteConnection;

use crate::aktor::{Aktör, Handling};
use crate::atkomst::{self, Ärende, ÅtkomstFel};
use crate::behandlingshistorik::{self, Händelse, Post};
use crate::foretag::FöretagsId;
use crate::{Databas, DatabasFel, Klocka, saldo};

pub use maya_domain::kontoplan::Årssaldo;

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
    /// Om en senare BAS-version lade till kontot ("Nya i BAS").
    pub ny_i_bas: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum KontoFel {
    /// Formuläret för ett eget konto klarar inte kontrollen i `domain`; samma koder som
    /// formuläret visar.
    #[error("formuläret är inte rätt ifyllt")]
    Ogiltigt(Vec<Kontofel>),
    #[error("{nummer} finns redan: {namn}")]
    Finns {
        nummer: Kontonummer,
        namn: String,
        aktiv: bool,
    },
    #[error("kontot finns inte i kontoplanen")]
    FinnsInte,
    #[error("namnet kan inte vara tomt")]
    NamnTomt,
    #[error("{0}")]
    KanInteInaktiveras(Inaktiveringsspärr),
    #[error(transparent)]
    Åtkomst(#[from] ÅtkomstFel),
}

impl From<sqlx::Error> for KontoFel {
    fn from(e: sqlx::Error) -> Self {
        KontoFel::Åtkomst(e.into())
    }
}

impl From<DatabasFel> for KontoFel {
    fn from(e: DatabasFel) -> Self {
        KontoFel::Åtkomst(e.into())
    }
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
    bas_tillagg: bool,
}

impl KontoRad {
    fn konto(self) -> Result<Konto, DatabasFel> {
        let fel = |vad: &str| DatabasFel::Korrupt(format!("kontoplanen: {vad}"));
        Ok(Konto {
            nummer: u16::try_from(self.nummer)
                .ok()
                .and_then(Kontonummer::new)
                .ok_or_else(|| fel("nummer"))?,
            namn: self.namn,
            bas_namn: self.bas_namn,
            ursprung: match self.ursprung.as_str() {
                "bas" => Ursprung::Bas,
                "eget" => Ursprung::Eget,
                _ => return Err(fel("ursprung")),
            },
            aktiv: self.aktiv,
            bas_version: self.bas_version,
            ny_i_bas: self.bas_tillagg,
        })
    }
}

const KONTO_KOLUMNER: &str = "nummer, namn, bas_namn, ursprung, aktiv, bas_version, bas_tillagg";

/// Företagets alla konton i nummerordning.
pub async fn konton(
    db: &Databas,
    aktör: &Aktör,
    företag: FöretagsId,
) -> Result<Vec<Konto>, ÅtkomstFel> {
    let mut tx = db.pool().begin().await?;
    atkomst::kräv(&mut tx, aktör, företag, Handling::LäsaFöretag).await?;
    let rader: Vec<KontoRad> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {KONTO_KOLUMNER} FROM konto WHERE foretag_id = ? ORDER BY nummer"
    )))
    .bind(företag.0)
    .fetch_all(&mut *tx)
    .await?;
    Ok(rader
        .into_iter()
        .map(KontoRad::konto)
        .collect::<Result<_, _>>()?)
}

async fn hämta(
    conn: &mut SqliteConnection,
    företag: FöretagsId,
    nummer: Kontonummer,
) -> Result<Option<Konto>, KontoFel> {
    let rad: Option<KontoRad> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {KONTO_KOLUMNER} FROM konto WHERE foretag_id = ? AND nummer = ?"
    )))
    .bind(företag.0)
    .bind(nummer.get())
    .fetch_optional(conn)
    .await?;
    Ok(rad.map(KontoRad::konto).transpose()?)
}

/// Kontots poster i behandlingshistoriken, äldst först.
pub async fn historik(
    db: &Databas,
    aktör: &Aktör,
    företag: FöretagsId,
    nummer: Kontonummer,
) -> Result<Vec<Post>, ÅtkomstFel> {
    let mut tx = db.pool().begin().await?;
    atkomst::kräv(&mut tx, aktör, företag, Handling::LäsaFöretag).await?;
    Ok(behandlingshistorik::läs_konto(&mut tx, företag, nummer.get()).await?)
}

/// Kontots saldo i varje räkenskapsår som inte är stängt.
pub async fn saldon(
    db: &Databas,
    aktör: &Aktör,
    företag: FöretagsId,
    nummer: Kontonummer,
) -> Result<Vec<Årssaldo>, ÅtkomstFel> {
    let mut tx = db.pool().begin().await?;
    atkomst::kräv(&mut tx, aktör, företag, Handling::LäsaFöretag).await?;
    Ok(saldon_i_öppna_år(&mut tx, företag, nummer).await?)
}

/// Kontots utgående balans (ingående balans och bokförda verifikatrader) per räkenskapsår som
/// inte är stängt, för år där kontot har en ingående balans eller rader. Räkenskapsår kan inte
/// stängas ännu, så alla år räknas.
async fn saldon_i_öppna_år(
    conn: &mut SqliteConnection,
    företag: FöretagsId,
    nummer: Kontonummer,
) -> Result<Vec<Årssaldo>, DatabasFel> {
    Ok(saldo::saldobalanser(conn, företag)
        .await?
        .iter()
        .filter_map(|sb| {
            let konto = sb.konton.iter().find(|k| k.konto == nummer)?;
            Some(Årssaldo {
                start: sb.år.start(),
                slut: sb.år.slut(),
                öre: konto.utgående.öre(),
            })
        })
        .collect())
}

/// Ett ärende som ändrar ett befintligt konto, och kontot som det är nu.
async fn börja_ändra(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    företag: FöretagsId,
    nummer: Kontonummer,
) -> Result<(Ärende, Konto), KontoFel> {
    let mut ärende = Ärende::börja(db, klocka, aktör, företag, Handling::ÄndraKontoplan).await?;
    let konto = hämta(ärende.conn(), företag, nummer)
        .await?
        .ok_or(KontoFel::FinnsInte)?;
    Ok((ärende, konto))
}

/// Byter namn på ett konto. Namnet syns i alla år.
pub async fn byt_namn(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    företag: FöretagsId,
    nummer: Kontonummer,
    namn: &str,
) -> Result<(), KontoFel> {
    let namn = namn.trim();
    if namn.is_empty() {
        return Err(KontoFel::NamnTomt);
    }
    let (mut ärende, konto) = börja_ändra(db, klocka, aktör, företag, nummer).await?;
    if konto.namn == namn {
        return Ok(());
    }
    sqlx::query("UPDATE konto SET namn = ? WHERE foretag_id = ? AND nummer = ?")
        .bind(namn)
        .bind(företag.0)
        .bind(nummer.get())
        .execute(ärende.conn())
        .await?;
    ärende
        .logga(
            Händelse::KontoNamnbytt,
            &nummer.to_string(),
            serde_json::json!({ "från": konto.namn, "till": namn }),
        )
        .await?;
    ärende.slutför().await?;
    Ok(())
}

/// Aktiverar ett konto så att det kan bokföras på.
pub async fn aktivera(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    företag: FöretagsId,
    nummer: Kontonummer,
) -> Result<(), KontoFel> {
    let (ärende, konto) = börja_ändra(db, klocka, aktör, företag, nummer).await?;
    if konto.aktiv {
        return Ok(());
    }
    sätt_aktiv(ärende, nummer, true, serde_json::json!({})).await
}

/// Inaktiverar ett konto. Nekas för flödenas konton och för ett konto med saldo skilt från noll
/// i ett räkenskapsår som inte är stängt.
pub async fn inaktivera(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    företag: FöretagsId,
    nummer: Kontonummer,
) -> Result<(), KontoFel> {
    let (mut ärende, konto) = börja_ändra(db, klocka, aktör, företag, nummer).await?;
    let saldon = saldon_i_öppna_år(ärende.conn(), företag, nummer).await?;
    regler::kan_inaktiveras(nummer, &saldon).map_err(KontoFel::KanInteInaktiveras)?;
    if !konto.aktiv {
        return Ok(());
    }
    sätt_aktiv(ärende, nummer, false, serde_json::json!({})).await
}

async fn sätt_aktiv(
    mut ärende: Ärende,
    nummer: Kontonummer,
    aktiv: bool,
    beskrivning: serde_json::Value,
) -> Result<(), KontoFel> {
    sqlx::query("UPDATE konto SET aktiv = ? WHERE foretag_id = ? AND nummer = ?")
        .bind(aktiv)
        .bind(ärende.företag.0)
        .bind(nummer.get())
        .execute(ärende.conn())
        .await?;
    let händelse = if aktiv {
        Händelse::KontoAktiverat
    } else {
        Händelse::KontoInaktiverat
    };
    ärende
        .logga(händelse, &nummer.to_string(), beskrivning)
        .await?;
    ärende.slutför().await?;
    Ok(())
}

/// Skapar ett eget, aktivt konto på ett nummer som inte finns i kontoplanen.
pub async fn skapa_eget(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    företag: FöretagsId,
    formulär: &NyttKonto,
) -> Result<Kontonummer, KontoFel> {
    let (nummer, namn) = formulär.kontrollera().map_err(KontoFel::Ogiltigt)?;
    let mut ärende = Ärende::börja(db, klocka, aktör, företag, Handling::ÄndraKontoplan).await?;
    if let Some(finns) = hämta(ärende.conn(), företag, nummer).await? {
        return Err(KontoFel::Finns {
            nummer,
            namn: finns.namn,
            aktiv: finns.aktiv,
        });
    }
    sqlx::query(
        "INSERT INTO konto (foretag_id, nummer, namn, ursprung, aktiv) VALUES (?, ?, ?, 'eget', 1)",
    )
    .bind(företag.0)
    .bind(nummer.get())
    .bind(&namn)
    .execute(ärende.conn())
    .await?;
    ärende
        .logga(
            Händelse::KontoSkapat,
            &nummer.to_string(),
            serde_json::json!({ "namn": namn }),
        )
        .await?;
    ärende.slutför().await?;
    Ok(nummer)
}

/// Lägger till en BAS-versions konton i varje företag (ADR 0007): bara nummer som inte finns,
/// som inaktiva, och aldrig namnbyten eller borttagningar. Ett eget konto på samma nummer
/// behålls. Varje företag som får konton får en post i behandlingshistoriken med Maya som
/// aktör. Ger antalet tillagda konton i alla företag.
pub async fn bas_tillägg(
    db: &Databas,
    klocka: &Klocka,
    version: &str,
    konton: &[(Kontonummer, &str)],
) -> Result<u64, DatabasFel> {
    let nu = klocka.nu();
    let lista = serde_json::Value::Array(
        konton
            .iter()
            .map(|(nummer, namn)| serde_json::json!([nummer.get(), namn]))
            .collect(),
    )
    .to_string();
    let mut tx = db.pool().begin_with("BEGIN IMMEDIATE").await?;
    let företag: Vec<i64> = sqlx::query_scalar("SELECT id FROM foretag ORDER BY id")
        .fetch_all(&mut *tx)
        .await?;
    let mut totalt = 0;
    for id in företag {
        let tillagda: Vec<i64> = sqlx::query_scalar(
            "INSERT INTO konto
                 (foretag_id, nummer, namn, bas_namn, ursprung, aktiv, bas_version, bas_tillagg)
             SELECT ?1, j.value ->> 0, j.value ->> 1, j.value ->> 1, 'bas', 0, ?2, 1
             FROM json_each(?3) j WHERE true
             ON CONFLICT (foretag_id, nummer) DO NOTHING
             RETURNING nummer",
        )
        .bind(id)
        .bind(version)
        .bind(&lista)
        .fetch_all(&mut *tx)
        .await?;
        if tillagda.is_empty() {
            continue;
        }
        let mut tillagda = tillagda;
        tillagda.sort_unstable();
        totalt += tillagda.len() as u64;
        behandlingshistorik::skriv(
            &mut tx,
            FöretagsId(id),
            nu,
            &Aktör::maya(),
            Händelse::BasTillägg,
            version,
            serde_json::json!({ "bas_version": version, "konton": tillagda }),
        )
        .await?;
    }
    tx.commit().await?;
    Ok(totalt)
}

/// [`bas_tillägg`] med den inbyggda BAS-versionen. Körs när servern startar, så att företag som
/// lades upp med en äldre BAS får de nya kontona.
pub async fn lägg_till_inbyggd_bas(db: &Databas, klocka: &Klocka) -> Result<u64, DatabasFel> {
    let konton: Vec<_> = bas::konton()
        .iter()
        .map(|k| (k.nummer(), k.namn()))
        .collect();
    bas_tillägg(db, klocka, bas::VERSION, &konton).await
}
