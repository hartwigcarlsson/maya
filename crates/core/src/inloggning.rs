//! Användare, passkeys, engångslänkar och sessioner. WebAuthn-ceremonierna görs i `maya-auth`;
//! här lagras resultatet. En passkey är ogenomskinlig JSON för `core`.

use jiff::{SignedDuration, Timestamp};
use sha2::{Digest, Sha256};
use sqlx::SqliteConnection;

use crate::aktor::{self, Aktör, AnvändarId, Handling};
use crate::foretag::FöretagsId;
use crate::installationslogg::{self, Händelse};
use crate::{Databas, DatabasFel, Klocka};
use crate::{atkomst, behandlingshistorik};

/// En session löper ut efter så här lång tid utan aktivitet.
pub const SESSION_INAKTIV: SignedDuration = SignedDuration::from_hours(7 * 24);
/// En session löper ut senast så här länge efter inloggningen.
pub const SESSION_LÄNGST: SignedDuration = SignedDuration::from_hours(30 * 24);
/// En engångslänk gäller så här länge.
pub const LÄNK_GILTIG: SignedDuration = SignedDuration::from_hours(24);
/// En passkey är färsk så här länge efter att användaren använde den i sessionen. Att hantera
/// passkeys kräver en färsk passkey.
pub const FÄRSK_PASSKEY: SignedDuration = SignedDuration::from_mins(5);

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
    #[error("använd din passkey igen; det krävs för att hantera passkeys")]
    KräverFärskPasskey,
    #[error("den sista passkeyn går inte att ta bort")]
    SistaPasskeyn,
    #[error("passkeyn måste ha ett namn")]
    TomtNamn,
    #[error("sessionen hör inte till användaren")]
    OkändSession,
    #[error("det finns ingen användare med den e-postadressen")]
    OkändAnvändare,
    #[error("användaren är avaktiverad")]
    Avaktiverad,
    #[error("användaren är inte avaktiverad")]
    InteAvaktiverad,
    /// Användaren är den sista aktiva med åtkomst till företaget (namn och orgnr).
    #[error(
        "användaren är den sista aktiva med åtkomst till {0}, som då skulle bli utan användare. \
         Ge någon annan åtkomst till företaget först."
    )]
    SistaAnvändaren(String),
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
    pub(crate) fn ny() -> Self {
        let mut bytes = [0u8; 32];
        getrandom::fill(&mut bytes).expect("slumptal från operativsystemet");
        Token(hex::encode(bytes))
    }

    pub(crate) fn hemlighet(&self) -> &str {
        &self.0
    }
}

pub(crate) fn hash(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

/// En engångslänk för att registrera en passkey.
#[derive(Debug, Clone)]
pub struct Registreringslänk {
    token: Token,
    pub giltig_till: Timestamp,
}

impl Registreringslänk {
    pub(crate) fn ny(token: Token, giltig_till: Timestamp) -> Self {
        Registreringslänk { token, giltig_till }
    }

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

/// Den som registrerar en passkey: en befintlig användare, eller en inbjuden som blir
/// användare när registreringen slutförs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Registrant {
    /// WebAuthn user handle: 16 slumpade byte.
    pub handle: [u8; 16],
    pub epost: String,
    pub visningsnamn: String,
}

impl From<&Användare> for Registrant {
    fn from(a: &Användare) -> Self {
        Registrant {
            handle: a.handle,
            epost: a.epost.clone(),
            visningsnamn: a.visningsnamn.clone(),
        }
    }
}

/// Det en registreringsceremoni behöver veta.
#[derive(Debug, Clone)]
pub struct Registrering {
    pub användare: Registrant,
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
    /// SHA-256 av sessionstoken.
    session: Vec<u8>,
}

impl Inloggad {
    /// Sessionens id i sessionslistan.
    pub fn session_id(&self) -> String {
        hex::encode(&self.session)
    }

    /// Om användaren har använt en passkey i sessionen de senaste [`FÄRSK_PASSKEY`].
    pub fn har_färsk_passkey(&self, klocka: &Klocka) -> bool {
        klocka.nu() < self.passkey_använd + FÄRSK_PASSKEY
    }

    fn kräv_färsk_passkey(&self, klocka: &Klocka) -> Result<(), InloggningsFel> {
        if self.har_färsk_passkey(klocka) {
            Ok(())
        } else {
            Err(InloggningsFel::KräverFärskPasskey)
        }
    }
}

/// En av användarens passkeys.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PasskeyRad {
    /// Credential-id:t i hex.
    pub id: String,
    pub namn: String,
    pub skapad: Timestamp,
    pub senast_använd: Option<Timestamp>,
}

