//! T2 under kontantmetoden (#150): obetalda fordringar och skulder bokförs vid räkenskapsårets
//! slut (BFL 5:2, krav T2).
//!
//! Dagen efter räkenskapsårets slut skapar ett bakgrundsjobb ([`kör`]), med Maya som aktör, ett
//! *T2-utkast* daterat på årets sista dag. Det har det obetalda beloppet vid årets slut
//! ([`fordran::obetalt_vid_årets_slut_i`]) på varje kund- och leverantörsfaktura med fakturadatum i året vars
//! fordran eller skuld inte är bokförd, fördelat proportionellt på fakturans konton och
//! momssatser ([`maya_domain::kontantmetoden::Obokförd::kontering`]): 1510 mot intäkt och
//! utgående moms, kostnad och ingående moms (och fiktiv moms) mot 2440. Finns inga obetalda
//! fakturor skapas inget utkast; under fakturametoden är varje fordran och skuld redan bokförd.
//!
//! Utkastet räknas om i varje ärende som ändrar företaget ([`Ärende::slutför`]), så att det
//! stämmer med fakturorna och betalningarna tills det bokförs, och blir det tomt raderar Maya
//! det. Det kan inte ändras eller raderas för hand ([`VerifikatFel::T2Utkast`]). En användare
//! bokför det ([`bokför`]): då räknas det om en sista gång, och varje faktura i det får
//! T2-verifikatet som det som bokförde dess fordran eller skuld ([`fordran::bokför_i`]).
//! Verifikatet vänds inte i det nya året (`t2_vands_inte`). En betalning, kredit eller
//! kundförlust daterad efter årets slut på en faktura som var obetald då bokförs mot 1510 eller
//! 2440, också innan T2 är bokfört ([`fordran::bokförd_på_i`]).
//!
//! Spärrarna som T2-utkastet ger mot momsdeklarationen (etapp 3) och stängningen (etapp 5)
//! byggs i de etapperna.

use jiff::civil::Date;
use maya_domain::rakenskapsar::Räkenskapsår;
use serde_json::json;
use sqlx::SqliteConnection;

use crate::aktor::{Aktör, Handling};
use crate::atkomst::{self, Ärende, ÅtkomstFel};
use crate::behandlingshistorik::Händelse;
use crate::betalning::Faktura;
use crate::fordran;
use crate::foretag::{FöretagsId, datum};
use crate::kundfaktura::KundfakturaId;
use crate::leverantorsfaktura::LeverantörsfakturaId;
use crate::verifikat::{self, Bokfört, VerifikatFel, VerifikatId};
use crate::{Databas, DatabasFel, Klocka};

#[derive(Debug, thiserror::Error)]
pub enum T2Fel {
    /// Inget T2-utkast med det id:t i företaget: det finns inte, är redan bokfört, eller har
    /// raderats av Maya för att inget längre var obetalt vid årets slut.
    #[error("T2-utkastet finns inte")]
    IngetUtkast,
    /// En faktura med fakturadatum i året är upplåst med Rätta registrering eller Rätta
    /// avskrift: utkastet tar inte med den, och T2 vänds aldrig, så den registreras om först
    /// (#151). Texten är den första fakturan, t.ex. "leverantörsfaktura 8812".
    #[error("{0} är upplåst")]
    UpplåstFaktura(String),
    /// Verifikatet kunde inte bokföras, t.ex. ett inaktivt konto.
    #[error(transparent)]
    Verifikat(VerifikatFel),
    #[error(transparent)]
    Åtkomst(#[from] ÅtkomstFel),
}

impl From<sqlx::Error> for T2Fel {
    fn from(e: sqlx::Error) -> Self {
        T2Fel::Åtkomst(e.into())
    }
}

impl From<DatabasFel> for T2Fel {
    fn from(e: DatabasFel) -> Self {
        T2Fel::Åtkomst(e.into())
    }
}

impl From<VerifikatFel> for T2Fel {
    fn from(e: VerifikatFel) -> Self {
        match e {
            VerifikatFel::Åtkomst(e) => T2Fel::Åtkomst(e),
            e => T2Fel::Verifikat(e),
        }
    }
}

/// Ett räkenskapsårs T2-verifikat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct T2 {
    pub verifikat: VerifikatId,
    pub räkenskapsår: Räkenskapsår,
    /// Bokfört av en användare; annars ett utkast som Maya räknar om.
    pub bokfört: bool,
}

