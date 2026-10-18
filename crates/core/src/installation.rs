//! Installationen som helhet: relying party för passkeys.

use url::Url;

use crate::aktor::{self, Aktör, Handling};
use crate::installationslogg::{self, Händelse};
use crate::{Databas, DatabasFel, Klocka};

/// `rp_id` och origin för WebAuthn, härledda ur den publika URL:en.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelyingParty {
    pub rp_id: String,
    pub origin: String,
}

impl RelyingParty {
    pub fn från_publik_url(url: &Url) -> Self {
        RelyingParty {
            rp_id: url.host_str().unwrap_or_default().to_string(),
            origin: url.origin().ascii_serialization(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum InstallationsFel {
    #[error(
        "den publika URL:en ger rp_id {nu_rp_id} och origin {nu_origin}, men installationen \
         skapades med rp_id {sparat_rp_id} och origin {sparat_origin}. Passkeys är bundna till \
         domänen. Sätt tillbaka MAYA_PUBLIC_URL, eller byt domän med `maya domain change`."
    )]
    AnnatRpId {
        sparat_rp_id: String,
        sparat_origin: String,
        nu_rp_id: String,
        nu_origin: String,
    },
    #[error("kanalen får inte göra det här")]
    KanalenFårInte,
    #[error("installationen har redan domänen {0}; ändra MAYA_PUBLIC_URL först")]
    SammaDomän(String),
    #[error(transparent)]
    Databas(#[from] DatabasFel),
}

impl From<sqlx::Error> for InstallationsFel {
    fn from(e: sqlx::Error) -> Self {
        InstallationsFel::Databas(e.into())
    }
}

/// Sparar `rp` vid första start och vägrar sedan ett annat `rp_id` eller origin.
pub async fn kontrollera_rp(db: &Databas, rp: &RelyingParty) -> Result<(), InstallationsFel> {
    let mut tx = db.pool().begin().await?;
    let sparat: Option<(String, String)> =
        sqlx::query_as("SELECT rp_id, origin FROM installation WHERE id = 1")
            .fetch_optional(&mut *tx)
            .await?;
    match sparat {
        None => {
            sqlx::query("INSERT INTO installation (id, rp_id, origin) VALUES (1, ?, ?)")
                .bind(&rp.rp_id)
                .bind(&rp.origin)
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
            Ok(())
        }
        Some((rp_id, origin)) if rp_id == rp.rp_id && origin == rp.origin => Ok(()),
        Some((sparat_rp_id, sparat_origin)) => Err(InstallationsFel::AnnatRpId {
            sparat_rp_id,
            sparat_origin,
            nu_rp_id: rp.rp_id.clone(),
            nu_origin: rp.origin.clone(),
        }),
    }
}

/// Prövar `rp` mot det som är sparat i databasfilen `path`, utan att öppna den som [`Databas`]
/// (som migrerar och loggar starten). Körs före öppningen, så att en start med fel domän aldrig
/// hinner skriva i databasen. En fil som saknas eller inte har något sparat godtas;
/// [`kontrollera_rp`] sparar det efter öppningen.
pub async fn kontrollera_rp_i_fil(
    path: &std::path::Path,
    rp: &RelyingParty,
) -> Result<(), InstallationsFel> {
    use sqlx::Connection;
    if !path.exists() {
        return Ok(());
    }
    let options = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(path)
        .read_only(true);
    let mut conn = sqlx::SqliteConnection::connect_with(&options).await?;
    let finns: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'installation')",
    )
    .fetch_one(&mut conn)
    .await?;
    let sparat: Option<(String, String)> = if finns {
        sqlx::query_as("SELECT rp_id, origin FROM installation WHERE id = 1")
            .fetch_optional(&mut conn)
            .await?
    } else {
        None
    };
    conn.close().await?;
    match sparat {
        Some((sparat_rp_id, sparat_origin))
            if sparat_rp_id != rp.rp_id || sparat_origin != rp.origin =>
        {
            Err(InstallationsFel::AnnatRpId {
                sparat_rp_id,
                sparat_origin,
                nu_rp_id: rp.rp_id.clone(),
                nu_origin: rp.origin.clone(),
            })
        }
        _ => Ok(()),
    }
}

/// Byter installationens domän till `rp`: sparar nytt `rp_id` och origin och tömmer alla
/// passkeys och sessioner, eftersom passkeys är bundna till domänen. Användarna får sedan en
/// återställningslänk var. Bara CLI:t (ADR 0005). Händelsen skrivs i installationsloggen.
pub async fn byt_domän(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    rp: &RelyingParty,
) -> Result<(), InstallationsFel> {
    if !aktor::tillåter(aktör.kanal, Handling::HanteraAnvändare) {
        return Err(InstallationsFel::KanalenFårInte);
    }
    let mut tx = db.pool().begin_with("BEGIN IMMEDIATE").await?;
    let sparat: Option<(String, String)> =
        sqlx::query_as("SELECT rp_id, origin FROM installation WHERE id = 1")
            .fetch_optional(&mut *tx)
            .await?;
    if let Some((rp_id, origin)) = &sparat
        && *rp_id == rp.rp_id
        && *origin == rp.origin
    {
        return Err(InstallationsFel::SammaDomän(rp.origin.clone()));
    }
    sqlx::query(
        "INSERT INTO installation (id, rp_id, origin) VALUES (1, ?1, ?2)
         ON CONFLICT (id) DO UPDATE SET rp_id = ?1, origin = ?2",
    )
    .bind(&rp.rp_id)
    .bind(&rp.origin)
    .execute(&mut *tx)
    .await?;
    let passkeys = sqlx::query("DELETE FROM passkey")
        .execute(&mut *tx)
        .await?
        .rows_affected();
    let sessioner = sqlx::query("DELETE FROM session")
        .execute(&mut *tx)
        .await?
        .rows_affected();
    let (från_rp_id, från_origin) = sparat.unzip();
    installationslogg::skriv(
        &mut tx,
        klocka.nu(),
        aktör,
        Händelse::DomänBytt,
        &rp.rp_id,
        serde_json::json!({
            "från_rp_id": från_rp_id,
            "från_origin": från_origin,
            "till_rp_id": rp.rp_id,
            "till_origin": rp.origin,
            "borttagna_passkeys": passkeys,
            "avslutade_sessioner": sessioner,
        }),
    )
    .await?;
    tx.commit().await?;
    Ok(())
}
