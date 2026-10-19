//! Externa kundfakturor (#143): kundfakturor som har skapats i ett annat system och registreras
//! i Maya med sin PDF som underlag.
//!
//! En extern faktura är en rad i `kundfaktura` med `extern = 1`, så att betalningar
//! ([`crate::betalning`]), kundförlust ([`crate::kundforlust`]), det som återstår och märkena går
//! genom samma regler som för Mayas egna fakturor. Den laddas upp som ett utkast med originalet
//! kopplat ([`ladda_upp`]), skrivs av ([`spara_utkast`]) och *registreras* ([`registrera`]) i en
//! transaktion: spärrarna i [`maya_domain::extern_kundfaktura`] prövas mot det som gäller i
//! transaktionen, registreringens verifikat bokförs daterat på fakturadatumet med originalet som
//! underlag (fakturametoden) och registreringen loggas (H1). Den ingår inte i fakturaserien.
//!
//! En *extern kreditfaktura* skapas från originalets dokument ([`kreditera`]) med en hänvisning
//! som aldrig ändras, och registreras med negativa belopp. *Rätta registrering…*
//! ([`rätta_registrering`]) vänder registreringens verifikat med en rättelse (R1), låser upp
//! fakturan och behåller betalningarna; den skrivs av och registreras om med samma regler.

use maya_domain::extern_kundfaktura::{
    Förutsättningar, Krediterar, NyExternFaktura, Spärr, Utkast, Utkastrad,
};
use maya_domain::faktura::Momssats;
use maya_domain::foretag::Bokföringsmetod;
use maya_domain::kund::OmvändSkattskyldighet;
use maya_domain::kundfaktura::Fakturakund;
use serde_json::json;
use sqlx::SqliteConnection;

use crate::aktor::{Aktör, Handling};
use crate::atkomst::{self, Ärende, ÅtkomstFel};
use crate::behandlingshistorik::{self, Händelse};
use crate::foretag::{self, FöretagsId, datum};
use crate::kundfaktura::{KundfakturaId, Status, bokföringsmetod, kontonummer};
use crate::underlag::{self, UnderlagFel, UnderlagId, Uppladdat};
use crate::verifikat::{self, Bokfört, Historikpost, Signatur, VerifikatFel, VerifikatId};
use crate::{Databas, DatabasFel, Klocka};

#[derive(Debug, thiserror::Error)]
pub enum ExternFel {
    /// Spärrarna i `domain`, med samma koder som formuläret visar.
    #[error("den externa fakturan kan inte registreras")]
    Spärrat(Vec<Spärr>),
    /// Ingen extern kundfaktura med det id:t i företaget.
    #[error("den externa fakturan finns inte")]
    FinnsInte,
    /// Fakturan skrivs inte av: den finns inte, eller är registrerad.
    #[error("utkastet finns inte")]
    IngetUtkast,
    /// Fakturan är inte registrerad, så den kan inte rättas eller krediteras.
    #[error("fakturan är inte registrerad")]
    InteRegistrerad,
    /// Kontantmetoden på fakturadatumet: registreringen bokförs inte, och det flödet kommer i
    /// #149.
    #[error("externa kundfakturor under kontantmetoden går inte att registrera ännu")]
    Kontantmetoden,
    /// Rätta registrering spärras när en kundförlust gäller fakturan: återkalla den först.
    #[error("en kundförlust gäller fakturan")]
    HarKundförlust,
    /// Rätta registrering spärras när registrerade kreditfakturor krediterar fakturan.
    #[error("kreditfakturor krediterar fakturan")]
    HarKreditfakturor,
    /// Rättelsen av registreringens verifikat gick inte att bokföra.
    #[error(transparent)]
    Rättelse(VerifikatFel),
    #[error(transparent)]
    Underlag(UnderlagFel),
    #[error(transparent)]
    Verifikat(VerifikatFel),
    #[error(transparent)]
    Åtkomst(#[from] ÅtkomstFel),
}

impl From<sqlx::Error> for ExternFel {
    fn from(e: sqlx::Error) -> Self {
        ExternFel::Åtkomst(e.into())
    }
}

impl From<DatabasFel> for ExternFel {
    fn from(e: DatabasFel) -> Self {
        ExternFel::Åtkomst(e.into())
    }
}

impl From<UnderlagFel> for ExternFel {
    fn from(e: UnderlagFel) -> Self {
        match e {
            UnderlagFel::Åtkomst(e) => ExternFel::Åtkomst(e),
            e => ExternFel::Underlag(e),
        }
    }
}

impl From<VerifikatFel> for ExternFel {
    fn from(e: VerifikatFel) -> Self {
        match e {
            VerifikatFel::Åtkomst(e) => ExternFel::Åtkomst(e),
            e => ExternFel::Verifikat(e),
        }
    }
}

/// Resultatet av en registrering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Registrerat {
    pub verifikat: Bokfört,
}