/// Företagets T2-verifikat, äldsta året först.
pub async fn lista(
    db: &Databas,
    aktör: &Aktör,
    företag: FöretagsId,
) -> Result<Vec<T2>, ÅtkomstFel> {
    let mut tx = db.pool().begin().await?;
    atkomst::kräv(&mut tx, aktör, företag, Handling::LäsaFöretag).await?;
    let rader: Vec<(i64, String, String, bool)> = sqlx::query_as(
        "SELECT t2.verifikat_id, r.start, r.slut, v.bokford IS NOT NULL
         FROM t2 JOIN rakenskapsar r ON r.id = t2.rakenskapsar_id
         JOIN verifikat v ON v.id = t2.verifikat_id
         WHERE t2.foretag_id = ? ORDER BY r.start",
    )
    .bind(företag.0)
    .fetch_all(&mut *tx)
    .await?;
    Ok(rader
        .into_iter()
        .map(|(v, start, slut, bokfört)| {
            Ok(T2 {
                verifikat: VerifikatId(v),
                räkenskapsår: Räkenskapsår::befintligt(datum(&start)?, datum(&slut)?),
                bokfört,
            })
        })
        .collect::<Result<_, DatabasFel>>()?)
}

/// Bakgrundsjobbet, som Maya kör varje natt: för varje företag och varje räkenskapsår som har
/// tagit slut före i dag och saknar T2-verifikat skapas T2-utkastet, när något var obetalt vid
/// årets slut. Det är idempotent. Ger de utkast som skapades. Ett företag där det misslyckas
/// hindrar inte de andra; felet loggas och kommer tillbaka sist.
pub async fn kör(
    db: &Databas,
    klocka: &Klocka,
) -> Result<Vec<(FöretagsId, VerifikatId)>, DatabasFel> {
    let företagen: Vec<i64> = sqlx::query_scalar("SELECT id FROM foretag ORDER BY id")
        .fetch_all(db.pool())
        .await?;
    let mut skapade = Vec::new();
    let mut fel = None;
    for företag in företagen.into_iter().map(FöretagsId) {
        match skapa_i_företaget(db, klocka, företag).await {
            Ok(s) => skapade.extend(s.into_iter().map(|v| (företag, v))),
            Err(e) => {
                tracing::error!("T2-utkastet för företag {} misslyckades: {e}", företag.0);
                fel = Some(e);
            }
        }
    }
    match fel {
        Some(e) => Err(e),
        None => Ok(skapade),
    }
}

async fn skapa_i_företaget(
    db: &Databas,
    klocka: &Klocka,
    företag: FöretagsId,
) -> Result<Vec<VerifikatId>, DatabasFel> {
    let mut ärende = Ärende::för_maya(db, klocka, företag, Handling::SkapaT2Utkast)
        .await
        .map_err(|e| match e {
            ÅtkomstFel::Databas(e) => e,
            e => DatabasFel::Korrupt(e.to_string()),
        })?;
    let idag = maya_domain::tid::datum(ärende.nu);
    // Åren som har tagit slut och saknar T2-verifikat, eller har ett utkast att räkna om.
    let år: Vec<(i64, String, String)> = sqlx::query_as(
        "SELECT r.id, r.start, r.slut FROM rakenskapsar r
         WHERE r.foretag_id = ? AND r.slut < ?
           AND NOT EXISTS (SELECT 1 FROM t2 WHERE t2.rakenskapsar_id = r.id)
         ORDER BY r.start",
    )
    .bind(företag.0)
    .bind(idag.to_string())
    .fetch_all(ärende.conn())
    .await?;
    let mut skapade = Vec::new();
    for (id, start, slut) in år {
        let år = År {
            id,
            start: datum(&start)?,
            slut: datum(&slut)?,
        };
        if let Utfall::Skapat(v) = räkna_om_i(&mut ärende, &år, None).await?.0 {
            skapade.push(v);
        }
    }
    ärende.slutför().await?;
    Ok(skapade)
}

