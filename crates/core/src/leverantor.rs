//! Leverantörsregistret: företagets leverantörer med betaluppgifter, standardkonto och förvald
//! omvänd skattskyldighet. Skapande, ändring, arkivering och radering loggas i
//! behandlingshistoriken, en ändring av standardkontot som egen typ eftersom det är en
//! automatkontering (H2, S2). En leverantör med fakturor kan arkiveras men inte raderas.
//! Arkiverade leverantörer finns inte bland de aktiva, som väljarna visar.

use std::fmt;
use std::str::FromStr;

use maya_domain::kontoplan::Kontonummer;
use maya_domain::leverantor::{Fältfel, GiltigLeverantör, NyLeverantör, OmvändSkattskyldighet};
use serde_json::{Value, json};
use sqlx::SqliteConnection;

use crate::aktor::{Aktör, Handling};
use crate::atkomst::{self, Ärende, ÅtkomstFel};
use crate::behandlingshistorik::{self, Händelse, Post};
use crate::foretag::FöretagsId;
use crate::{Databas, DatabasFel, Klocka};

/// En leverantörs id i företaget.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LeverantörsId(pub(crate) i64);

impl fmt::Display for LeverantörsId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for LeverantörsId {
    type Err = std::num::ParseIntError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse().map(LeverantörsId)
    }
}

/// Var leverantören står i registret.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Aktiv,
    Arkiverad,
}

impl Status {
    pub fn kod(self) -> &'static str {
        match self {
            Status::Aktiv => "aktiv",
            Status::Arkiverad => "arkiverad",
        }
    }
}

/// Vilka leverantörer en lista visar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Urval {
    /// De som väljarna visar.
    #[default]
    Aktiva,
    Arkiverade,
    Alla,
}

impl Urval {
    pub fn kod(self) -> &'static str {
        match self {
            Urval::Aktiva => "aktiva",
            Urval::Arkiverade => "arkiverade",
            Urval::Alla => "alla",
        }
    }

    pub fn från_kod(kod: &str) -> Option<Self> {
        [Urval::Aktiva, Urval::Arkiverade, Urval::Alla]
            .into_iter()
            .find(|u| u.kod() == kod)
    }
}

