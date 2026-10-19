//! Underlag (ADR 0002): originalfilen lagras som BLOB i samma databas, med SHA-256 som räknas när
//! den tas emot (VF5). Ett uppladdat underlag är ett *okopplat underlag* tills det kopplas till ett
//! verifikat. Koppling är många-till-många. En koppling till ett utkast kan tas bort; en koppling
//! till ett bokfört verifikat aldrig, och en koppling som görs efter bokföringen är en
//! *komplettering* som loggas med vem och när (VF6). Ett underlag som har hört till ett bokfört
//! verifikat kan aldrig tas bort. Schemat avvisar det också.
//!
//! Innehållet läses bara när underlaget visas eller laddas ned ([`filen`]).

use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;

use jiff::Timestamp;
use maya_domain::underlag::{self as regler, STÖRSTA_STORLEK};
use sha2::{Digest, Sha256};
use sqlx::SqliteConnection;

use crate::aktor::{Aktör, Handling};
use crate::atkomst::{self, Ärende, ÅtkomstFel};
use crate::behandlingshistorik::Händelse;
use crate::foretag::FöretagsId;
use crate::leverantorsfaktura::LeverantörsfakturaId;
use crate::verifikat::{self, Signatur, VerifikatId, Verifikationsnummer};
use crate::{Databas, DatabasFel, Klocka};

/// Ett underlags id i installationen. Vem som helst kan skriva ett id; `core` avgör om aktören har
/// åtkomst till underlagets företag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct UnderlagId(pub(crate) i64);

impl fmt::Display for UnderlagId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for UnderlagId {
    type Err = std::num::ParseIntError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse().map(UnderlagId)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum UnderlagFel {
    #[error("underlaget är större än 25 MB")]
    FörStort,
    /// Inget underlag med det id:t i företaget.
    #[error("underlaget finns inte")]
    FinnsInte,
    #[error("underlaget hör till ett bokfört verifikat och kan inte tas bort")]
    HörTillBokfört,
    /// Underlaget är original till en leverantörsfaktura (#145).
    #[error("underlaget är original till en leverantörsfaktura och kan inte tas bort")]
    HörTillFaktura,
    /// Komplettering gäller bara ett bokfört verifikat i företaget.
    #[error("verifikatet finns inte eller är inte bokfört")]
    IngetBokförtVerifikat,
    #[error("underlaget är redan kopplat till verifikatet")]
    RedanKopplat,
    #[error(transparent)]
    Åtkomst(#[from] ÅtkomstFel),
}

impl From<sqlx::Error> for UnderlagFel {
    fn from(e: sqlx::Error) -> Self {
        UnderlagFel::Åtkomst(e.into())
    }
}

impl From<DatabasFel> for UnderlagFel {
    fn from(e: DatabasFel) -> Self {
        UnderlagFel::Åtkomst(e.into())
    }
}

/// Resultatet av en uppladdning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Uppladdat {
    pub id: UnderlagId,
    pub sha256: String,
    /// Andra underlag i företaget med samma hash. Varningen stoppar inte uppladdningen.
    pub dubbletter: Vec<Dubblett>,
}

/// Ett annat underlag i företaget med samma innehåll.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dubblett {
    pub id: UnderlagId,
    pub filnamn: String,
    pub uppladdad: Timestamp,
}

/// Ett underlags metadata, utan innehållet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Underlag {
    pub id: UnderlagId,
    pub filnamn: String,
    pub mime: String,
    pub storlek: u64,
    /// SHA-256 av originalet, räknad när det togs emot, i hex.
    pub sha256: String,
    pub uppladdad: Signatur,
    /// Verifikaten underlaget är kopplat till, i den ordning kopplingarna gjordes.
    pub kopplingar: Vec<Koppling>,
    pub dubbletter: Vec<Dubblett>,
    /// Leverantörsfakturan som underlaget är original till (#145).
    pub leverantörsfaktura: Option<LeverantörsfakturaId>,
    /// Den externa kundfakturan som underlaget är original till (#143), eller Mayas kundfaktura
    /// vars arkiverade PDF det är; under kontantmetoden har PDF:en inget verifikat (#149).
    pub kundfaktura: Option<crate::kundfaktura::KundfakturaId>,
}

