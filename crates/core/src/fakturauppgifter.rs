//! Företagets fakturauppgifter: betalsätten, OCR på eller av, logotypen, svarsadressen och
//! standardmeddelandet för fakturamejl. Reglerna finns i [`maya_domain::fakturauppgifter`];
//! varje ändring loggas med från- och till-värde (H2), OCR-inställningen som en egen typ.

use maya_domain::fakturauppgifter::{Fakturauppgifter, Fältfel, kontrollera_logotyp};
use maya_domain::orgnr::Organisationsnummer;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::SqliteConnection;

use crate::aktor::{Aktör, Handling};
use crate::atkomst::{self, Ärende, ÅtkomstFel};
use crate::behandlingshistorik::Händelse;
use crate::foretag::FöretagsId;
use crate::{Databas, DatabasFel, Klocka, underlag};

#[derive(Debug, thiserror::Error)]
pub enum FakturauppgiftsFel {
    /// Formuläret klarar inte kontrollen i `domain`; samma koder som formuläret visar.
    #[error("formuläret är inte rätt ifyllt")]
    Ogiltigt(Vec<Fältfel>),
    #[error(transparent)]
    Åtkomst(#[from] ÅtkomstFel),
}

impl From<sqlx::Error> for FakturauppgiftsFel {
    fn from(e: sqlx::Error) -> Self {
        FakturauppgiftsFel::Åtkomst(e.into())
    }
}

impl From<DatabasFel> for FakturauppgiftsFel {
    fn from(e: DatabasFel) -> Self {
        FakturauppgiftsFel::Åtkomst(e.into())
    }
}

/// Logotypen som den är lagrad, utan innehållet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Logotyp {
    pub filnamn: String,
    /// `image/png`, `image/jpeg` eller `image/svg+xml`.
    pub mime: String,
    /// SHA-256 av filen, i hex.
    pub sha256: String,
    pub storlek: u64,
}

/// Logotypens fil.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Logotypfil {
    pub filnamn: String,
    pub mime: String,
    pub innehåll: Vec<u8>,
}

/// Företagets fakturauppgifter och logotyp.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Fakturainställningar {
    /// Kontrollerade; tomma för ett företag som inte har fyllt i dem.
    pub uppgifter: Fakturauppgifter,
    pub logotyp: Option<Logotyp>,
}

/// Företagets fakturauppgifter och logotyp.
pub async fn fakturauppgifterna(
    db: &Databas,
    aktör: &Aktör,
    id: FöretagsId,
) -> Result<Fakturainställningar, ÅtkomstFel> {
    let mut tx = db.pool().begin().await?;
    atkomst::kräv(&mut tx, aktör, id, Handling::LäsaFöretag).await?;
    Ok(läs_i(&mut tx, id).await?)
}

/// Fakturauppgifterna i en pågående transaktion, t.ex. för att kopiera in dem när en faktura
/// utfärdas.
pub(crate) async fn läs_i(
    conn: &mut SqliteConnection,
    id: FöretagsId,
) -> Result<Fakturainställningar, DatabasFel> {
    type Rad = (
        String,
        String,
        String,
        String,
        bool,
        String,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<i64>,
    );
    let rad: Option<Rad> = sqlx::query_as(
        "SELECT bankgiro, plusgiro, iban, bic, ocr, svarsadress, standardmeddelande,
                logotyp_filnamn, logotyp_mime, logotyp_sha256, length(logotyp)
         FROM fakturauppgifter WHERE foretag_id = ?",
    )
    .bind(id.0)
    .fetch_optional(conn)
    .await?;
    let Some((
        bankgiro,
        plusgiro,
        iban,
        bic,
        ocr,
        svarsadress,
        standardmeddelande,
        filnamn,
        mime,
        sha256,
        storlek,
    )) = rad
    else {
        return Ok(Fakturainställningar::default());
    };
    let logotyp = match (filnamn, mime, sha256, storlek) {
        (Some(filnamn), Some(mime), Some(sha256), Some(storlek)) => Some(Logotyp {
            filnamn,
            mime,
            sha256,
            storlek: storlek as u64,
        }),
        _ => None,
    };
    Ok(Fakturainställningar {
        uppgifter: Fakturauppgifter {
            bankgiro,
            plusgiro,
            iban,
            bic,
            ocr,
            svarsadress,
            standardmeddelande,
        },
        logotyp,
    })
}