/// Bokför T2-utkastet `utkast`: räknar om det, bokför det med nästa nummer i årets V-serie och
/// gör varje faktura i det bokförd, i en transaktion, och loggar det. Bara en användare bokför.
/// Det spärras medan en faktura från året är upplåst ([`T2Fel::UpplåstFaktura`]).
pub async fn bokför(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    företag: FöretagsId,
    utkast: VerifikatId,
) -> Result<Bokfört, T2Fel> {
    let mut ärende = Ärende::börja(db, klocka, aktör, företag, Handling::BokföraT2).await?;
    let rad: Option<(i64, i64, String, String)> = sqlx::query_as(
        "SELECT t2.id, r.id, r.start, r.slut FROM t2
         JOIN rakenskapsar r ON r.id = t2.rakenskapsar_id
         JOIN verifikat v ON v.id = t2.verifikat_id
         WHERE t2.foretag_id = ? AND t2.verifikat_id = ? AND v.bokford IS NULL",
    )
    .bind(företag.0)
    .bind(utkast.0)
    .fetch_optional(ärende.conn())
    .await?;
    let (t2, år_id, start, slut) = rad.ok_or(T2Fel::IngetUtkast)?;
    let år = År {
        id: år_id,
        start: datum(&start)?,
        slut: datum(&slut)?,
    };
    let upplåst: Option<String> = sqlx::query_scalar(
        "SELECT 'kundfaktura ' || fakturanummer FROM kundfaktura
         WHERE foretag_id = ?1 AND status = 'upplast' AND fakturadatum BETWEEN ?2 AND ?3
         UNION ALL
         SELECT 'leverantörsfaktura ' || fakturanummer FROM leverantorsfaktura
         WHERE foretag_id = ?1 AND status = 'upplast' AND fakturadatum BETWEEN ?2 AND ?3
         LIMIT 1",
    )
    .bind(företag.0)
    .bind(&start)
    .bind(&slut)
    .fetch_optional(ärende.conn())
    .await?;
    if let Some(faktura) = upplåst {
        return Err(T2Fel::UpplåstFaktura(faktura));
    }
    let (utfall, poster) = räkna_om_i(&mut ärende, &år, Some((t2, utkast))).await?;
    if utfall == Utfall::Raderat {
        // Inget var obetalt längre; Maya har raderat utkastet, och det loggas.
        ärende.slutför().await?;
        return Err(T2Fel::IngetUtkast);
    }
    let rader: Vec<_> = poster
        .iter()
        .flat_map(|p| p.rader.iter().cloned())
        .collect();
    let formulär = verifikat::formulär_av(år.slut, &beskrivning(&år), &rader);
    let bokfört = verifikat::bokför_utkast_i(&mut ärende, utkast, &formulär).await?;
    for p in &poster {
        fordran::bokför_i(ärende.conn(), företag, p.faktura, bokfört.id).await?;
    }
    let summa: i64 = rader.iter().map(|r| r.debet.öre()).sum();
    ärende
        .logga(
            Händelse::T2UtkastBokfört,
            &bokfört.nummer.to_string(),
            json!({
                "t2": t2,
                "verifikat": bokfört.id.0,
                "nummer": bokfört.nummer.to_string(),
                "räkenskapsår": { "start": år.start.to_string(), "slut": år.slut.to_string() },
                "datum": år.slut.to_string(),
                "summa_öre": summa,
                "fakturor": fakturorna(&poster),
            }),
        )
        .await?;
    ärende.slutför().await?;
    Ok(bokfört)
}

/// Räknar om företagets T2-utkast som inte är bokförda, i ärendet. [`Ärende::slutför`] gör det i
/// varje ärende, så att en ändrad faktura eller betalning genast syns i utkastet. Nya utkast
/// skapas bara av jobbet ([`kör`]).
pub(crate) async fn räkna_om_utkasten_i(ärende: &mut Ärende) -> Result<(), sqlx::Error> {
    let företag = ärende.företag;
    // En databas som ännu inte har migrerats till #150 (bara i migreringstesterna, som skriver
    // med äldre scheman) har inga T2-utkast.
    let t2_finns: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 't2')",
    )
    .fetch_one(ärende.conn())
    .await?;
    if !t2_finns {
        return Ok(());
    }
    let utkast: Vec<(i64, i64, i64, String, String)> = sqlx::query_as(
        "SELECT t2.id, t2.verifikat_id, r.id, r.start, r.slut FROM t2
         JOIN rakenskapsar r ON r.id = t2.rakenskapsar_id
         JOIN verifikat v ON v.id = t2.verifikat_id
         WHERE t2.foretag_id = ? AND v.bokford IS NULL
         ORDER BY r.start",
    )
    .bind(företag.0)
    .fetch_all(ärende.conn())
    .await?;
    for (t2, v, id, start, slut) in utkast {
        let år = År {
            id,
            start: datum(&start).map_err(som_sqlx)?,
            slut: datum(&slut).map_err(som_sqlx)?,
        };
        räkna_om_i(ärende, &år, Some((t2, VerifikatId(v))))
            .await
            .map_err(som_sqlx)?;
    }
    Ok(())
}