/// En leverantör i registret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Leverantör {
    pub id: LeverantörsId,
    pub status: Status,
    pub uppgifter: GiltigLeverantör,
    /// Om leverantören saknar fakturor och därför kan raderas.
    pub kan_raderas: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum LeverantörsFel {
    /// Formuläret klarar inte kontrollen i `domain`; samma koder som formuläret visar.
    #[error("formuläret är inte rätt ifyllt")]
    Ogiltigt(Vec<Fältfel>),
    #[error("leverantören finns inte i företaget")]
    FinnsInte,
    /// Leverantören har fakturor, som är räkenskapsinformation (A1). Den kan arkiveras.
    #[error("leverantören har fakturor och kan inte raderas; arkivera den i stället")]
    HarFakturor,
    #[error(transparent)]
    Åtkomst(#[from] ÅtkomstFel),
}

impl From<sqlx::Error> for LeverantörsFel {
    fn from(e: sqlx::Error) -> Self {
        LeverantörsFel::Åtkomst(e.into())
    }
}

impl From<DatabasFel> for LeverantörsFel {
    fn from(e: DatabasFel) -> Self {
        LeverantörsFel::Åtkomst(e.into())
    }
}

/// Lägger upp en aktiv leverantör och loggar `leverantör skapad`, och `standardkonto ändrat`
/// när ett standardkonto valdes, i samma transaktion.
pub async fn skapa(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    företag: FöretagsId,
    formulär: &NyLeverantör,
) -> Result<LeverantörsId, LeverantörsFel> {
    let mut ärende =
        Ärende::börja(db, klocka, aktör, företag, Handling::HanteraLeverantörer).await?;
    let l = kontrollera(&mut ärende, formulär).await?;
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO leverantor
             (foretag_id, status, namn, orgnr, gatuadress, postnummer, postort, land, momsregnr,
              bankgiro, plusgiro, iban, bic, standardkonto, omvand_skattskyldighet,
              skapad, skapad_kanal, skapad_av)
         VALUES (?, 'aktiv', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
         RETURNING id",
    )
    .bind(företag.0)
    .bind(&l.namn)
    .bind(&l.orgnr)
    .bind(&l.gatuadress)
    .bind(&l.postnummer)
    .bind(&l.postort)
    .bind(&l.land)
    .bind(&l.momsregnr)
    .bind(&l.bankgiro)
    .bind(&l.plusgiro)
    .bind(&l.iban)
    .bind(&l.bic)
    .bind(l.standardkonto.map(Kontonummer::get))
    .bind(l.omvänd_skattskyldighet.kod())
    .bind(ärende.nu.to_string())
    .bind(aktör.kanal.namn())
    .bind(aktör.användare.map(|a| a.0))
    .fetch_one(ärende.conn())
    .await?;
    let mut beskrivning = json!({ "leverantör": id });
    for (fält, värde) in fält(&l) {
        beskrivning[fält] = värde;
    }
    beskrivning["standardkonto"] = konto(l.standardkonto);
    ärende
        .logga(Händelse::LeverantörSkapad, &l.namn, beskrivning)
        .await?;
    if l.standardkonto.is_some() {
        logga_standardkonto(&mut ärende, id, &l.namn, None, l.standardkonto).await?;
    }
    ärende.slutför().await?;
    Ok(LeverantörsId(id))
}

/// Ändrar en leverantör i registret. Fälten som ändrades loggas som `leverantör ändrad` med
/// från- och till-värde, och ett nytt standardkonto som `standardkonto ändrat`; utan ändring
/// loggas inget.
pub async fn ändra(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    företag: FöretagsId,
    id: LeverantörsId,
    formulär: &NyLeverantör,
) -> Result<(), LeverantörsFel> {
    let mut ärende =
        Ärende::börja(db, klocka, aktör, företag, Handling::HanteraLeverantörer).await?;
    let gammal = läs_en(ärende.conn(), företag, id)
        .await?
        .ok_or(LeverantörsFel::FinnsInte)?
        .uppgifter;
    let ny = kontrollera(&mut ärende, formulär).await?;
    let mut ändrat = serde_json::Map::new();
    for ((namn, från), (_, till)) in fält(&gammal).into_iter().zip(fält(&ny)) {
        if från != till {
            ändrat.insert(namn.into(), json!({ "från": från, "till": till }));
        }
    }
    let nytt_konto = gammal.standardkonto != ny.standardkonto;
    if ändrat.is_empty() && !nytt_konto {
        return Ok(());
    }
    sqlx::query(
        "UPDATE leverantor SET namn = ?, orgnr = ?, gatuadress = ?, postnummer = ?, postort = ?,
             land = ?, momsregnr = ?, bankgiro = ?, plusgiro = ?, iban = ?, bic = ?,
             standardkonto = ?, omvand_skattskyldighet = ?
         WHERE foretag_id = ? AND id = ?",
    )
    .bind(&ny.namn)
    .bind(&ny.orgnr)
    .bind(&ny.gatuadress)
    .bind(&ny.postnummer)
    .bind(&ny.postort)
    .bind(&ny.land)
    .bind(&ny.momsregnr)
    .bind(&ny.bankgiro)
    .bind(&ny.plusgiro)
    .bind(&ny.iban)
    .bind(&ny.bic)
    .bind(ny.standardkonto.map(Kontonummer::get))
    .bind(ny.omvänd_skattskyldighet.kod())
    .bind(företag.0)
    .bind(id.0)
    .execute(ärende.conn())
    .await?;
    if !ändrat.is_empty() {
        ärende
            .logga(
                Händelse::LeverantörÄndrad,
                &ny.namn,
                json!({ "leverantör": id.0, "ändrat": ändrat }),
            )
            .await?;
    }
    if nytt_konto {
        logga_standardkonto(
            &mut ärende,
            id.0,
            &ny.namn,
            gammal.standardkonto,
            ny.standardkonto,
        )
        .await?;
    }
    ärende.slutför().await?;
    Ok(())
}

/// Arkiverar en leverantör, så att den inte längre finns bland de aktiva, och loggar
/// `leverantör arkiverad`. En leverantör som redan är arkiverad lämnas som den är.
pub async fn arkivera(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    företag: FöretagsId,
    id: LeverantörsId,
) -> Result<(), LeverantörsFel> {
    let mut ärende =
        Ärende::börja(db, klocka, aktör, företag, Handling::HanteraLeverantörer).await?;
    let l = läs_en(ärende.conn(), företag, id)
        .await?
        .ok_or(LeverantörsFel::FinnsInte)?;
    if l.status == Status::Arkiverad {
        return Ok(());
    }
    sqlx::query("UPDATE leverantor SET status = 'arkiverad' WHERE foretag_id = ? AND id = ?")
        .bind(företag.0)
        .bind(id.0)
        .execute(ärende.conn())
        .await?;
    ärende
        .logga(
            Händelse::LeverantörArkiverad,
            &l.uppgifter.namn,
            json!({ "leverantör": id.0, "namn": l.uppgifter.namn }),
        )
        .await?;
    ärende.slutför().await?;
    Ok(())
}

/// Raderar en leverantör som saknar fakturor och loggar `leverantör raderad` med dess
/// uppgifter. En leverantör med fakturor kan bara arkiveras.
pub async fn radera(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    företag: FöretagsId,
    id: LeverantörsId,
) -> Result<(), LeverantörsFel> {
    let mut ärende =
        Ärende::börja(db, klocka, aktör, företag, Handling::HanteraLeverantörer).await?;
    let l = läs_en(ärende.conn(), företag, id)
        .await?
        .ok_or(LeverantörsFel::FinnsInte)?;
    if !l.kan_raderas {
        return Err(LeverantörsFel::HarFakturor);
    }
    sqlx::query("DELETE FROM leverantor WHERE foretag_id = ? AND id = ?")
        .bind(företag.0)
        .bind(id.0)
        .execute(ärende.conn())
        .await?;
    let mut beskrivning = json!({ "leverantör": id.0, "status": l.status.kod() });
    for (fält, värde) in fält(&l.uppgifter) {
        beskrivning[fält] = värde;
    }
    beskrivning["standardkonto"] = konto(l.uppgifter.standardkonto);
    ärende
        .logga(Händelse::LeverantörRaderad, &l.uppgifter.namn, beskrivning)
        .await?;
    ärende.slutför().await?;
    Ok(())
}

/// Företagets leverantörer i urvalet, i namnordning.
pub async fn lista(
    db: &Databas,
    aktör: &Aktör,
    företag: FöretagsId,
    urval: Urval,
) -> Result<Vec<Leverantör>, ÅtkomstFel> {
    let mut tx = db.pool().begin().await?;
    atkomst::kräv(&mut tx, aktör, företag, Handling::LäsaFöretag).await?;
    let alla = läs(&mut tx, företag, None).await?;
    Ok(alla
        .into_iter()
        .filter(|l| match urval {
            Urval::Aktiva => l.status == Status::Aktiv,
            Urval::Arkiverade => l.status == Status::Arkiverad,
            Urval::Alla => true,
        })
        .collect())
}

/// En leverantör i företaget, eller `None` om den inte finns där.
pub async fn leverantören(
    db: &Databas,
    aktör: &Aktör,
    företag: FöretagsId,
    id: LeverantörsId,
) -> Result<Option<Leverantör>, ÅtkomstFel> {
    let mut tx = db.pool().begin().await?;
    atkomst::kräv(&mut tx, aktör, företag, Handling::LäsaFöretag).await?;
    Ok(läs_en(&mut tx, företag, id).await?)
}

/// Posterna i behandlingshistoriken som rör leverantören, äldst först.
pub async fn historik(
    db: &Databas,
    aktör: &Aktör,
    företag: FöretagsId,
    id: LeverantörsId,
) -> Result<Vec<Post>, ÅtkomstFel> {
    let mut tx = db.pool().begin().await?;
    atkomst::kräv(&mut tx, aktör, företag, Handling::LäsaFöretag).await?;
    Ok(behandlingshistorik::läs_om(&mut tx, företag, "leverantör", id.0).await?)
}

async fn kontrollera(
    ärende: &mut Ärende,
    formulär: &NyLeverantör,
) -> Result<GiltigLeverantör, LeverantörsFel> {
    let konton: Vec<(i64, bool)> =
        sqlx::query_as("SELECT nummer, aktiv FROM konto WHERE foretag_id = ?")
            .bind(ärende.företag.0)
            .fetch_all(ärende.conn())
            .await?;
    let konton: Vec<(Kontonummer, bool)> = konton
        .into_iter()
        .filter_map(|(nr, aktiv)| Some((Kontonummer::new(u16::try_from(nr).ok()?)?, aktiv)))
        .collect();
    formulär
        .kontrollera(&konton)
        .map_err(LeverantörsFel::Ogiltigt)
}

/// Uppgifterna som loggas och jämförs, med namnen i historiken. Standardkontot loggas för sig.
fn fält(l: &GiltigLeverantör) -> [(&'static str, Value); 12] {
    [
        ("namn", l.namn.clone().into()),
        ("orgnr", l.orgnr.clone().into()),
        ("gatuadress", l.gatuadress.clone().into()),
        ("postnummer", l.postnummer.clone().into()),
        ("postort", l.postort.clone().into()),
        ("land", l.land.clone().into()),
        ("momsregnr", l.momsregnr.clone().into()),
        ("bankgiro", l.bankgiro.clone().into()),
        ("plusgiro", l.plusgiro.clone().into()),
        ("iban", l.iban.clone().into()),
        ("bic", l.bic.clone().into()),
        (
            "omvänd_skattskyldighet",
            l.omvänd_skattskyldighet.kod().into(),
        ),
    ]
}

fn konto(nr: Option<Kontonummer>) -> Value {
    nr.map_or(Value::Null, |k| k.get().into())
}

async fn logga_standardkonto(
    ärende: &mut Ärende,
    id: i64,
    namn: &str,
    från: Option<Kontonummer>,
    till: Option<Kontonummer>,
) -> Result<(), sqlx::Error> {
    ärende
        .logga(
            Händelse::StandardkontoÄndrat,
            namn,
            json!({
                "leverantör": id,
                "namn": namn,
                "från": konto(från),
                "till": konto(till),
            }),
        )
        .await
}

#[derive(sqlx::FromRow)]
struct LeverantörsRad {
    id: i64,
    status: String,
    namn: String,
    orgnr: String,
    gatuadress: String,
    postnummer: String,
    postort: String,
    land: String,
    momsregnr: String,
    bankgiro: String,
    plusgiro: String,
    iban: String,
    bic: String,
    standardkonto: Option<i64>,
    omvand_skattskyldighet: String,
    har_fakturor: bool,
}

impl LeverantörsRad {
    fn leverantör(self) -> Result<Leverantör, DatabasFel> {
        let fel = |vad: &str| DatabasFel::Korrupt(format!("leverantor.{vad}"));
        let status = match self.status.as_str() {
            "aktiv" => Status::Aktiv,
            "arkiverad" => Status::Arkiverad,
            _ => return Err(fel("status")),
        };
        let standardkonto = self
            .standardkonto
            .map(|nr| {
                u16::try_from(nr)
                    .ok()
                    .and_then(Kontonummer::new)
                    .ok_or_else(|| fel("standardkonto"))
            })
            .transpose()?;
        Ok(Leverantör {
            id: LeverantörsId(self.id),
            status,
            uppgifter: GiltigLeverantör {
                namn: self.namn,
                orgnr: self.orgnr,
                gatuadress: self.gatuadress,
                postnummer: self.postnummer,
                postort: self.postort,
                land: self.land,
                momsregnr: self.momsregnr,
                bankgiro: self.bankgiro,
                plusgiro: self.plusgiro,
                iban: self.iban,
                bic: self.bic,
                standardkonto,
                omvänd_skattskyldighet: OmvändSkattskyldighet::från_kod(
                    &self.omvand_skattskyldighet,
                )
                .ok_or_else(|| fel("omvand_skattskyldighet"))?,
            },
            kan_raderas: !self.har_fakturor,
        })
    }
}

/// Registrets leverantörer (inte de nya från ett utkast), alla i företaget eller bara `id`.
async fn läs(
    conn: &mut SqliteConnection,
    företag: FöretagsId,
    id: Option<LeverantörsId>,
) -> Result<Vec<Leverantör>, DatabasFel> {
    // `har_fakturor` är alltid falskt före leverantörsfakturorna (#145), som prövar sin
    // tabell här.
    let rader: Vec<LeverantörsRad> = sqlx::query_as(
        "SELECT l.id, l.status, l.namn, l.orgnr, l.gatuadress, l.postnummer, l.postort, l.land,
                l.momsregnr, l.bankgiro, l.plusgiro, l.iban, l.bic, l.standardkonto,
                l.omvand_skattskyldighet, 0 AS har_fakturor
         FROM leverantor l
         WHERE l.foretag_id = ?1 AND l.status IN ('aktiv', 'arkiverad')
           AND (?2 IS NULL OR l.id = ?2)
         ORDER BY l.namn COLLATE NOCASE, l.id",
    )
    .bind(företag.0)
    .bind(id.map(|l| l.0))
    .fetch_all(conn)
    .await?;
    rader.into_iter().map(LeverantörsRad::leverantör).collect()
}

async fn läs_en(
    conn: &mut SqliteConnection,
    företag: FöretagsId,
    id: LeverantörsId,
) -> Result<Option<Leverantör>, DatabasFel> {
    Ok(läs(conn, företag, Some(id)).await?.into_iter().next())
}