/// Logotypens fil, eller `None` om företaget inte har någon.
pub async fn logotypen(
    db: &Databas,
    aktör: &Aktör,
    id: FöretagsId,
) -> Result<Option<Logotypfil>, ÅtkomstFel> {
    let mut tx = db.pool().begin().await?;
    atkomst::kräv(&mut tx, aktör, id, Handling::LäsaFöretag).await?;
    Ok(logotypen_i(&mut tx, id).await?)
}

/// Logotypens fil i en pågående transaktion, t.ex. till fakturans PDF.
pub(crate) async fn logotypen_i(
    conn: &mut SqliteConnection,
    id: FöretagsId,
) -> Result<Option<Logotypfil>, sqlx::Error> {
    let rad: Option<(String, String, Vec<u8>)> = sqlx::query_as(
        "SELECT logotyp_filnamn, logotyp_mime, logotyp FROM fakturauppgifter
         WHERE foretag_id = ? AND logotyp IS NOT NULL",
    )
    .bind(id.0)
    .fetch_optional(conn)
    .await?;
    Ok(rad.map(|(filnamn, mime, innehåll)| Logotypfil {
        filnamn,
        mime,
        innehåll,
    }))
}

/// Ändrar betalsätten, OCR-inställningen, svarsadressen och standardmeddelandet. Det som
/// ändrades loggas som `fakturauppgifter ändrade` med från- och till-värde per fält, och en
/// ändrad OCR-inställning som `ocr-inställning ändrad`; utan ändring loggas inget.
pub async fn ändra(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    id: FöretagsId,
    formulär: &Fakturauppgifter,
) -> Result<(), FakturauppgiftsFel> {
    let mut ärende = Ärende::börja(db, klocka, aktör, id, Handling::ÄndraFöretag).await?;
    let nya = formulär
        .kontrollera()
        .map_err(FakturauppgiftsFel::Ogiltigt)?;
    let gamla = läs_i(ärende.conn(), id).await?.uppgifter;
    let mut ändrat = serde_json::Map::new();
    for (fält, från, till) in [
        ("bankgiro", &gamla.bankgiro, &nya.bankgiro),
        ("plusgiro", &gamla.plusgiro, &nya.plusgiro),
        ("iban", &gamla.iban, &nya.iban),
        ("bic", &gamla.bic, &nya.bic),
        ("svarsadress", &gamla.svarsadress, &nya.svarsadress),
        (
            "standardmeddelande",
            &gamla.standardmeddelande,
            &nya.standardmeddelande,
        ),
    ] {
        if från != till {
            ändrat.insert(fält.into(), json!({ "från": från, "till": till }));
        }
    }
    let ocr_ändrad = gamla.ocr != nya.ocr;
    if ändrat.is_empty() && !ocr_ändrad {
        return Ok(());
    }
    sqlx::query(
        "INSERT INTO fakturauppgifter
             (foretag_id, bankgiro, plusgiro, iban, bic, ocr, svarsadress, standardmeddelande)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT (foretag_id) DO UPDATE SET
             bankgiro = excluded.bankgiro, plusgiro = excluded.plusgiro,
             iban = excluded.iban, bic = excluded.bic, ocr = excluded.ocr,
             svarsadress = excluded.svarsadress,
             standardmeddelande = excluded.standardmeddelande",
    )
    .bind(id.0)
    .bind(&nya.bankgiro)
    .bind(&nya.plusgiro)
    .bind(&nya.iban)
    .bind(&nya.bic)
    .bind(nya.ocr)
    .bind(&nya.svarsadress)
    .bind(&nya.standardmeddelande)
    .execute(ärende.conn())
    .await?;
    let orgnr = orgnr(ärende.conn(), id).await?;
    if !ändrat.is_empty() {
        ärende
            .logga(
                Händelse::FakturauppgifterÄndrade,
                &orgnr,
                Value::Object(ändrat),
            )
            .await?;
    }
    if ocr_ändrad {
        ärende
            .logga(
                Händelse::OcrInställningÄndrad,
                &orgnr,
                json!({ "från": gamla.ocr, "till": nya.ocr }),
            )
            .await?;
    }
    ärende.slutför().await?;
    Ok(())
}