/// En av användarens sessioner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRad {
    pub id: String,
    pub skapad: Timestamp,
    pub senast_aktiv: Timestamp,
    /// User-Agent när sessionen skapades.
    pub enhet: Option<String>,
    /// Sessionen som frågar.
    pub denna: bool,
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
    let länk = ny_länk(&mut tx, nu, AnvändarId(id), Syfte::Registrering).await?;
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

/// Vad en engångslänk är till för. Båda ger en ny passkey.
#[derive(Debug, Clone, Copy)]
enum Syfte {
    Registrering,
    Återställning,
}

impl Syfte {
    fn namn(self) -> &'static str {
        match self {
            Syfte::Registrering => "registrering",
            Syfte::Återställning => "aterstallning",
        }
    }
}

async fn ny_länk(
    conn: &mut SqliteConnection,
    nu: Timestamp,
    användare: AnvändarId,
    syfte: Syfte,
) -> Result<Registreringslänk, sqlx::Error> {
    let token = Token::ny();
    let giltig_till = nu + LÄNK_GILTIG;
    sqlx::query(
        "INSERT INTO engangslank (hash, syfte, anvandare_id, skapad, giltig_till)
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(hash(&token.0))
    .bind(syfte.namn())
    .bind(användare.0)
    .bind(nu.to_string())
    .bind(giltig_till.to_string())
    .execute(conn)
    .await?;
    Ok(Registreringslänk { token, giltig_till })
}

/// Ger en användare som har tappat alla sina passkeys en återställningslänk, där hen
/// registrerar en ny. Bara CLI:t (ADR 0005). Händelsen skrivs i installationsloggen.
pub async fn återställ_användare(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    epost: &str,
) -> Result<Registreringslänk, InloggningsFel> {
    let (mut tx, id, epost) = hela_användaren(db, aktör, epost).await?;
    if avaktiverad(&mut tx, id).await? {
        return Err(InloggningsFel::Avaktiverad);
    }
    let nu = klocka.nu();
    let länk = ny_länk(&mut tx, nu, id, Syfte::Återställning).await?;
    installationslogg::skriv(
        &mut tx,
        nu,
        aktör,
        Händelse::AnvändareÅterställd,
        &epost,
        serde_json::json!({
            "epost": epost,
            "återställningslänk_giltig_till": länk.giltig_till.to_string(),
        }),
    )
    .await?;
    tx.commit().await?;
    Ok(länk)
}

/// Avaktiverar en användare: hen kan inte logga in, och hens sessioner och oanvända länkar
/// upphör. Användaren raderas aldrig, och åtkomsten står kvar. Avaktiveringen loggas i
/// installationsloggen och i varje företag som användaren har åtkomst till, och nekas när
/// användaren är den sista aktiva i något av dem. Bara CLI:t (ADR 0005).
pub async fn avaktivera_användare(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    epost: &str,
) -> Result<(), InloggningsFel> {
    let (mut tx, id, epost) = hela_användaren(db, aktör, epost).await?;
    if avaktiverad(&mut tx, id).await? {
        return Err(InloggningsFel::Avaktiverad);
    }
    // Ett företag blir aldrig utan aktiv användare.
    let ensam: Option<(String, String)> = sqlx::query_as(
        "SELECT f.namn, f.orgnr FROM atkomst a JOIN foretag f ON f.id = a.foretag_id
         WHERE a.anvandare_id = ?1 AND NOT EXISTS (
             SELECT 1 FROM atkomst b JOIN anvandare u ON u.id = b.anvandare_id
             WHERE b.foretag_id = a.foretag_id AND b.anvandare_id <> ?1
               AND u.avaktiverad IS NULL)
         ORDER BY f.namn, f.id LIMIT 1",
    )
    .bind(id.0)
    .fetch_optional(&mut *tx)
    .await?;
    if let Some((namn, orgnr)) = ensam {
        let orgnr =
            maya_domain::orgnr::Organisationsnummer::tolka(&orgnr).map_or(orgnr, |o| o.to_string());
        return Err(InloggningsFel::SistaAnvändaren(
            format!("{namn} ({orgnr})"),
        ));
    }
    let företag: Vec<i64> =
        sqlx::query_scalar("SELECT foretag_id FROM atkomst WHERE anvandare_id = ? ORDER BY 1")
            .bind(id.0)
            .fetch_all(&mut *tx)
            .await?;
    for företag in företag {
        behandlingshistorik::skriv(
            &mut tx,
            FöretagsId(företag),
            klocka.nu(),
            aktör,
            behandlingshistorik::Händelse::AnvändareAvaktiverad,
            &epost,
            serde_json::json!({ "användare": id.0, "epost": epost }),
        )
        .await?;
    }
    let nu = klocka.nu().to_string();
    sqlx::query("UPDATE anvandare SET avaktiverad = ? WHERE id = ?")
        .bind(&nu)
        .bind(id.0)
        .execute(&mut *tx)
        .await?;
    let sessioner = sqlx::query("DELETE FROM session WHERE anvandare_id = ?")
        .bind(id.0)
        .execute(&mut *tx)
        .await?
        .rows_affected();
    let länkar =
        sqlx::query("UPDATE engangslank SET anvand = ? WHERE anvandare_id = ? AND anvand IS NULL")
            .bind(&nu)
            .bind(id.0)
            .execute(&mut *tx)
            .await?
            .rows_affected();
    installationslogg::skriv(
        &mut tx,
        klocka.nu(),
        aktör,
        Händelse::AnvändareAvaktiverad,
        &epost,
        serde_json::json!({
            "epost": epost,
            "avslutade_sessioner": sessioner,
            "upphävda_länkar": länkar,
        }),
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

/// Återaktiverar en avaktiverad användare: hen kan logga in med sina passkeys igen. Bara
/// CLI:t (ADR 0005).
pub async fn återaktivera_användare(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    epost: &str,
) -> Result<(), InloggningsFel> {
    let (mut tx, id, epost) = hela_användaren(db, aktör, epost).await?;
    if !avaktiverad(&mut tx, id).await? {
        return Err(InloggningsFel::InteAvaktiverad);
    }
    sqlx::query("UPDATE anvandare SET avaktiverad = NULL WHERE id = ?")
        .bind(id.0)
        .execute(&mut *tx)
        .await?;
    installationslogg::skriv(
        &mut tx,
        klocka.nu(),
        aktör,
        Händelse::AnvändareÅteraktiverad,
        &epost,
        serde_json::json!({ "epost": epost }),
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

/// Kontrollerar att aktören får hantera användare som helhet och slår upp användaren med
/// e-postadressen `epost` i en ny transaktion.
async fn hela_användaren(
    db: &Databas,
    aktör: &Aktör,
    epost: &str,
) -> Result<(sqlx::Transaction<'static, sqlx::Sqlite>, AnvändarId, String), InloggningsFel> {
    if !aktor::tillåter(aktör.kanal, Handling::HanteraAnvändare) {
        return Err(InloggningsFel::KanalenFårInte);
    }
    let epost = maya_domain::epost::normalisera(epost)?;
    let mut tx = db.pool().begin_with("BEGIN IMMEDIATE").await?;
    let id: Option<i64> = sqlx::query_scalar("SELECT id FROM anvandare WHERE epost = ?")
        .bind(&epost)
        .fetch_optional(&mut *tx)
        .await?;
    let id = id.ok_or(InloggningsFel::OkändAnvändare)?;
    Ok((tx, AnvändarId(id), epost))
}

async fn avaktiverad(conn: &mut SqliteConnection, id: AnvändarId) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar("SELECT avaktiverad IS NOT NULL FROM anvandare WHERE id = ?")
        .bind(id.0)
        .fetch_one(conn)
        .await
}

/// Den som en giltig, oanvänd registrerings-, återställnings- eller inbjudningslänk hör till.
/// Förbrukar inte länken.
pub async fn registrering(
    db: &Databas,
    klocka: &Klocka,
    token: &str,
) -> Result<Registrering, InloggningsFel> {
    let mut conn = db.pool().acquire().await?;
    let nu = klocka.nu();
    let användare = match giltig_länk(&mut conn, nu, token).await {
        Err(InloggningsFel::OgiltigLänk) => {
            let inbjudan = atkomst::giltig_inbjudan(&mut conn, nu, token).await?;
            return Ok(Registrering {
                användare: inbjudan.registrant,
                befintliga_passkeys: Vec::new(),
            });
        }
        svar => svar?,
    };
    let befintliga_passkeys =
        sqlx::query_scalar("SELECT credential_id FROM passkey WHERE anvandare_id = ?")
            .bind(användare.id.0)
            .fetch_all(&mut *conn)
            .await?;
    Ok(Registrering {
        användare: Registrant::from(&användare),
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
    enhet: Option<&str>,
) -> Result<Session, InloggningsFel> {
    let nu = klocka.nu();
    let mut tx = db.pool().begin_with("BEGIN IMMEDIATE").await?;
    let användare = match giltig_länk(&mut tx, nu, token).await {
        Ok(användare) => {
            sqlx::query("UPDATE engangslank SET anvand = ? WHERE hash = ?")
                .bind(nu.to_string())
                .bind(hash(token))
                .execute(&mut *tx)
                .await?;
            användare.id
        }
        Err(InloggningsFel::OgiltigLänk) => atkomst::använd_inbjudan(&mut tx, nu, token).await?,
        Err(e) => return Err(e),
    };
    spara_passkey(&mut tx, nu, användare, &passkey).await?;
    let session = ny_session(&mut tx, nu, användare, enhet).await?;
    tx.commit().await?;
    Ok(session)
}

async fn spara_passkey(
    conn: &mut SqliteConnection,
    nu: Timestamp,
    användare: AnvändarId,
    passkey: &NyPasskey,
) -> Result<(), InloggningsFel> {
    let namn = passkey.namn.trim();
    if namn.is_empty() {
        return Err(InloggningsFel::TomtNamn);
    }
    let finns: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM passkey WHERE credential_id = ?)")
            .bind(&passkey.credential_id)
            .fetch_one(&mut *conn)
            .await?;
    if finns {
        return Err(InloggningsFel::PasskeyFinns);
    }
    sqlx::query(
        "INSERT INTO passkey (credential_id, anvandare_id, namn, passkey, skapad)
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(&passkey.credential_id)
    .bind(användare.0)
    .bind(namn)
    .bind(&passkey.passkey)
    .bind(nu.to_string())
    .execute(conn)
    .await?;
    Ok(())
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
    enhet: Option<&str>,
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
    let session = ny_session(&mut tx, nu, användare, enhet).await?;
    tx.commit().await?;
    Ok(session)
}

async fn ny_session(
    conn: &mut SqliteConnection,
    nu: Timestamp,
    användare: AnvändarId,
    enhet: Option<&str>,
) -> Result<Session, sqlx::Error> {
    let token = Token::ny();
    sqlx::query(
        "INSERT INTO session (hash, anvandare_id, skapad, senast_aktiv, passkey_anvand, enhet)
         VALUES (?1, ?2, ?3, ?3, ?3, ?4)",
    )
    .bind(hash(&token.0))
    .bind(användare.0)
    .bind(nu.to_string())
    .bind(enhet.map(|e| e.chars().take(300).collect::<String>()))
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
    if utgången(nu, tidpunkt(&skapad)?, tidpunkt(&senast_aktiv)?) {
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
        session: hash,
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

/// Användarens passkeys i den ordning de registrerades.
pub async fn passkeys(
    db: &Databas,
    inloggad: &Inloggad,
) -> Result<Vec<PasskeyRad>, InloggningsFel> {
    let rader: Vec<(Vec<u8>, String, String, Option<String>)> = sqlx::query_as(
        "SELECT credential_id, namn, skapad, senast_anvand FROM passkey
         WHERE anvandare_id = ? ORDER BY skapad, rowid",
    )
    .bind(inloggad.användare.id.0)
    .fetch_all(db.pool())
    .await?;
    rader
        .into_iter()
        .map(|(id, namn, skapad, senast_använd)| {
            Ok(PasskeyRad {
                id: hex::encode(id),
                namn,
                skapad: tidpunkt(&skapad)?,
                senast_använd: senast_använd.as_deref().map(tidpunkt).transpose()?,
            })
        })
        .collect()
}

/// Det en ceremoni behöver för att lägga till en passkey för den inloggade. Kräver en färsk
/// passkey.
pub async fn ny_passkey(
    db: &Databas,
    klocka: &Klocka,
    inloggad: &Inloggad,
) -> Result<Registrering, InloggningsFel> {
    inloggad.kräv_färsk_passkey(klocka)?;
    let befintliga_passkeys =
        sqlx::query_scalar("SELECT credential_id FROM passkey WHERE anvandare_id = ?")
            .bind(inloggad.användare.id.0)
            .fetch_all(db.pool())
            .await?;
    Ok(Registrering {
        användare: Registrant::from(&inloggad.användare),
        befintliga_passkeys,
    })
}

/// Sparar en passkey som den inloggade har registrerat. Kräver en färsk passkey.
pub async fn lägg_till_passkey(
    db: &Databas,
    klocka: &Klocka,
    inloggad: &Inloggad,
    passkey: NyPasskey,
) -> Result<(), InloggningsFel> {
    inloggad.kräv_färsk_passkey(klocka)?;
    let mut tx = db.pool().begin_with("BEGIN IMMEDIATE").await?;
    spara_passkey(&mut tx, klocka.nu(), inloggad.användare.id, &passkey).await?;
    tx.commit().await?;
    Ok(())
}

/// Byter namn på en av den inloggades passkeys. Kräver en färsk passkey.
pub async fn byt_namn_på_passkey(
    db: &Databas,
    klocka: &Klocka,
    inloggad: &Inloggad,
    id: &str,
    namn: &str,
) -> Result<(), InloggningsFel> {
    inloggad.kräv_färsk_passkey(klocka)?;
    let namn = namn.trim();
    if namn.is_empty() {
        return Err(InloggningsFel::TomtNamn);
    }
    let id = hex::decode(id).map_err(|_| InloggningsFel::OkändPasskey)?;
    let träff =
        sqlx::query("UPDATE passkey SET namn = ? WHERE credential_id = ? AND anvandare_id = ?")
            .bind(namn)
            .bind(&id)
            .bind(inloggad.användare.id.0)
            .execute(db.pool())
            .await?;
    if träff.rows_affected() != 1 {
        return Err(InloggningsFel::OkändPasskey);
    }
    Ok(())
}

/// Tar bort en av den inloggades passkeys, men aldrig den sista. Kräver en färsk passkey.
pub async fn ta_bort_passkey(
    db: &Databas,
    klocka: &Klocka,
    inloggad: &Inloggad,
    id: &str,
) -> Result<(), InloggningsFel> {
    inloggad.kräv_färsk_passkey(klocka)?;
    let id = hex::decode(id).map_err(|_| InloggningsFel::OkändPasskey)?;
    let mut tx = db.pool().begin_with("BEGIN IMMEDIATE").await?;
    let antal: i64 = sqlx::query_scalar("SELECT count(*) FROM passkey WHERE anvandare_id = ?")
        .bind(inloggad.användare.id.0)
        .fetch_one(&mut *tx)
        .await?;
    let träff = sqlx::query("DELETE FROM passkey WHERE credential_id = ? AND anvandare_id = ?")
        .bind(&id)
        .bind(inloggad.användare.id.0)
        .execute(&mut *tx)
        .await?;
    if träff.rows_affected() != 1 {
        return Err(InloggningsFel::OkändPasskey);
    }
    if antal <= 1 {
        return Err(InloggningsFel::SistaPasskeyn);
    }
    tx.commit().await?;
    Ok(())
}

/// Den inloggades passkeys som JSON, för en ceremoni som bekräftar att användaren har en av
/// dem till hands.
pub async fn bekräftelse_för(
    db: &Databas,
    inloggad: &Inloggad,
) -> Result<Inloggningsgrund, InloggningsFel> {
    let passkeys = sqlx::query_scalar(
        "SELECT passkey FROM passkey WHERE anvandare_id = ? ORDER BY skapad, credential_id",
    )
    .bind(inloggad.användare.id.0)
    .fetch_all(db.pool())
    .await?;
    Ok(Inloggningsgrund {
        användare: inloggad.användare.clone(),
        passkeys,
    })
}

/// Den inloggade har använt passkeyn `credential_id` i sessionen: passkeyn blir färsk.
/// `uppdaterad` som i [`logga_in`].
pub async fn bekräfta_passkey(
    db: &Databas,
    klocka: &Klocka,
    inloggad: &Inloggad,
    credential_id: &[u8],
    uppdaterad: Option<String>,
) -> Result<(), InloggningsFel> {
    let nu = klocka.nu().to_string();
    let mut tx = db.pool().begin_with("BEGIN IMMEDIATE").await?;
    let träff = sqlx::query(
        "UPDATE passkey SET senast_anvand = ?1, passkey = coalesce(?2, passkey)
         WHERE credential_id = ?3 AND anvandare_id = ?4",
    )
    .bind(&nu)
    .bind(uppdaterad)
    .bind(credential_id)
    .bind(inloggad.användare.id.0)
    .execute(&mut *tx)
    .await?;
    if träff.rows_affected() != 1 {
        return Err(InloggningsFel::OkändPasskey);
    }
    let träff =
        sqlx::query("UPDATE session SET passkey_anvand = ? WHERE hash = ? AND anvandare_id = ?")
            .bind(&nu)
            .bind(&inloggad.session)
            .bind(inloggad.användare.id.0)
            .execute(&mut *tx)
            .await?;
    if träff.rows_affected() != 1 {
        return Err(InloggningsFel::OkändSession);
    }
    tx.commit().await?;
    Ok(())
}

/// Den inloggades sessioner som inte har gått ut, senast aktiv först.
pub async fn sessioner(
    db: &Databas,
    klocka: &Klocka,
    inloggad: &Inloggad,
) -> Result<Vec<SessionRad>, InloggningsFel> {
    let nu = klocka.nu();
    let rader: Vec<(Vec<u8>, String, String, Option<String>)> = sqlx::query_as(
        "SELECT hash, skapad, senast_aktiv, enhet FROM session
         WHERE anvandare_id = ? ORDER BY senast_aktiv DESC, skapad DESC",
    )
    .bind(inloggad.användare.id.0)
    .fetch_all(db.pool())
    .await?;
    let mut sessioner = Vec::new();
    for (hash, skapad, senast_aktiv, enhet) in rader {
        let (skapad, senast_aktiv) = (tidpunkt(&skapad)?, tidpunkt(&senast_aktiv)?);
        if utgången(nu, skapad, senast_aktiv) {
            continue;
        }
        sessioner.push(SessionRad {
            denna: hash == inloggad.session,
            id: hex::encode(hash),
            skapad,
            senast_aktiv,
            enhet,
        });
    }
    Ok(sessioner)
}

/// Om en session har gått ut: [`SESSION_INAKTIV`] utan aktivitet eller [`SESSION_LÄNGST`]
/// sedan den skapades.
fn utgången(nu: Timestamp, skapad: Timestamp, senast_aktiv: Timestamp) -> bool {
    nu >= senast_aktiv + SESSION_INAKTIV || nu >= skapad + SESSION_LÄNGST
}

/// Avslutar en av den inloggades sessioner. Den som har sessionen är utloggad direkt.
pub async fn avsluta_session(
    db: &Databas,
    inloggad: &Inloggad,
    id: &str,
) -> Result<(), InloggningsFel> {
    let hash = hex::decode(id).map_err(|_| InloggningsFel::OkändSession)?;
    let träff = sqlx::query("DELETE FROM session WHERE hash = ? AND anvandare_id = ?")
        .bind(hash)
        .bind(inloggad.användare.id.0)
        .execute(db.pool())
        .await?;
    if träff.rows_affected() != 1 {
        return Err(InloggningsFel::OkändSession);
    }
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

pub(crate) fn tidpunkt(text: &str) -> Result<Timestamp, DatabasFel> {
    text.parse()
        .map_err(|_| DatabasFel::Korrupt(format!("tidpunkten {text}")))
}