impl Underlag {
    /// Ett underlag som aldrig har hört till ett bokfört verifikat, och inte är original till
    /// en leverantörsfaktura, kan tas bort. Ett utkasts original tas bort genom att utkastet
    /// raderas först.
    pub fn kan_tas_bort(&self) -> bool {
        self.kopplingar.iter().all(|k| k.nummer.is_none())
            && self.leverantörsfaktura.is_none()
            && self.kundfaktura.is_none()
    }

    /// Ett *okopplat underlag*: inte kopplat till något verifikat och inte original till en
    /// leverantörsfaktura eller en kundfaktura.
    pub fn okopplat(&self) -> bool {
        self.kopplingar.is_empty()
            && self.leverantörsfaktura.is_none()
            && self.kundfaktura.is_none()
    }
}

/// En koppling från ett underlag till ett verifikat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Koppling {
    pub verifikat: VerifikatId,
    /// `None` när verifikatet är ett utkast.
    pub nummer: Option<Verifikationsnummer>,
    pub kopplad: Signatur,
    /// Kopplingen gjordes efter att verifikatet bokfördes (VF6).
    pub komplettering: bool,
}

/// Originalfilen, för att visa den eller ladda ned den.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fil {
    pub filnamn: String,
    pub mime: String,
    pub innehåll: Vec<u8>,
}

/// Vilka underlag listan visar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Urval {
    /// Underlag som inte är kopplade till något verifikat.
    #[default]
    Okopplade,
    Alla,
}

/// Tar emot ett underlag och arkiverar det direkt som ett okopplat underlag, med SHA-256 räknad
/// här, filnamn, MIME-typ, storlek, vem och när. Loggas som `underlag uppladdat`. Samma innehåll
/// som ett annat underlag i företaget ger en varning i [`Uppladdat::dubbletter`], men laddas upp.
pub async fn ladda_upp(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    företag: FöretagsId,
    filnamn: &str,
    mime: &str,
    innehåll: &[u8],
) -> Result<Uppladdat, UnderlagFel> {
    let mut ärende = Ärende::börja(db, klocka, aktör, företag, Handling::HanteraUnderlag).await?;
    let uppladdat = ladda_upp_i(&mut ärende, filnamn, mime, innehåll).await?;
    ärende.slutför().await?;
    Ok(uppladdat)
}

pub(crate) async fn ladda_upp_i(
    ärende: &mut Ärende,
    filnamn: &str,
    mime: &str,
    innehåll: &[u8],
) -> Result<Uppladdat, UnderlagFel> {
    if innehåll.len() > STÖRSTA_STORLEK {
        return Err(UnderlagFel::FörStort);
    }
    let (företag, aktör, nu) = (ärende.företag, ärende.aktör, ärende.nu);
    let sha256 = hex::encode(Sha256::digest(innehåll));
    let filnamn = rensa_filnamn(filnamn);
    let mime = regler::mime(mime, &innehåll[..innehåll.len().min(16)]);
    let dubbletter = dubbletter(ärende.conn(), företag, &sha256, None).await?;
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO underlag
             (foretag_id, filnamn, mime, storlek, sha256, innehall, uppladdad, uppladdad_kanal,
              uppladdad_av)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(företag.0)
    .bind(&filnamn)
    .bind(&mime)
    .bind(innehåll.len() as i64)
    .bind(&sha256)
    .bind(innehåll)
    .bind(nu.to_string())
    .bind(aktör.kanal.namn())
    .bind(aktör.användare.map(|a| a.0))
    .fetch_one(ärende.conn())
    .await?;
    ärende
        .logga(
            Händelse::UnderlagUppladdat,
            &filnamn,
            serde_json::json!({
                "underlag": id,
                "filnamn": filnamn,
                "mime": mime,
                "storlek": innehåll.len(),
                "sha256": sha256,
                "dubbletter": dubbletter.iter().map(|d| d.id.0).collect::<Vec<_>>(),
            }),
        )
        .await?;
    Ok(Uppladdat {
        id: UnderlagId(id),
        sha256,
        dubbletter,
    })
}