/// Resultatet av Rätta registrering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegistreringRättad {
    /// Rättelsen av registreringens verifikat; `None` när fordran inte var bokförd (#149).
    pub rättelse: Option<Bokfört>,
}

/// En upplåsning med Rätta registrering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Registreringsrättelse {
    /// Registreringens verifikat, som rättelsen vände.
    pub verifikat: Option<verifikat::Länk>,
    pub rättelse: Option<verifikat::Länk>,
    pub orsak: String,
    pub rättad: Signatur,
}

/// En registrerad kreditfaktura på originalets dokument.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Kreditlänk {
    pub id: KundfakturaId,
    pub fakturanummer: String,
    /// Kreditfakturans total, negativ.
    pub total: i64,
}

/// En extern kundfaktura med formuläret, registreringen och historiken.
#[derive(Debug, Clone, PartialEq)]
pub struct ExternKundfaktura {
    pub id: KundfakturaId,
    /// Utkast, registrerad eller upplåst.
    pub status: Status,
    /// Fakturan som formulär: det sparade, eller det som registrerades.
    pub formulär: NyExternFaktura,
    /// Kundens namn: som det kopierades in vid registreringen, annars ur registret.
    pub kund: String,
    /// Originalet från det andra systemet.
    pub underlag: UnderlagId,
    /// Registreringens verifikat; efter Rätta registrering det senaste.
    pub verifikat: Option<verifikat::Länk>,
    pub skapad: Signatur,
    pub registrerad: Option<Signatur>,
    /// Totalen som registrerades, i öre.
    pub total: Option<i64>,
    /// Originalet när fakturan är en kreditfaktura.
    pub krediterar: Option<Krediterar>,
    /// De registrerade kreditfakturorna som krediterar fakturan.
    pub krediteras_av: Vec<Kreditlänk>,
    pub rättelser: Vec<Registreringsrättelse>,
    pub historik: Vec<Historikpost>,
}

/// Laddar upp originalet och skapar ett externt utkast med det kopplat, i en transaktion.
pub async fn ladda_upp(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    företag: FöretagsId,
    fil: (&str, &str, &[u8]),
) -> Result<(KundfakturaId, Uppladdat), ExternFel> {
    let mut ärende =
        Ärende::börja(db, klocka, aktör, företag, Handling::HanteraFakturautkast).await?;
    atkomst::kräv(ärende.conn(), aktör, företag, Handling::HanteraUnderlag).await?;
    let (filnamn, mime, innehåll) = fil;
    let uppladdat = underlag::ladda_upp_i(&mut ärende, filnamn, mime, innehåll).await?;
    let id = skapa_i(&mut ärende, uppladdat.id, None).await?;
    ärende.slutför().await?;
    Ok((id, uppladdat))
}

/// Registrera kreditfaktura… på en registrerad extern faktura: laddar upp kreditfakturans
/// original och skapar ett utkast till en extern kreditfaktura med en hänvisning till `original`
/// och originalets kund. Hänvisningen ändras aldrig.
pub async fn kreditera(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    företag: FöretagsId,
    original: KundfakturaId,
    fil: (&str, &str, &[u8]),
) -> Result<KundfakturaId, ExternFel> {
    let mut ärende =
        Ärende::börja(db, klocka, aktör, företag, Handling::HanteraFakturautkast).await?;
    atkomst::kräv(ärende.conn(), aktör, företag, Handling::HanteraUnderlag).await?;
    let h = krediterar_i(ärende.conn(), företag, original.0, None)
        .await?
        .ok_or(ExternFel::InteRegistrerad)?;
    let (filnamn, mime, innehåll) = fil;
    let uppladdat = underlag::ladda_upp_i(&mut ärende, filnamn, mime, innehåll).await?;
    let id = skapa_i(&mut ärende, uppladdat.id, Some(&h)).await?;
    ärende.slutför().await?;
    Ok(id)
}

