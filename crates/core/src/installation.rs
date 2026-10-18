//! Installationen som helhet: relying party för passkeys.

use url::Url;

use crate::{Databas, DatabasFel};

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