/// Kompletterar det bokförda verifikatet `verifikat` med underlaget (VF6). Verifikatet ändras
/// inte i övrigt. Kopplingen loggas som `verifikat kompletterat` med vem, när och vad som lades
/// till, och kan aldrig tas bort.
pub async fn komplettera(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    företag: FöretagsId,
    verifikat: VerifikatId,
    underlag: UnderlagId,
) -> Result<(), UnderlagFel> {
    let mut ärende = Ärende::börja(db, klocka, aktör, företag, Handling::Komplettera).await?;
    komplettera_i(&mut ärende, verifikat, underlag).await?;
    ärende.slutför().await?;
    Ok(())
}

/// Laddar upp en fil och kompletterar verifikatet med den, i en transaktion.
pub async fn ladda_upp_och_komplettera(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    företag: FöretagsId,
    verifikat: VerifikatId,
    fil: (&str, &str, &[u8]),
) -> Result<Uppladdat, UnderlagFel> {
    let mut ärende = Ärende::börja(db, klocka, aktör, företag, Handling::Komplettera).await?;
    atkomst::kräv(ärende.conn(), aktör, företag, Handling::HanteraUnderlag).await?;
    let (filnamn, mime, innehåll) = fil;
    let uppladdat = ladda_upp_i(&mut ärende, filnamn, mime, innehåll).await?;
    komplettera_i(&mut ärende, verifikat, uppladdat.id).await?;
    ärende.slutför().await?;
    Ok(uppladdat)
}

async fn komplettera_i(
    ärende: &mut Ärende,
    verifikat: VerifikatId,
    underlag: UnderlagId,
) -> Result<(), UnderlagFel> {
    let företag = ärende.företag;
    let nummer: Option<i64> = sqlx::query_scalar(
        "SELECT nummer FROM verifikat
         WHERE id = ? AND foretag_id = ? AND bokford IS NOT NULL",
    )
    .bind(verifikat.0)
    .bind(företag.0)
    .fetch_optional(ärende.conn())
    .await?;
    let Some(nummer) = nummer else {
        return Err(UnderlagFel::IngetBokförtVerifikat);
    };
    let nummer = verifikat::verifikationsnummer(nummer)?;
    let fil: Option<(String, String)> =
        sqlx::query_as("SELECT filnamn, sha256 FROM underlag WHERE id = ? AND foretag_id = ?")
            .bind(underlag.0)
            .bind(företag.0)
            .fetch_optional(ärende.conn())
            .await?;
    let Some((filnamn, sha256)) = fil else {
        return Err(UnderlagFel::FinnsInte);
    };
    let (aktör, nu) = (ärende.aktör, ärende.nu);
    let ny = sqlx::query(
        "INSERT INTO underlagskoppling
             (underlag_id, verifikat_id, kopplad, kopplad_kanal, kopplad_av, komplettering)
         VALUES (?, ?, ?, ?, ?, 1)
         ON CONFLICT DO NOTHING",
    )
    .bind(underlag.0)
    .bind(verifikat.0)
    .bind(nu.to_string())
    .bind(aktör.kanal.namn())
    .bind(aktör.användare.map(|a| a.0))
    .execute(ärende.conn())
    .await?
    .rows_affected();
    if ny == 0 {
        return Err(UnderlagFel::RedanKopplat);
    }
    ärende
        .logga(
            Händelse::VerifikatKompletterat,
            &nummer.to_string(),
            serde_json::json!({
                "verifikat": verifikat.0,
                "nummer": nummer.to_string(),
                "underlag": underlag.0,
                "filnamn": filnamn,
                "sha256": sha256,
            }),
        )
        .await?;
    Ok(())
}