async fn skapa_i(
    ärende: &mut Ärende,
    underlag: UnderlagId,
    krediterar: Option<&Krediterar>,
) -> Result<KundfakturaId, sqlx::Error> {
    let (företag, aktör, nu) = (ärende.företag, ärende.aktör, ärende.nu);
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO kundfaktura
             (foretag_id, status, extern, kund_id, krediterar, underlag_id, skapad, skapad_kanal,
              skapad_av)
         VALUES (?, 'utkast', 1, ?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(företag.0)
    .bind(krediterar.map(|h| h.kund.id))
    .bind(krediterar.map(|h| h.faktura))
    .bind(underlag.0)
    .bind(nu.to_string())
    .bind(aktör.kanal.namn())
    .bind(aktör.användare.map(|a| a.0))
    .fetch_one(ärende.conn())
    .await?;
    Ok(KundfakturaId(id))
}

/// Sparar formuläret i utkastet eller den upplåsta fakturan. Bara det som inte går att lagra
/// spärrar (se [`NyExternFaktura::som_utkast`]).
pub async fn spara_utkast(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    företag: FöretagsId,
    id: KundfakturaId,
    formulär: &NyExternFaktura,
) -> Result<(), ExternFel> {
    let mut ärende =
        Ärende::börja(db, klocka, aktör, företag, Handling::HanteraFakturautkast).await?;
    spara_i(&mut ärende, id, formulär).await?;
    ärende.slutför().await?;
    Ok(())
}

async fn spara_i(
    ärende: &mut Ärende,
    id: KundfakturaId,
    formulär: &NyExternFaktura,
) -> Result<Status, ExternFel> {
    let företag = ärende.företag;
    let (status, krediterar) = kräv_avskrift(ärende, id).await?;
    let u = formulär.som_utkast().map_err(ExternFel::Spärrat)?;
    // En kreditfaktura behåller originalets kund; annars en kund ur registret.
    let kund = match krediterar {
        Some(_) => None,
        None => match u.kund {
            Some(k) => sqlx::query_scalar::<_, i64>(
                "SELECT id FROM kund WHERE id = ? AND foretag_id = ?
                     AND status IN ('aktiv', 'arkiverad')",
            )
            .bind(k)
            .bind(företag.0)
            .fetch_optional(ärende.conn())
            .await?
            .map(Some)
            .ok_or_else(|| {
                ExternFel::Spärrat(
                    formulär
                        .kontrollera(&Förutsättningar::default())
                        .err()
                        .unwrap_or_default()
                        .into_iter()
                        .filter(|s| s.kod == "kund_saknas")
                        .collect(),
                )
            })?,
            None => None,
        },
    };
    let sql = if krediterar.is_some() {
        "UPDATE kundfaktura SET fakturanummer = ?2, fakturadatum = ?3, forfallodag = ?4
         WHERE id = ?5 AND status IN ('utkast', 'upplast')"
    } else {
        "UPDATE kundfaktura SET kund_id = ?1, fakturanummer = ?2, fakturadatum = ?3,
             forfallodag = ?4
         WHERE id = ?5 AND status IN ('utkast', 'upplast')"
    };
    sqlx::query(sql)
        .bind(kund)
        .bind(&u.fakturanummer)
        .bind(u.fakturadatum.map(|d| d.to_string()))
        .bind(u.förfallodag.map(|d| d.to_string()))
        .bind(id.0)
        .execute(ärende.conn())
        .await?;
    sqlx::query("DELETE FROM kundfakturarad WHERE faktura_id = ?")
        .bind(id.0)
        .execute(ärende.conn())
        .await?;
    for (i, r) in u.rader.iter().enumerate() {
        sqlx::query(
            "INSERT INTO kundfakturarad
                 (faktura_id, radnr, beskrivning, enhet, momssats, konto, netto)
             VALUES (?, ?, '', '', ?, ?, ?)",
        )
        .bind(id.0)
        .bind(i as i64 + 1)
        .bind(i64::from(r.momssats.procent()))
        .bind(r.konto.map(|k| i64::from(k.get())))
        .bind(r.belopp)
        .execute(ärende.conn())
        .await?;
    }
    Ok(status)
}

/// Raderar ett externt utkast och loggar `fakturautkast raderat` med en sammanfattning.
/// Originalet blir ett okopplat underlag igen. En registrerad eller upplåst faktura raderas
/// aldrig (L1, A1).
pub async fn radera_utkast(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    företag: FöretagsId,
    id: KundfakturaId,
) -> Result<(), ExternFel> {
    let mut ärende =
        Ärende::börja(db, klocka, aktör, företag, Handling::HanteraFakturautkast).await?;
    let (status, _) = kräv_avskrift(&mut ärende, id).await?;
    if status != Status::Utkast {
        return Err(ExternFel::IngetUtkast);
    }
    let utkast = läs_utkast(ärende.conn(), id).await?;
    let kundnamn = kundens_namn(ärende.conn(), id).await?;
    let summa = utkast.som_formulär().beräkna().total;
    sqlx::query("DELETE FROM kundfakturarad WHERE faktura_id = ?")
        .bind(id.0)
        .execute(ärende.conn())
        .await?;
    sqlx::query("DELETE FROM kundfaktura WHERE id = ? AND status = 'utkast'")
        .bind(id.0)
        .execute(ärende.conn())
        .await?;
    ärende
        .logga(
            Händelse::FakturautkastRaderat,
            if kundnamn.is_empty() {
                "extern kundfaktura utan kund"
            } else {
                &kundnamn
            },
            json!({
                "sort": "extern kundfaktura",
                "kundfaktura": id.0,
                "kund": utkast.kund,
                "kundnamn": kundnamn,
                "fakturanummer": utkast.fakturanummer,
                "fakturadatum": utkast.fakturadatum.map(|d| d.to_string()),
                "summa_öre": summa,
                "antal_rader": utkast.rader.len(),
            }),
        )
        .await?;
    ärende.slutför().await?;
    Ok(())
}

/// Registrerar och bokför utkastet, eller den upplåsta fakturan, med innehållet i formuläret i
/// en transaktion: prövar spärrarna, sparar det som registreras, bokför registreringens
/// verifikat daterat på fakturadatumet med originalet som underlag, låser fakturan och loggar
/// registreringen. Under kontantmetoden på fakturadatumet registreras fakturan inte än (#149).
pub async fn registrera(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    företag: FöretagsId,
    id: KundfakturaId,
    formulär: &NyExternFaktura,
) -> Result<Registrerat, ExternFel> {
    let mut ärende = Ärende::börja(db, klocka, aktör, företag, Handling::RegistreraFaktura).await?;
    kräv_avskrift(&mut ärende, id).await?;
    let förutsättningar = läs_förutsättningar(ärende.conn(), företag, Some(id)).await?;
    let r = formulär
        .kontrollera(&förutsättningar)
        .map_err(ExternFel::Spärrat)?;
    if bokföringsmetod(ärende.conn(), företag, r.fakturadatum).await? != Bokföringsmetod::Faktura
    {
        return Err(ExternFel::Kontantmetoden);
    }
    let status = spara_i(&mut ärende, id, formulär).await?;
    let underlag = UnderlagId(
        sqlx::query_scalar("SELECT underlag_id FROM kundfaktura WHERE id = ?")
            .bind(id.0)
            .fetch_one(ärende.conn())
            .await?,
    );
    // Raderna som registreras, med kontot och beloppet som bokförs.
    sqlx::query("DELETE FROM kundfakturarad WHERE faktura_id = ?")
        .bind(id.0)
        .execute(ärende.conn())
        .await?;
    for (i, rad) in r.rader.iter().enumerate() {
        sqlx::query(
            "INSERT INTO kundfakturarad
                 (faktura_id, radnr, beskrivning, enhet, momssats, konto, netto)
             VALUES (?, ?, '', '', ?, ?, ?)",
        )
        .bind(id.0)
        .bind(i as i64 + 1)
        .bind(i64::from(rad.momssats.procent()))
        .bind(i64::from(rad.konto.get()))
        .bind(rad.netto)
        .execute(ärende.conn())
        .await?;
    }
    let bokfört = verifikat::bokför_i_ärende(
        &mut ärende,
        &verifikat::formulär_av(r.fakturadatum, &r.beskrivning(), &r.kontering()),
        &[underlag],
    )
    .await?;
    let momsregistrerat = förutsättningar.momsregistrerat(r.fakturadatum);
    let (aktör_, nu) = (ärende.aktör, ärende.nu);
    sqlx::query(
        "UPDATE kundfaktura SET status = 'registrerad', kund_id = ?, fakturanummer = ?,
             fakturadatum = ?, forfallodag = ?, kunduppgifter = ?, netto = ?, moms = ?,
             total = ?, momsregistrerat = ?, verifikat_id = ?, registrerad = ?,
             registrerad_kanal = ?, registrerad_av = ?
         WHERE id = ? AND status IN ('utkast', 'upplast')",
    )
    .bind(r.kund.id)
    .bind(&r.fakturanummer)
    .bind(r.fakturadatum.to_string())
    .bind(r.förfallodag.to_string())
    .bind(serde_json::to_string(&r.kund).expect("kunden som JSON"))
    .bind(r.netto)
    .bind(r.moms)
    .bind(r.total)
    .bind(momsregistrerat)
    .bind(bokfört.id.0)
    .bind(nu.to_string())
    .bind(aktör_.kanal.namn())
    .bind(aktör_.användare.map(|a| a.0))
    .bind(id.0)
    .execute(ärende.conn())
    .await?;
    let (filnamn, sha256): (String, String) =
        sqlx::query_as("SELECT filnamn, sha256 FROM underlag WHERE id = ?")
            .bind(underlag.0)
            .fetch_one(ärende.conn())
            .await?;
    let händelse = match &r.krediterar {
        Some(_) => Händelse::KreditfakturaRegistrerad,
        None => Händelse::ExternKundfakturaRegistrerad,
    };
    let mut post = json!({
        "kundfaktura": id.0,
        "kund": r.kund.id,
        "kundnamn": r.kund.namn,
        "fakturanummer": r.fakturanummer,
        "fakturadatum": r.fakturadatum.to_string(),
        "förfallodag": r.förfallodag.to_string(),
        "netto_öre": r.netto,
        "moms_öre": r.moms,
        "summa_öre": r.total,
        "omregistrerad": status == Status::Upplåst,
        "verifikat": { "id": bokfört.id.0, "nummer": bokfört.nummer.to_string() },
        "underlag": { "underlag": underlag.0, "filnamn": filnamn, "sha256": sha256 },
    });
    // Kreditfakturans post syns också i originalets historik.
    if let Some(k) = &r.krediterar {
        post["krediterar"] = json!(k.faktura);
        post["krediterar_nummer"] = json!(k.fakturanummer);
    }
    ärende.logga(händelse, &r.fakturanummer, post).await?;
    ärende.slutför().await?;
    Ok(Registrerat {
        verifikat: bokfört
    })
}

/// Rätta registrering… (berättelse 46): vänder registreringens verifikat med en rättelse
/// daterad i dag (R1), låser upp fakturan och loggar det, i en transaktion. Betalningarna ligger
/// kvar; fakturan skrivs av och registreras om med samma regler. Orsaken är valfri. Spärras när
/// en kundförlust gäller fakturan eller kreditfakturor krediterar den, eftersom de bokfördes mot
/// registreringen.
pub async fn rätta_registrering(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    företag: FöretagsId,
    id: KundfakturaId,
    orsak: &str,
) -> Result<RegistreringRättad, ExternFel> {
    let mut ärende = Ärende::börja(db, klocka, aktör, företag, Handling::RättaFlöde).await?;
    type Fakturan = (Option<i64>, String, String, Option<String>, Option<i64>);
    let rad: Option<Fakturan> = sqlx::query_as(
        "SELECT verifikat_id, registrerad, fakturanummer, kunduppgifter ->> '$.namn', total
         FROM kundfaktura
         WHERE id = ? AND foretag_id = ? AND extern = 1 AND status = 'registrerad'",
    )
    .bind(id.0)
    .bind(företag.0)
    .fetch_optional(ärende.conn())
    .await?;
    let (verifikat_id, registrering, fakturanummer, kundnamn, total) =
        rad.ok_or(ExternFel::InteRegistrerad)?;
    if crate::kundforlust::förlorat_i(ärende.conn(), id).await? != 0 {
        return Err(ExternFel::HarKundförlust);
    }
    if crate::kundfaktura::krediterat_i(ärende.conn(), id).await? != 0 {
        return Err(ExternFel::HarKreditfakturor);
    }
    let idag = maya_domain::tid::datum(ärende.nu);
    let rättelse = match verifikat_id {
        Some(v) => Some(
            verifikat::rätta_i(&mut ärende, VerifikatId(v), idag, orsak)
                .await
                .map_err(|e| match e {
                    VerifikatFel::Åtkomst(e) => ExternFel::Åtkomst(e),
                    e => ExternFel::Rättelse(e),
                })?,
        ),
        None => None,
    };
    let (nu, kanal, av) = (
        ärende.nu.to_string(),
        ärende.aktör.kanal.namn(),
        ärende.aktör.användare.map(|a| a.0),
    );
    sqlx::query(
        "INSERT INTO kundfaktura_registrering
             (foretag_id, faktura_id, registrering, verifikat_id, rattelse_id, orsak, rattad,
              rattad_kanal, rattad_av)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(företag.0)
    .bind(id.0)
    .bind(&registrering)
    .bind(verifikat_id)
    .bind(rättelse.map(|r| r.id.0))
    .bind(orsak.trim())
    .bind(nu)
    .bind(kanal)
    .bind(av)
    .execute(ärende.conn())
    .await?;
    sqlx::query(
        "UPDATE kundfaktura SET status = 'upplast' WHERE id = ? AND status = 'registrerad'",
    )
    .bind(id.0)
    .execute(ärende.conn())
    .await?;
    let verifikatnummer = verifikat::länk(ärende.conn(), verifikat_id)
        .await?
        .and_then(|l| l.nummer)
        .map(|n| n.to_string());
    let kundnamn = kundnamn.unwrap_or_default();
    ärende
        .logga(
            Händelse::RegistreringRättad,
            &fakturanummer,
            json!({
                "kundfaktura": id.0,
                "fakturanummer": fakturanummer,
                "kundnamn": kundnamn,
                "summa_öre": total,
                "orsak": orsak.trim(),
                "verifikat": verifikat_id.map(|v| json!({ "id": v, "nummer": verifikatnummer })),
                "rättelse": rättelse.map(|r| json!({ "id": r.id.0, "nummer": r.nummer.to_string() })),
                "datum": idag.to_string(),
            }),
        )
        .await?;
    ärende.slutför().await?;
    Ok(RegistreringRättad { rättelse })
}

/// Det som spärrarna prövas mot för fakturan `id`, eller för ett nytt utkast.
pub async fn förutsättningar(
    db: &Databas,
    aktör: &Aktör,
    företag: FöretagsId,
    id: Option<KundfakturaId>,
) -> Result<Förutsättningar, ÅtkomstFel> {
    let mut tx = db.pool().begin().await?;
    atkomst::kräv(&mut tx, aktör, företag, Handling::LäsaFöretag).await?;
    Ok(läs_förutsättningar(&mut tx, företag, id).await?)
}

async fn läs_förutsättningar(
    conn: &mut SqliteConnection,
    företag: FöretagsId,
    id: Option<KundfakturaId>,
) -> Result<Förutsättningar, DatabasFel> {
    let f = foretag::läs_företag(&mut *conn, företag).await?;
    type Kundrad = (i64, String, String, String, String, String, i64, String);
    let kunder: Vec<Kundrad> = sqlx::query_as(
        "SELECT id, namn, identitet, adress, epost, momsregnr, betalningsvillkor,
                omvand_skattskyldighet
         FROM kund
         WHERE foretag_id = ? AND status = 'aktiv' ORDER BY namn COLLATE NOCASE, id",
    )
    .bind(företag.0)
    .fetch_all(&mut *conn)
    .await?;
    let konton: Vec<(i64, bool)> =
        sqlx::query_as("SELECT nummer, aktiv FROM konto WHERE foretag_id = ? ORDER BY nummer")
            .bind(företag.0)
            .fetch_all(&mut *conn)
            .await?;
    let upptagna: Vec<String> = sqlx::query_scalar(
        "SELECT fakturanummer FROM kundfaktura
         WHERE foretag_id = ? AND extern = 1 AND status <> 'utkast' AND id IS NOT ?",
    )
    .bind(företag.0)
    .bind(id.map(|i| i.0))
    .fetch_all(&mut *conn)
    .await?;
    let original: Option<i64> = match id {
        Some(id) => sqlx::query_scalar("SELECT krediterar FROM kundfaktura WHERE id = ?")
            .bind(id.0)
            .fetch_optional(&mut *conn)
            .await?
            .flatten(),
        None => None,
    };
    let krediterar = match original {
        Some(o) => krediterar_i(&mut *conn, företag, o, id).await?,
        None => None,
    };
    let betalt = match id {
        Some(id) => {
            sqlx::query_scalar(
                "SELECT coalesce(sum(belopp + oresutjamning), 0) FROM betalning_i_reskontran
             WHERE kundfaktura_id = ?",
            )
            .bind(id.0)
            .fetch_one(&mut *conn)
            .await?
        }
        None => 0,
    };
    Ok(Förutsättningar {
        kunder: kunder
            .into_iter()
            .map(
                |(id, namn, identitet, adress, epost, momsregnr, villkor, omvänd)| {
                    Ok(Fakturakund {
                        id,
                        namn,
                        identitet,
                        adress,
                        epost,
                        momsregnr,
                        betalningsvillkor: u16::try_from(villkor).unwrap_or(u16::MAX),
                        omvänd_skattskyldighet: OmvändSkattskyldighet::från_kod(&omvänd)
                            .ok_or_else(|| {
                                DatabasFel::Korrupt("kund.omvand_skattskyldighet".into())
                            })?,
                    })
                },
            )
            .collect::<Result<_, DatabasFel>>()?,
        konton: konton
            .into_iter()
            .map(|(nummer, aktiv)| Ok((kontonummer(nummer)?, aktiv)))
            .collect::<Result<_, DatabasFel>>()?,
        räkenskapsår: f
            .räkenskapsår
            .iter()
            .map(|å| maya_domain::rakenskapsar::Räkenskapsår::befintligt(å.start, å.slut))
            .collect(),
        momsperioder: f.momsperioder,
        upptagna,
        krediterar,
        betalt,
    })
}

/// Originalet `original` som en extern kreditfaktura kan hänvisa till: en registrerad extern
/// faktura i företaget som inte själv är en kreditfaktura, med det som finns kvar att kreditera
/// utan kreditfakturan `utan`.
async fn krediterar_i(
    conn: &mut SqliteConnection,
    företag: FöretagsId,
    original: i64,
    utan: Option<KundfakturaId>,
) -> Result<Option<Krediterar>, DatabasFel> {
    let rad: Option<(String, String, i64)> = sqlx::query_as(
        "SELECT fakturanummer, kunduppgifter, total FROM kundfaktura
         WHERE id = ? AND foretag_id = ? AND extern = 1 AND status = 'registrerad'
           AND krediterar IS NULL",
    )
    .bind(original)
    .bind(företag.0)
    .fetch_optional(&mut *conn)
    .await?;
    let Some((fakturanummer, kund, total)) = rad else {
        return Ok(None);
    };
    let krediterat: i64 = sqlx::query_scalar(
        "SELECT -coalesce(sum(total), 0) FROM kundfaktura
         WHERE krediterar = ? AND status IN ('utfardad', 'registrerad') AND id IS NOT ?",
    )
    .bind(original)
    .bind(utan.map(|u| u.0))
    .fetch_one(&mut *conn)
    .await?;
    Ok(Some(Krediterar {
        faktura: original,
        fakturanummer,
        kund: serde_json::from_str(&kund)
            .map_err(|_| DatabasFel::Korrupt(format!("kundfaktura {original}: kunduppgifter")))?,
        kvar: total - krediterat,
    }))
}

/// Den externa kundfakturan `id` i företaget, eller `None` när det inte finns någon extern
/// faktura med det id:t.
pub async fn fakturan(
    db: &Databas,
    aktör: &Aktör,
    företag: FöretagsId,
    id: KundfakturaId,
) -> Result<Option<ExternKundfaktura>, ÅtkomstFel> {
    let mut tx = db.pool().begin().await?;
    atkomst::kräv(&mut tx, aktör, företag, Handling::LäsaFöretag).await?;
    type Huvud = (
        String,
        i64,
        Option<i64>,
        Option<i64>,
        String,
        String,
        Option<i64>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<i64>,
        Option<String>,
        Option<i64>,
    );
    let huvud: Option<Huvud> = sqlx::query_as(
        "SELECT f.status, f.underlag_id, f.verifikat_id, f.krediterar, f.skapad, f.skapad_kanal,
                f.skapad_av, a.visningsnamn, f.registrerad, f.registrerad_kanal,
                f.registrerad_av, r.visningsnamn, f.total
         FROM kundfaktura f
         LEFT JOIN anvandare a ON a.id = f.skapad_av
         LEFT JOIN anvandare r ON r.id = f.registrerad_av
         WHERE f.id = ? AND f.foretag_id = ? AND f.extern = 1",
    )
    .bind(id.0)
    .bind(företag.0)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((
        status,
        underlag_id,
        verifikat_id,
        original,
        skapad,
        skapad_kanal,
        skapad_av,
        skapad_namn,
        registrerad,
        registrerad_kanal,
        registrerad_av,
        registrerad_namn,
        total,
    )) = huvud
    else {
        return Ok(None);
    };
    let status = Status::från_kod(&status)
        .ok_or_else(|| DatabasFel::Korrupt("kundfaktura.status".into()))?;
    let formulär = läs_utkast(&mut tx, id).await?.som_formulär();
    let kund = kundens_namn(&mut tx, id).await?;
    let krediterar = match original {
        Some(o) => Some(hänvisningen_i(&mut tx, o).await?),
        None => None,
    };
    let krediteras_av: Vec<(i64, String, i64)> = sqlx::query_as(
        "SELECT id, fakturanummer, total FROM kundfaktura
         WHERE krediterar = ? AND status = 'registrerad' ORDER BY registrerad, id",
    )
    .bind(id.0)
    .fetch_all(&mut *tx)
    .await?;
    type Rättelse = (
        Option<i64>,
        Option<i64>,
        String,
        String,
        String,
        Option<i64>,
        Option<String>,
    );
    let rader: Vec<Rättelse> = sqlx::query_as(
        "SELECT r.verifikat_id, r.rattelse_id, r.orsak, r.rattad, r.rattad_kanal, r.rattad_av,
                a.visningsnamn
         FROM kundfaktura_registrering r LEFT JOIN anvandare a ON a.id = r.rattad_av
         WHERE r.faktura_id = ? ORDER BY r.id",
    )
    .bind(id.0)
    .fetch_all(&mut *tx)
    .await?;
    let mut rättelser = Vec::new();
    for (v, r, orsak, tid, kanal, av, namn) in rader {
        rättelser.push(Registreringsrättelse {
            verifikat: verifikat::länk(&mut tx, v).await?,
            rättelse: verifikat::länk(&mut tx, r).await?,
            orsak,
            rättad: verifikat::signatur(&tid, &kanal, av, namn)?,
        });
    }
    let mut historik = Vec::new();
    let poster =
        behandlingshistorik::läs_om_någon(&mut tx, företag, ["kundfaktura", "krediterar"], id.0)
            .await?;
    for post in poster {
        let namn = verifikat::namn(&mut tx, &post.aktör).await?;
        historik.push(Historikpost { post, namn });
    }
    let registrerad = match (registrerad, registrerad_kanal) {
        (Some(t), Some(k)) => Some(verifikat::signatur(
            &t,
            &k,
            registrerad_av,
            registrerad_namn,
        )?),
        _ => None,
    };
    Ok(Some(ExternKundfaktura {
        id,
        status,
        formulär,
        kund,
        underlag: UnderlagId(underlag_id),
        verifikat: verifikat::länk(&mut tx, verifikat_id).await?,
        skapad: verifikat::signatur(&skapad, &skapad_kanal, skapad_av, skapad_namn)?,
        registrerad,
        total,
        krediterar,
        krediteras_av: krediteras_av
            .into_iter()
            .map(|(id, fakturanummer, total)| Kreditlänk {
                id: KundfakturaId(id),
                fakturanummer,
                total,
            })
            .collect(),
        rättelser,
        historik,
    }))
}

/// Hänvisningen som en kreditfaktura har till `original`, med det som finns kvar nu.
async fn hänvisningen_i(
    conn: &mut SqliteConnection,
    original: i64,
) -> Result<Krediterar, DatabasFel> {
    let (fakturanummer, kund, total): (String, Option<String>, Option<i64>) =
        sqlx::query_as("SELECT fakturanummer, kunduppgifter, total FROM kundfaktura WHERE id = ?")
            .bind(original)
            .fetch_one(&mut *conn)
            .await?;
    let krediterat = crate::kundfaktura::krediterat_i(&mut *conn, KundfakturaId(original)).await?;
    Ok(Krediterar {
        faktura: original,
        fakturanummer,
        kund: kund
            .as_deref()
            .and_then(|k| serde_json::from_str(k).ok())
            .unwrap_or_default(),
        kvar: total.unwrap_or_default() - krediterat,
    })
}

/// Att `id` är ett externt utkast eller en upplåst extern faktura i ärendets företag, som
/// skrivs av; ger dess status och originalet när det är en kreditfaktura.
async fn kräv_avskrift(
    ärende: &mut Ärende,
    id: KundfakturaId,
) -> Result<(Status, Option<i64>), ExternFel> {
    let företag = ärende.företag;
    let rad: Option<(String, Option<i64>)> = sqlx::query_as(
        "SELECT status, krediterar FROM kundfaktura
         WHERE id = ? AND foretag_id = ? AND extern = 1 AND status IN ('utkast', 'upplast')",
    )
    .bind(id.0)
    .bind(företag.0)
    .fetch_optional(ärende.conn())
    .await?;
    let (status, krediterar) = rad.ok_or(ExternFel::IngetUtkast)?;
    Ok((
        Status::från_kod(&status)
            .ok_or_else(|| DatabasFel::Korrupt("kundfaktura.status".into()))?,
        krediterar,
    ))
}

/// Kundens namn på fakturan: som det kopierades in vid registreringen, annars ur registret.
async fn kundens_namn(
    conn: &mut SqliteConnection,
    id: KundfakturaId,
) -> Result<String, sqlx::Error> {
    Ok(sqlx::query_scalar::<_, Option<String>>(
        "SELECT coalesce(f.kunduppgifter ->> '$.namn', k.namn)
         FROM kundfaktura f LEFT JOIN kund k ON k.id = f.kund_id WHERE f.id = ?",
    )
    .bind(id.0)
    .fetch_one(conn)
    .await?
    .unwrap_or_default())
}

/// Det sparade utkastet (eller den registrerade fakturans fält) som det lagrades.
async fn läs_utkast(conn: &mut SqliteConnection, id: KundfakturaId) -> Result<Utkast, DatabasFel> {
    let (kund, fakturanummer, fakturadatum, förfallodag): (
        Option<i64>,
        String,
        Option<String>,
        Option<String>,
    ) = sqlx::query_as(
        "SELECT kund_id, fakturanummer, fakturadatum, forfallodag FROM kundfaktura WHERE id = ?",
    )
    .bind(id.0)
    .fetch_one(&mut *conn)
    .await?;
    let rader: Vec<(i64, Option<i64>, Option<i64>)> = sqlx::query_as(
        "SELECT momssats, konto, netto FROM kundfakturarad WHERE faktura_id = ? ORDER BY radnr",
    )
    .bind(id.0)
    .fetch_all(&mut *conn)
    .await?;
    let korrupt = |vad: &str| DatabasFel::Korrupt(format!("kundfaktura {}: {vad}", id.0));
    Ok(Utkast {
        kund,
        fakturanummer,
        fakturadatum: fakturadatum.as_deref().map(datum).transpose()?,
        förfallodag: förfallodag.as_deref().map(datum).transpose()?,
        rader: rader
            .into_iter()
            .map(|(sats, konto, belopp)| {
                Ok(Utkastrad {
                    momssats: u8::try_from(sats)
                        .ok()
                        .and_then(Momssats::från_procent)
                        .ok_or_else(|| korrupt("momssats"))?,
                    konto: konto.map(kontonummer).transpose()?,
                    belopp,
                })
            })
            .collect::<Result<_, DatabasFel>>()?,
    })
}

/// Totalen för ett externt utkast med de rader som går att räkna, för listan.
pub(crate) async fn utkastets_total_i(
    conn: &mut SqliteConnection,
    id: KundfakturaId,
) -> Result<i64, DatabasFel> {
    Ok(läs_utkast(conn, id).await?.som_formulär().beräkna().total)
}

/// Verifikatet som en registrerad extern fakturas fordran bokfördes med, eller `None` när
/// fakturan inte är extern. Kundförlusten speglar det.
pub(crate) async fn registreringens_verifikat_i(
    conn: &mut SqliteConnection,
    företag: FöretagsId,
    id: KundfakturaId,
) -> Result<Option<VerifikatId>, DatabasFel> {
    let rad: Option<Option<i64>> = sqlx::query_scalar(
        "SELECT verifikat_id FROM kundfaktura WHERE id = ? AND foretag_id = ? AND extern = 1",
    )
    .bind(id.0)
    .bind(företag.0)
    .fetch_optional(conn)
    .await?;
    match rad {
        None => Ok(None),
        Some(Some(v)) => Ok(Some(VerifikatId(v))),
        Some(None) => Err(DatabasFel::Korrupt(format!(
            "kundfaktura {}: registreringens verifikat",
            id.0
        ))),
    }
}