/// Lagrar logotypen till fakturamallen, eller byter den som finns. Bara PNG, JPG och SVG tas
/// emot, efter filens innehåll. Bytet loggas som `fakturauppgifter ändrade` med filnamn och
/// SHA-256 före och efter.
pub async fn byt_logotyp(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    id: FöretagsId,
    filnamn: &str,
    innehåll: &[u8],
) -> Result<(), FakturauppgiftsFel> {
    let mut ärende = Ärende::börja(db, klocka, aktör, id, Handling::ÄndraFöretag).await?;
    let mime = kontrollera_logotyp(innehåll).map_err(|f| FakturauppgiftsFel::Ogiltigt(vec![f]))?;
    let ny = Logotyp {
        filnamn: underlag::rensa_filnamn(filnamn),
        mime: mime.to_string(),
        sha256: hex::encode(Sha256::digest(innehåll)),
        storlek: innehåll.len() as u64,
    };
    let gammal = läs_i(ärende.conn(), id).await?.logotyp;
    sqlx::query(
        "INSERT INTO fakturauppgifter
             (foretag_id, logotyp, logotyp_mime, logotyp_filnamn, logotyp_sha256)
         VALUES (?, ?, ?, ?, ?)
         ON CONFLICT (foretag_id) DO UPDATE SET
             logotyp = excluded.logotyp, logotyp_mime = excluded.logotyp_mime,
             logotyp_filnamn = excluded.logotyp_filnamn,
             logotyp_sha256 = excluded.logotyp_sha256",
    )
    .bind(id.0)
    .bind(innehåll)
    .bind(&ny.mime)
    .bind(&ny.filnamn)
    .bind(&ny.sha256)
    .execute(ärende.conn())
    .await?;
    logga_logotyp(&mut ärende, gammal.as_ref(), Some(&ny)).await?;
    ärende.slutför().await?;
    Ok(())
}

/// Tar bort logotypen. Borttagningen loggas som `fakturauppgifter ändrade`; utan logotyp
/// händer inget.
pub async fn ta_bort_logotyp(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    id: FöretagsId,
) -> Result<(), FakturauppgiftsFel> {
    let mut ärende = Ärende::börja(db, klocka, aktör, id, Handling::ÄndraFöretag).await?;
    let Some(gammal) = läs_i(ärende.conn(), id).await?.logotyp else {
        return Ok(());
    };
    sqlx::query(
        "UPDATE fakturauppgifter SET logotyp = NULL, logotyp_mime = NULL,
             logotyp_filnamn = NULL, logotyp_sha256 = NULL
         WHERE foretag_id = ?",
    )
    .bind(id.0)
    .execute(ärende.conn())
    .await?;
    logga_logotyp(&mut ärende, Some(&gammal), None).await?;
    ärende.slutför().await?;
    Ok(())
}

async fn logga_logotyp(
    ärende: &mut Ärende,
    från: Option<&Logotyp>,
    till: Option<&Logotyp>,
) -> Result<(), FakturauppgiftsFel> {
    let värde = |l: Option<&Logotyp>| {
        l.map_or(Value::Null, |l| {
            json!({ "filnamn": l.filnamn, "mime": l.mime, "sha256": l.sha256, "storlek": l.storlek })
        })
    };
    let företag = ärende.företag;
    let orgnr = orgnr(ärende.conn(), företag).await?;
    ärende
        .logga(
            Händelse::FakturauppgifterÄndrade,
            &orgnr,
            json!({ "logotyp": { "från": värde(från), "till": värde(till) } }),
        )
        .await?;
    Ok(())
}

/// Organisationsnumret som det står i behandlingshistorikens objekt.
async fn orgnr(conn: &mut SqliteConnection, id: FöretagsId) -> Result<String, DatabasFel> {
    let siffror: String = sqlx::query_scalar("SELECT orgnr FROM foretag WHERE id = ?")
        .bind(id.0)
        .fetch_one(conn)
        .await?;
    Organisationsnummer::tolka(&siffror)
        .map(|nr| nr.to_string())
        .map_err(|_| DatabasFel::Korrupt("foretag.orgnr".into()))
}