/// Tar bort ett underlag som aldrig har hört till ett bokfört verifikat, och loggar det som
/// `underlag borttaget`. Kopplingar till utkast tas bort med det. Ett underlag som hör till ett
/// bokfört verifikat finns kvar (A1).
pub async fn ta_bort(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    företag: FöretagsId,
    id: UnderlagId,
) -> Result<(), UnderlagFel> {
    let mut ärende = Ärende::börja(db, klocka, aktör, företag, Handling::HanteraUnderlag).await?;
    let Some(u) = läs(ärende.conn(), företag, Some(id))
        .await?
        .into_iter()
        .next()
    else {
        return Err(UnderlagFel::FinnsInte);
    };
    if u.leverantörsfaktura.is_some() || u.kundfaktura.is_some() {
        return Err(UnderlagFel::HörTillFaktura);
    }
    if !u.kan_tas_bort() {
        return Err(UnderlagFel::HörTillBokfört);
    }
    sqlx::query("DELETE FROM underlagskoppling WHERE underlag_id = ?")
        .bind(id.0)
        .execute(ärende.conn())
        .await?;
    sqlx::query("DELETE FROM underlag WHERE id = ?")
        .bind(id.0)
        .execute(ärende.conn())
        .await?;
    ärende
        .logga(
            Händelse::UnderlagBorttaget,
            &u.filnamn,
            serde_json::json!({
                "underlag": id.0,
                "filnamn": u.filnamn,
                "mime": u.mime,
                "storlek": u.storlek,
                "sha256": u.sha256,
                "uppladdad": u.uppladdad.tid.to_string(),
                "utkast": u.kopplingar.iter().map(|k| k.verifikat.0).collect::<Vec<_>>(),
            }),
        )
        .await?;
    ärende.slutför().await?;
    Ok(())
}

/// Filnamnet utan katalog och styrtecken, högst 255 tecken.
pub(crate) fn rensa_filnamn(filnamn: &str) -> String {
    let namn: String = filnamn
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default()
        .chars()
        .filter(|c| !c.is_control())
        .take(255)
        .collect();
    let namn = namn.trim();
    if namn.is_empty() {
        "underlag".into()
    } else {
        namn.into()
    }
}

async fn dubbletter(
    conn: &mut SqliteConnection,
    företag: FöretagsId,
    sha256: &str,
    utom: Option<UnderlagId>,
) -> Result<Vec<Dubblett>, UnderlagFel> {
    let rader: Vec<(i64, String, String)> = sqlx::query_as(
        "SELECT id, filnamn, uppladdad FROM underlag
         WHERE foretag_id = ? AND sha256 = ? AND id IS NOT ? ORDER BY id",
    )
    .bind(företag.0)
    .bind(sha256)
    .bind(utom.map(|u| u.0))
    .fetch_all(conn)
    .await?;
    rader
        .into_iter()
        .map(|(id, filnamn, uppladdad)| {
            Ok(Dubblett {
                id: UnderlagId(id),
                filnamn,
                uppladdad: tid(&uppladdad)?,
            })
        })
        .collect()
}

/// Ett underlag i företaget, eller `None` om det inte finns där.
pub async fn underlaget(
    db: &Databas,
    aktör: &Aktör,
    företag: FöretagsId,
    id: UnderlagId,
) -> Result<Option<Underlag>, ÅtkomstFel> {
    let mut tx = db.pool().begin().await?;
    atkomst::kräv(&mut tx, aktör, företag, Handling::LäsaFöretag).await?;
    Ok(läs(&mut tx, företag, Some(id)).await?.into_iter().next())
}

/// Företagets underlag i urvalet, äldst först.
pub async fn lista(
    db: &Databas,
    aktör: &Aktör,
    företag: FöretagsId,
    urval: Urval,
) -> Result<Vec<Underlag>, ÅtkomstFel> {
    let mut tx = db.pool().begin().await?;
    atkomst::kräv(&mut tx, aktör, företag, Handling::LäsaFöretag).await?;
    let alla = läs(&mut tx, företag, None).await?;
    Ok(match urval {
        Urval::Alla => alla,
        Urval::Okopplade => alla.into_iter().filter(Underlag::okopplat).collect(),
    })
}

#[derive(sqlx::FromRow)]
struct UnderlagRad {
    id: i64,
    filnamn: String,
    mime: String,
    storlek: i64,
    sha256: String,
    uppladdad: String,
    uppladdad_kanal: String,
    uppladdad_av: Option<i64>,
    uppladdad_namn: Option<String>,
    leverantorsfaktura: Option<i64>,
    kundfaktura: Option<i64>,
}

