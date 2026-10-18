//! Användare, passkeys, engångslänkar och sessioner. WebAuthn-ceremonierna görs i `maya-auth`;
//! här lagras resultatet. En passkey är ogenomskinlig JSON för `core`.

use jiff::{SignedDuration, Timestamp};
use sha2::{Digest, Sha256};
use sqlx::SqliteConnection;

use crate::aktor::{self, Aktör, AnvändarId, Handling};
use crate::installationslogg::{self, Händelse};
use crate::{Databas, DatabasFel, Klocka};

/// En session löper ut efter så här lång tid utan aktivitet.
pub const SESSION_INAKTIV: SignedDuration = SignedDuration::from_hours(7 * 24);
/// En session löper ut senast så här länge efter inloggningen.
pub const SESSION_LÄNGST: SignedDuration = SignedDuration::from_hours(30 * 24);
/// En engångslänk gäller så här länge.
pub const LÄNK_GILTIG: SignedDuration = SignedDuration::from_hours(24);

#[derive(Debug, thiserror::Error)]
pub enum InloggningsFel {
    #[error("kanalen får inte göra det här")]
    KanalenFårInte,
    #[error("det finns redan en användare; nya användare bjuds in i webben")]
    AnvändareFinns,
    #[error(transparent)]
    OgiltigEpost(#[from] maya_domain::epost::OgiltigEpost),
    #[error("länken är ogiltig, redan använd eller för gammal")]
    OgiltigLänk,
    #[error("passkeyn är redan registrerad")]
    PasskeyFinns,
    #[error("passkeyn hör inte till användaren")]
    OkändPasskey,
    #[error(transparent)]
    Databas(#[from] DatabasFel),
}

impl From<sqlx::Error> for InloggningsFel {
    fn from(e: sqlx::Error) -> Self {
        InloggningsFel::Databas(e.into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Användare {
    pub id: AnvändarId,
    /// WebAuthn user handle: 16 slumpade byte.
    pub handle: [u8; 16],
    pub epost: String,
    pub visningsnamn: String,
}

/// En hemlig token som bara visas en gång. Databasen har bara dess SHA-256.
#[derive(Debug, Clone)]
pub struct Token(String);

impl Token {
    fn ny() -> Self {
        let mut bytes = [0u8; 32];
        getrandom::fill(&mut bytes).expect("slumptal från operativsystemet");
        Token(hex::encode(bytes))
    }
}

fn hash(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

/// En engångslänk för att registrera en passkey.
#[derive(Debug, Clone)]
pub struct Registreringslänk {
    token: Token,
    pub giltig_till: Timestamp,
}

impl Registreringslänk {
    pub fn token(&self) -> &str {
        &self.token.0
    }
}

/// En inloggad session. Token går till cookien.
#[derive(Debug, Clone)]
pub struct Session {
    token: Token,
}

impl Session {
    pub fn token(&self) -> &str {
        &self.token.0
    }
}

/// Det en registreringsceremoni behöver veta.
#[derive(Debug, Clone)]
pub struct Registrering {
    pub användare: Användare,
    /// Credential-id:n som användaren redan har, för `exclude_credentials`.
    pub befintliga_passkeys: Vec<Vec<u8>>,
}

/// En passkey som en ceremoni har registrerat.
#[derive(Debug, Clone)]
pub struct NyPasskey {
    pub credential_id: Vec<u8>,
    pub namn: String,
    /// `webauthn_rs::prelude::Passkey` som JSON.
    pub passkey: String,
}

/// Det en inloggningsceremoni behöver veta.
#[derive(Debug, Clone)]
pub struct Inloggningsgrund {
    pub användare: Användare,
    /// Användarens passkeys som JSON.
    pub passkeys: Vec<String>,
}

/// Den som en giltig sessionscookie hör till.
#[derive(Debug, Clone)]
pub struct Inloggad {
    pub användare: Användare,
    pub aktör: Aktör,
    /// När användaren senast använde en passkey i sessionen.
    pub passkey_använd: Timestamp,
}

pub async fn finns_användare(db: &Databas) -> Result<bool, DatabasFel> {
    Ok(
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM anvandare)")
            .fetch_one(db.pool())
            .await?,
    )
}

/// Skapar installationens första användare och en registreringslänk till hen. Bara CLI:t, och
/// bara så länge ingen användare finns (ADR 0005). Händelsen skrivs i installationsloggen.
pub async fn skapa_första_användare(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    epost: &str,
) -> Result<Registreringslänk, InloggningsFel> {
    if !aktor::tillåter(aktör.kanal, Handling::HanteraAnvändare) {
        return Err(InloggningsFel::KanalenFårInte);
    }
    let epost = maya_domain::epost::normalisera(epost)?;
    let nu = klocka.nu();
    let mut tx = db.pool().begin_with("BEGIN IMMEDIATE").await?;
    let finns: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM anvandare)")
        .fetch_one(&mut *tx)
        .await?;
    if finns {
        return Err(InloggningsFel::AnvändareFinns);
    }
    let mut handle = [0u8; 16];
    getrandom::fill(&mut handle).expect("slumptal från operativsystemet");
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO anvandare (handle, epost, visningsnamn, skapad) VALUES (?, ?, ?, ?)
         RETURNING id",
    )
    .bind(&handle[..])
    .bind(&epost)
    .bind(&epost)
    .bind(nu.to_string())
    .fetch_one(&mut *tx)
    .await?;
    let länk = ny_registreringslänk(&mut tx, nu, AnvändarId(id)).await?;
    installationslogg::skriv(
        &mut tx,
        nu,
        aktör,
        Händelse::AnvändareSkapad,
        &epost,
        serde_json::json!({
            "epost": epost,
            "registreringslänk_giltig_till": länk.giltig_till.to_string(),
        }),
    )
    .await?;
    tx.commit().await?;
    Ok(länk)
}

async fn ny_registreringslänk(
    conn: &mut SqliteConnection,
    nu: Timestamp,
    användare: AnvändarId,
) -> Result<Registreringslänk, sqlx::Error> {
    let token = Token::ny();
    let giltig_till = nu + LÄNK_GILTIG;
    sqlx::query(
        "INSERT INTO engangslank (hash, syfte, anvandare_id, skapad, giltig_till)
         VALUES (?, 'registrering', ?, ?, ?)",
    )
    .bind(hash(&token.0))
    .bind(användare.0)
    .bind(nu.to_string())
    .bind(giltig_till.to_string())
    .execute(conn)
    .await?;
    Ok(Registreringslänk { token, giltig_till })
}

/// Användaren som en giltig, oanvänd registreringslänk hör till. Förbrukar inte länken.
pub async fn registrering(
    db: &Databas,
    klocka: &Klocka,
    token: &str,
) -> Result<Registrering, InloggningsFel> {
    let mut conn = db.pool().acquire().await?;
    let användare = giltig_länk(&mut conn, klocka.nu(), token).await?;
    let befintliga_passkeys =
        sqlx::query_scalar("SELECT credential_id FROM passkey WHERE anvandare_id = ?")
            .bind(användare.id.0)
            .fetch_all(&mut *conn)
            .await?;
    Ok(Registrering {
        användare,
        befintliga_passkeys,
    })
}

async fn giltig_länk(
    conn: &mut SqliteConnection,
    nu: Timestamp,
    token: &str,
) -> Result<Användare, InloggningsFel> {
    let rad: Option<(i64, String)> = sqlx::query_as(
        "SELECT l.anvandare_id, l.giltig_till FROM engangslank l
         JOIN anvandare a ON a.id = l.anvandare_id
         WHERE l.hash = ? AND l.anvand IS NULL AND a.avaktiverad IS NULL",
    )
    .bind(hash(token))
    .fetch_optional(&mut *conn)
    .await?;
    let Some((id, giltig_till)) = rad else {
        return Err(InloggningsFel::OgiltigLänk);
    };
    if nu >= tidpunkt(&giltig_till)? {
        return Err(InloggningsFel::OgiltigLänk);
    }
    användare(conn, AnvändarId(id))
        .await?
        .ok_or(InloggningsFel::OgiltigLänk)
}

/// Förbrukar registreringslänken, sparar passkeyn och loggar in användaren.
pub async fn slutför_registrering(
    db: &Databas,
    klocka: &Klocka,
    token: &str,
    passkey: NyPasskey,
) -> Result<Session, InloggningsFel> {
    let nu = klocka.nu();
    let mut tx = db.pool().begin_with("BEGIN IMMEDIATE").await?;
    let användare = giltig_länk(&mut tx, nu, token).await?;
    sqlx::query("UPDATE engangslank SET anvand = ? WHERE hash = ?")
        .bind(nu.to_string())
        .bind(hash(token))
        .execute(&mut *tx)
        .await?;
    let finns: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM passkey WHERE credential_id = ?)")
            .bind(&passkey.credential_id)
            .fetch_one(&mut *tx)
            .await?;
    if finns {
        return Err(InloggningsFel::PasskeyFinns);
    }
    sqlx::query(
        "INSERT INTO passkey (credential_id, anvandare_id, namn, passkey, skapad)
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(&passkey.credential_id)
    .bind(användare.id.0)
    .bind(passkey.namn.trim())
    .bind(&passkey.passkey)
    .bind(nu.to_string())
    .execute(&mut *tx)
    .await?;
    let session = ny_session(&mut tx, nu, användare.id).await?;
    tx.commit().await?;
    Ok(session)
}

/// Användaren med e-postadressen `epost` och hens passkeys, om användaren finns och är aktiv.
pub async fn inloggning_för(
    db: &Databas,
    epost: &str,
) -> Result<Option<Inloggningsgrund>, InloggningsFel> {
    let Ok(epost) = maya_domain::epost::normalisera(epost) else {
        return Ok(None);
    };
    let mut conn = db.pool().acquire().await?;
    let id: Option<i64> =
        sqlx::query_scalar("SELECT id FROM anvandare WHERE epost = ? AND avaktiverad IS NULL")
            .bind(&epost)
            .fetch_optional(&mut *conn)
            .await?;
    let Some(id) = id else { return Ok(None) };
    let Some(användare) = användare(&mut conn, AnvändarId(id)).await? else {
        return Ok(None);
    };
    let passkeys = sqlx::query_scalar(
        "SELECT passkey FROM passkey WHERE anvandare_id = ? ORDER BY skapad, credential_id",
    )
    .bind(id)
    .fetch_all(&mut *conn)
    .await?;
    Ok(Some(Inloggningsgrund {
        användare,
        passkeys,
    }))
}

/// Loggar in användaren efter en lyckad inloggningsceremoni med passkeyn `credential_id`.
/// `uppdaterad` är passkeyn som JSON när ceremonin har ändrat den (räknare, backupflaggor).
pub async fn logga_in(
    db: &Databas,
    klocka: &Klocka,
    användare: AnvändarId,
    credential_id: &[u8],
    uppdaterad: Option<String>,
) -> Result<Session, InloggningsFel> {
    let nu = klocka.nu();
    let mut tx = db.pool().begin_with("BEGIN IMMEDIATE").await?;
    let träff = sqlx::query(
        "UPDATE passkey SET senast_anvand = ?1, passkey = coalesce(?2, passkey)
         WHERE credential_id = ?3 AND anvandare_id = ?4
           AND anvandare_id IN (SELECT id FROM anvandare WHERE avaktiverad IS NULL)",
    )
    .bind(nu.to_string())
    .bind(uppdaterad)
    .bind(credential_id)
    .bind(användare.0)
    .execute(&mut *tx)
    .await?;
    if träff.rows_affected() != 1 {
        return Err(InloggningsFel::OkändPasskey);
    }
    let session = ny_session(&mut tx, nu, användare).await?;
    tx.commit().await?;
    Ok(session)
}

async fn ny_session(
    conn: &mut SqliteConnection,
    nu: Timestamp,
    användare: AnvändarId,
) -> Result<Session, sqlx::Error> {
    let token = Token::ny();
    sqlx::query(
        "INSERT INTO session (hash, anvandare_id, skapad, senast_aktiv, passkey_anvand)
         VALUES (?1, ?2, ?3, ?3, ?3)",
    )
    .bind(hash(&token.0))
    .bind(användare.0)
    .bind(nu.to_string())
    .execute(conn)
    .await?;
    Ok(Session { token })
}

/// Den inloggade som sessionstoken hör till, om sessionen gäller. Sessionen löper ut efter
/// [`SESSION_INAKTIV`] utan aktivitet och senast efter [`SESSION_LÄNGST`]; varje anrop räknas
/// som aktivitet.
pub async fn session(
    db: &Databas,
    klocka: &Klocka,
    token: &str,
) -> Result<Option<Inloggad>, InloggningsFel> {
    let nu = klocka.nu();
    let hash = hash(token);
    let mut tx = db.pool().begin_with("BEGIN IMMEDIATE").await?;
    let rad: Option<(i64, String, String, String)> = sqlx::query_as(
        "SELECT s.anvandare_id, s.skapad, s.senast_aktiv, s.passkey_anvand FROM session s
         JOIN anvandare a ON a.id = s.anvandare_id
         WHERE s.hash = ? AND a.avaktiverad IS NULL",
    )
    .bind(&hash)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((id, skapad, senast_aktiv, passkey_använd)) = rad else {
        return Ok(None);
    };
    if nu >= tidpunkt(&senast_aktiv)? + SESSION_INAKTIV
        || nu >= tidpunkt(&skapad)? + SESSION_LÄNGST
    {
        sqlx::query("DELETE FROM session WHERE hash = ?")
            .bind(&hash)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        return Ok(None);
    }
    sqlx::query("UPDATE session SET senast_aktiv = ? WHERE hash = ?")
        .bind(nu.to_string())
        .bind(&hash)
        .execute(&mut *tx)
        .await?;
    let användare = användare(&mut tx, AnvändarId(id)).await?;
    tx.commit().await?;
    Ok(användare.map(|användare| Inloggad {
        aktör: Aktör::webb(användare.id),
        användare,
        passkey_använd: tidpunkt(&passkey_använd).unwrap_or(nu),
    }))
}

/// Avslutar sessionen.
pub async fn logga_ut(db: &Databas, token: &str) -> Result<(), InloggningsFel> {
    sqlx::query("DELETE FROM session WHERE hash = ?")
        .bind(hash(token))
        .execute(db.pool())
        .await?;
    Ok(())
}

async fn användare(
    conn: &mut SqliteConnection,
    id: AnvändarId,
) -> Result<Option<Användare>, InloggningsFel> {
    let rad: Option<(Vec<u8>, String, String)> =
        sqlx::query_as("SELECT handle, epost, visningsnamn FROM anvandare WHERE id = ?")
            .bind(id.0)
            .fetch_optional(conn)
            .await?;
    rad.map(|(handle, epost, visningsnamn)| {
        Ok(Användare {
            id,
            handle: handle
                .try_into()
                .map_err(|_| DatabasFel::Korrupt("anvandare.handle".into()))?,
            epost,
            visningsnamn,
        })
    })
    .transpose()
}

fn tidpunkt(text: &str) -> Result<Timestamp, DatabasFel> {
    text.parse()
        .map_err(|_| DatabasFel::Korrupt(format!("tidpunkten {text}")))
}