/// Ett fel ur omräkningen som det fel som [`Ärende::slutför`] ger.
fn som_sqlx(e: DatabasFel) -> sqlx::Error {
    match e {
        DatabasFel::Sql(e) => e,
        e => sqlx::Error::Protocol(e.to_string()),
    }
}

/// Ett räkenskapsår i företaget.
struct År {
    id: i64,
    start: Date,
    slut: Date,
}

/// En faktura i T2-utkastet: det obetalda beloppet vid årets slut och dess rader.
struct Post {
    faktura: Faktura,
    nummer: String,
    motpart: String,
    /// I öre, större än noll.
    obetalt: i64,
    rader: Vec<maya_domain::verifikat::Rad>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Utfall {
    Skapat(VerifikatId),
    Oförändrat,
    Omräknat,
    Raderat,
    /// Inget utkast, och inget att ta med.
    Inget,
}

fn beskrivning(år: &År) -> String {
    format!(
        "Obetalda kund- och leverantörsfakturor vid räkenskapsårets slut {} (T2)",
        år.slut
    )
}

fn fakturorna(poster: &[Post]) -> Vec<serde_json::Value> {
    poster
        .iter()
        .map(|p| {
            let (nyckel, id) = match p.faktura {
                Faktura::Kund(id) => ("kundfaktura", id.0),
                Faktura::Leverantör(id) => ("leverantörsfaktura", id.0),
            };
            json!({
                nyckel: id,
                "fakturanummer": p.nummer,
                "motpart": p.motpart,
                "obetalt_öre": p.obetalt,
            })
        })
        .collect()
}

/// Räknar om årets T2-utkast `utkast` (T2:s id och verifikatet), eller skapar det när det inte
/// finns: skriver om raderna när de har ändrats, raderar utkastet när inget är obetalt, och
/// loggar när Maya skapar och raderar det. Ger utfallet och fakturorna i utkastet.
async fn räkna_om_i(
    ärende: &mut Ärende,
    år: &År,
    utkast: Option<(i64, VerifikatId)>,
) -> Result<(Utfall, Vec<Post>), DatabasFel> {
    let företag = ärende.företag;
    let poster = obetalda_i(ärende.conn(), företag, år).await?;
    let rader: Vec<_> = poster
        .iter()
        .flat_map(|p| p.rader.iter().cloned())
        .collect();
    let summa: i64 = rader.iter().map(|r| r.debet.öre()).sum();
    let beskrivning = beskrivning(år);
    let maya = Aktör::maya();
    let utfall = match utkast {
        None if rader.is_empty() => Utfall::Inget,
        Some((t2, v)) if rader.is_empty() => {
            let antal: i64 =
                sqlx::query_scalar("SELECT count(*) FROM verifikatrad WHERE verifikat_id = ?")
                    .bind(v.0)
                    .fetch_one(ärende.conn())
                    .await?;
            sqlx::query("DELETE FROM t2 WHERE id = ?")
                .bind(t2)
                .execute(ärende.conn())
                .await?;
            verifikat::ta_bort_utkast_i(ärende, v).await?;
            ärende
                .logga_som(
                    &maya,
                    Händelse::UtkastRaderat,
                    &beskrivning,
                    json!({
                        "utkast": v.0,
                        "t2": t2,
                        "datum": år.slut.to_string(),
                        "beskrivning": beskrivning,
                        "summa_öre": 0,
                        "antal_rader": antal,
                        "underlag": [],
                        "skäl": "inget var obetalt vid årets slut",
                    }),
                )
                .await?;
            Utfall::Raderat
        }
        Some((_, v)) => {
            let nu = verifikat::läs_verifikat(ärende.conn(), företag, v)
                .await?
                .ok_or_else(|| DatabasFel::Korrupt("t2.verifikat_id".into()))?;
            let samma = nu.beskrivning == beskrivning
                && nu.rader.len() == rader.len()
                && nu.rader.iter().zip(&rader).all(|(a, b)| {
                    a.konto == Some(b.konto)
                        && a.debet == b.debet
                        && a.kredit == b.kredit
                        && a.text == b.text
                });
            if samma {
                Utfall::Oförändrat
            } else {
                verifikat::skriv_utkast_i(
                    ärende,
                    Some(v),
                    &maya,
                    år.id,
                    år.slut,
                    &beskrivning,
                    &rader,
                )
                .await?;
                Utfall::Omräknat
            }
        }
        None => {
            let v = verifikat::skriv_utkast_i(
                ärende,
                None,
                &maya,
                år.id,
                år.slut,
                &beskrivning,
                &rader,
            )
            .await?;
            let t2: i64 = sqlx::query_scalar(
                "INSERT INTO t2 (foretag_id, rakenskapsar_id, verifikat_id) VALUES (?, ?, ?)
                 RETURNING id",
            )
            .bind(ärende.företag.0)
            .bind(år.id)
            .bind(v.0)
            .fetch_one(ärende.conn())
            .await?;
            ärende
                .logga_som(
                    &maya,
                    Händelse::T2UtkastSkapat,
                    &beskrivning,
                    json!({
                        "t2": t2,
                        "utkast": v.0,
                        "räkenskapsår": { "start": år.start.to_string(), "slut": år.slut.to_string() },
                        "datum": år.slut.to_string(),
                        "summa_öre": summa,
                        "fakturor": fakturorna(&poster),
                    }),
                )
                .await?;
            Utfall::Skapat(v)
        }
    };
    Ok((utfall, poster))
}

/// Kund- och sedan leverantörsfakturorna med fakturadatum i året vars fordran eller skuld inte
/// är bokförd och som hade något obetalt vid årets slut, i fakturadatumets ordning, med sina
/// rader. En kreditfaktura räknas av mot sitt original och står inte här själv.
async fn obetalda_i(
    conn: &mut SqliteConnection,
    företag: FöretagsId,
    år: &År,
) -> Result<Vec<Post>, DatabasFel> {
    let kund: Vec<(i64, String, Option<String>)> = sqlx::query_as(
        "SELECT f.id, coalesce(CAST(f.nummer AS TEXT), f.fakturanummer),
                f.kunduppgifter ->> '$.namn'
         FROM kundfaktura f
         WHERE f.foretag_id = ?1 AND f.status IN ('utfardad', 'registrerad')
           AND f.krediterar IS NULL AND f.fakturadatum BETWEEN ?2 AND ?3
           AND NOT EXISTS (SELECT 1 FROM reskontrapost_bokford r WHERE r.kundfaktura_id = f.id)
         ORDER BY f.fakturadatum, f.id",
    )
    .bind(företag.0)
    .bind(år.start.to_string())
    .bind(år.slut.to_string())
    .fetch_all(&mut *conn)
    .await?;
    let leverantör: Vec<(i64, String, Option<String>)> = sqlx::query_as(
        "SELECT f.id, f.fakturanummer, l.namn
         FROM leverantorsfaktura f LEFT JOIN leverantor l ON l.id = f.leverantor_id
         WHERE f.foretag_id = ?1 AND f.status = 'registrerad'
           AND f.krediterar IS NULL AND f.fakturadatum BETWEEN ?2 AND ?3
           AND NOT EXISTS (
               SELECT 1 FROM reskontrapost_bokford r WHERE r.leverantorsfaktura_id = f.id)
         ORDER BY f.fakturadatum, f.id",
    )
    .bind(företag.0)
    .bind(år.start.to_string())
    .bind(år.slut.to_string())
    .fetch_all(&mut *conn)
    .await?;
    let fakturor = kund
        .into_iter()
        .map(|(id, n, m)| (Faktura::Kund(KundfakturaId(id)), "kundfaktura", n, m))
        .chain(leverantör.into_iter().map(|(id, n, m)| {
            (
                Faktura::Leverantör(LeverantörsfakturaId(id)),
                "leverantörsfaktura",
                n,
                m,
            )
        }));
    let mut poster = Vec::new();
    for (faktura, sort, nummer, motpart) in fakturor {
        let obetalt = fordran::obetalt_vid_årets_slut_i(&mut *conn, faktura, år.slut).await?;
        if obetalt <= 0 {
            continue;
        }
        let o = fordran::underlaget_i(&mut *conn, företag, faktura).await?;
        let motpart = motpart.unwrap_or_default();
        let text = format!("{sort} {nummer}, {motpart}");
        let rader = o
            .kontering(obetalt * o.total().signum())
            .into_iter()
            .map(|r| maya_domain::verifikat::Rad {
                text: Some(text.clone()),
                ..r
            })
            .collect();
        poster.push(Post {
            faktura,
            nummer,
            motpart,
            obetalt,
            rader,
        });
    }
    Ok(poster)
}