#[derive(sqlx::FromRow)]
struct KopplingRad {
    underlag_id: i64,
    verifikat_id: i64,
    nummer: Option<i64>,
    kopplad: String,
    kopplad_kanal: String,
    kopplad_av: Option<i64>,
    kopplad_namn: Option<String>,
    komplettering: bool,
}

/// Underlagens metadata och kopplingar, utan innehållet: alla i företaget, eller bara `id`.
async fn läs(
    conn: &mut SqliteConnection,
    företag: FöretagsId,
    id: Option<UnderlagId>,
) -> Result<Vec<Underlag>, DatabasFel> {
    let rader: Vec<UnderlagRad> = sqlx::query_as(
        "SELECT u.id, u.filnamn, u.mime, u.storlek, u.sha256, u.uppladdad, u.uppladdad_kanal,
                u.uppladdad_av, a.visningsnamn AS uppladdad_namn,
                (SELECT f.id FROM leverantorsfaktura f WHERE f.underlag_id = u.id)
                    AS leverantorsfaktura,
                (SELECT f.id FROM kundfaktura f WHERE f.underlag_id = u.id)
                    AS kundfaktura
         FROM underlag u LEFT JOIN anvandare a ON a.id = u.uppladdad_av
         WHERE u.foretag_id = ?1 AND (?2 IS NULL OR u.id = ?2)
         ORDER BY u.uppladdad, u.id",
    )
    .bind(företag.0)
    .bind(id.map(|u| u.0))
    .fetch_all(&mut *conn)
    .await?;
    let kopplingar: Vec<KopplingRad> = sqlx::query_as(
        "SELECT k.underlag_id, k.verifikat_id, v.nummer, k.kopplad, k.kopplad_kanal,
                k.kopplad_av, a.visningsnamn AS kopplad_namn, k.komplettering
         FROM underlagskoppling k
         JOIN underlag u ON u.id = k.underlag_id
         JOIN verifikat v ON v.id = k.verifikat_id
         LEFT JOIN anvandare a ON a.id = k.kopplad_av
         WHERE u.foretag_id = ?1 AND (?2 IS NULL OR u.id = ?2)
         ORDER BY k.kopplad, k.rowid",
    )
    .bind(företag.0)
    .bind(id.map(|u| u.0))
    .fetch_all(&mut *conn)
    .await?;
    // Dubbletterna räknas mot alla underlag i företaget, också när bara ett läses.
    let hashar: Vec<(i64, String, String, String)> = sqlx::query_as(
        "SELECT id, sha256, filnamn, uppladdad FROM underlag WHERE foretag_id = ? ORDER BY id",
    )
    .bind(företag.0)
    .fetch_all(&mut *conn)
    .await?;
    let mut per_hash: HashMap<&str, Vec<Dubblett>> = HashMap::new();
    for (id, sha256, filnamn, uppladdad) in &hashar {
        per_hash.entry(sha256).or_default().push(Dubblett {
            id: UnderlagId(*id),
            filnamn: filnamn.clone(),
            uppladdad: tid(uppladdad)?,
        });
    }
    rader
        .into_iter()
        .map(|u| {
            Ok(Underlag {
                id: UnderlagId(u.id),
                leverantörsfaktura: u.leverantorsfaktura.map(LeverantörsfakturaId),
                kundfaktura: u.kundfaktura.map(crate::kundfaktura::KundfakturaId),
                kopplingar: kopplingar
                    .iter()
                    .filter(|k| k.underlag_id == u.id)
                    .map(|k| {
                        Ok(Koppling {
                            verifikat: VerifikatId(k.verifikat_id),
                            nummer: k.nummer.map(verifikat::verifikationsnummer).transpose()?,
                            kopplad: verifikat::signatur(
                                &k.kopplad,
                                &k.kopplad_kanal,
                                k.kopplad_av,
                                k.kopplad_namn.clone(),
                            )?,
                            komplettering: k.komplettering,
                        })
                    })
                    .collect::<Result<_, DatabasFel>>()?,
                dubbletter: per_hash
                    .get(u.sha256.as_str())
                    .into_iter()
                    .flatten()
                    .filter(|d| d.id.0 != u.id)
                    .cloned()
                    .collect(),
                filnamn: u.filnamn,
                mime: u.mime,
                storlek: u64::try_from(u.storlek)
                    .map_err(|_| DatabasFel::Korrupt("underlag.storlek".into()))?,
                sha256: u.sha256,
                uppladdad: verifikat::signatur(
                    &u.uppladdad,
                    &u.uppladdad_kanal,
                    u.uppladdad_av,
                    u.uppladdad_namn,
                )?,
            })
        })
        .collect()
}

/// Originalfilen, oförändrad, eller `None` om underlaget inte finns i företaget.
pub async fn filen(
    db: &Databas,
    aktör: &Aktör,
    företag: FöretagsId,
    id: UnderlagId,
) -> Result<Option<Fil>, ÅtkomstFel> {
    let mut tx = db.pool().begin().await?;
    atkomst::kräv(&mut tx, aktör, företag, Handling::LäsaFöretag).await?;
    let fil: Option<(String, String, Vec<u8>)> = sqlx::query_as(
        "SELECT filnamn, mime, innehall FROM underlag WHERE foretag_id = ? AND id = ?",
    )
    .bind(företag.0)
    .bind(id.0)
    .fetch_optional(&mut *tx)
    .await?;
    Ok(fil.map(|(filnamn, mime, innehåll)| Fil {
        filnamn,
        mime,
        innehåll,
    }))
}

/// Ett underlag kopplat till ett verifikat, med kopplingen (VF2, VF5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifikatetsUnderlag {
    pub id: UnderlagId,
    pub filnamn: String,
    pub mime: String,
    pub storlek: u64,
    pub sha256: String,
    pub kopplad: Signatur,
    /// Kopplingen gjordes efter att verifikatet bokfördes (VF6).
    pub komplettering: bool,
}

/// Verifikatets underlag: de som kopplades före bokföringen i ordning, sedan kompletteringarna.
pub(crate) async fn verifikatets(
    conn: &mut SqliteConnection,
    verifikat: VerifikatId,
) -> Result<Vec<VerifikatetsUnderlag>, DatabasFel> {
    type Rad = (
        i64,
        String,
        String,
        i64,
        String,
        String,
        String,
        Option<i64>,
        Option<String>,
        bool,
    );
    let rader: Vec<Rad> = sqlx::query_as(
        "SELECT u.id, u.filnamn, u.mime, u.storlek, u.sha256, k.kopplad, k.kopplad_kanal,
                k.kopplad_av, a.visningsnamn, k.komplettering
         FROM underlagskoppling k
         JOIN underlag u ON u.id = k.underlag_id
         LEFT JOIN anvandare a ON a.id = k.kopplad_av
         WHERE k.verifikat_id = ?
         ORDER BY k.komplettering, k.kopplad, k.rowid",
    )
    .bind(verifikat.0)
    .fetch_all(conn)
    .await?;
    rader
        .into_iter()
        .map(
            |(id, filnamn, mime, storlek, sha256, tid, kanal, av, namn, komplettering)| {
                Ok(VerifikatetsUnderlag {
                    id: UnderlagId(id),
                    filnamn,
                    mime,
                    storlek: u64::try_from(storlek)
                        .map_err(|_| DatabasFel::Korrupt("underlag.storlek".into()))?,
                    sha256,
                    kopplad: verifikat::signatur(&tid, &kanal, av, namn)?,
                    komplettering,
                })
            },
        )
        .collect()
}

/// Referenserna till underlagen i en post i behandlingshistoriken: id, filnamn och hash.
pub(crate) fn referenser(underlag: &[VerifikatetsUnderlag]) -> serde_json::Value {
    underlag
        .iter()
        .map(|u| {
            serde_json::json!({
                "underlag": u.id.0,
                "filnamn": u.filnamn,
                "sha256": u.sha256,
            })
        })
        .collect()
}

fn tid(text: &str) -> Result<Timestamp, DatabasFel> {
    text.parse()
        .map_err(|_| DatabasFel::Korrupt(format!("en tidpunkt: {text}")))
}
