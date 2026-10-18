//! Kundregistret: företagets kunder, som kundfakturorna hänvisar till. En kund kan ändras när
//! som helst, arkiveras så att den inte visas i väljarna, och raderas om den saknar fakturor.
//! Varje skapande, ändring, arkivering och radering loggas i behandlingshistoriken (H2).
//! Reglerna för formuläret finns i [`maya_domain::kund`].

use std::fmt;
use std::str::FromStr;

use jiff::Timestamp;
use maya_domain::kund::{self as regler, Fältfel, GiltigKund, NyKund, OmvändSkattskyldighet};
use serde_json::{Value, json};
use sqlx::SqliteConnection;

use crate::aktor::{Aktör, Handling};
use crate::atkomst::{self, Ärende, ÅtkomstFel};
use crate::behandlingshistorik::Händelse;
use crate::foretag::FöretagsId;
use crate::{Databas, DatabasFel, Klocka};

/// En kunds id i installationen. `core` avgör om kunden hör till företaget.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct KundId(pub(crate) i64);

impl fmt::Display for KundId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for KundId {
    type Err = std::num::ParseIntError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse().map(KundId)
    }
}

/// Om kunden visas i väljarna.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kundstatus {
    Aktiv,
    /// Kvar i registret men inte i väljarna.
    Arkiverad,
}

impl Kundstatus {
    fn kod(self) -> &'static str {
        match self {
            Kundstatus::Aktiv => "aktiv",
            Kundstatus::Arkiverad => "arkiverad",
        }
    }

    fn från_kod(kod: &str) -> Option<Self> {
        [Kundstatus::Aktiv, Kundstatus::Arkiverad]
            .into_iter()
            .find(|s| s.kod() == kod)
    }
}

/// En kund i registret. Tom text betyder att uppgiften saknas.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Kund {
    pub id: KundId,
    pub namn: String,
    /// Organisationsnummer eller personnummer.
    pub identitet: String,
    pub adress: String,
    pub epost: String,
    pub momsregnr: String,
    /// Dagar från fakturadatum till förfallodag.
    pub betalningsvillkor: u16,
    pub omvänd_skattskyldighet: OmvändSkattskyldighet,
    pub status: Kundstatus,
    pub skapad: Timestamp,
}

impl Kund {
    /// Kunden som formulär, för att ändra den.
    pub fn formulär(&self) -> NyKund {
        NyKund {
            namn: self.namn.clone(),
            identitet: self.identitet.clone(),
            adress: self.adress.clone(),
            epost: self.epost.clone(),
            momsregnr: self.momsregnr.clone(),
            betalningsvillkor: self.betalningsvillkor.to_string(),
            omvänd_skattskyldighet: self.omvänd_skattskyldighet.kod().into(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum KundFel {
    /// Formuläret klarar inte kontrollen i `domain`; samma koder som formuläret visar.
    #[error("formuläret är inte rätt ifyllt")]
    Ogiltigt(Vec<Fältfel>),
    /// Också när kunden hör till ett annat företag.
    #[error("kunden finns inte")]
    FinnsInte,
    #[error("{0}")]
    KanInteRaderas(regler::Raderingsspärr),
    #[error(transparent)]
    Åtkomst(#[from] ÅtkomstFel),
}

impl From<sqlx::Error> for KundFel {
    fn from(e: sqlx::Error) -> Self {
        KundFel::Åtkomst(e.into())
    }
}

impl From<DatabasFel> for KundFel {
    fn from(e: DatabasFel) -> Self {
        KundFel::Åtkomst(e.into())
    }
}

/// Frågor som räknar en kunds fakturor, en per tabell med fakturor som hänvisar till kunden
/// (`?1` är kundens id). Var och en av fakturornas tickets (#135, #143) lägger till sin fråga
/// här, så att en kund med fakturor inte kan raderas.
const KUNDENS_FAKTUROR: &[&str] = &[
    // #135 Kundfakturor, också utkast: de hänvisar till kunden.
    "SELECT count(*) FROM kundfaktura WHERE kund_id = ?1",
];

/// Lägger upp en kund i registret och loggar `kund skapad` med uppgifterna.
pub async fn skapa(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    företag: FöretagsId,
    formulär: &NyKund,
) -> Result<KundId, KundFel> {
    let mut ärende = Ärende::börja(db, klocka, aktör, företag, Handling::HanteraKunder).await?;
    let k = formulär.kontrollera().map_err(KundFel::Ogiltigt)?;
    let nu = ärende.nu;
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO kund (foretag_id, namn, identitet, adress, epost, momsregnr,
             betalningsvillkor, omvand_skattskyldighet, status, skapad, skapad_kanal, skapad_av)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, 'aktiv', ?, ?, ?)
         RETURNING id",
    )
    .bind(företag.0)
    .bind(&k.namn)
    .bind(&k.identitet)
    .bind(&k.adress)
    .bind(&k.epost)
    .bind(&k.momsregnr)
    .bind(i64::from(k.betalningsvillkor))
    .bind(k.omvänd_skattskyldighet.kod())
    .bind(nu.to_string())
    .bind(aktör.kanal.namn())
    .bind(aktör.användare.map(|a| a.0))
    .fetch_one(ärende.conn())
    .await?;
    ärende
        .logga(Händelse::KundSkapad, &k.namn, uppgifter(id, &k))
        .await?;
    ärende.slutför().await?;
    Ok(KundId(id))
}

/// Ändrar en kund, också en arkiverad. Det som ändrades loggas som `kund ändrad`, med från- och
/// till-värde per fält; utan ändring loggas inget.
pub async fn ändra(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    företag: FöretagsId,
    id: KundId,
    formulär: &NyKund,
) -> Result<(), KundFel> {
    let mut ärende = Ärende::börja(db, klocka, aktör, företag, Handling::HanteraKunder).await?;
    let gamla = läs(ärende.conn(), företag, id).await?;
    let nya = formulär.kontrollera().map_err(KundFel::Ogiltigt)?;
    let mut ändrat = serde_json::Map::new();
    let mut jämför = |fält: &str, från: Value, till: Value| {
        if från != till {
            ändrat.insert(fält.into(), json!({ "från": från, "till": till }));
        }
    };
    jämför("namn", gamla.namn.into(), nya.namn.clone().into());
    jämför(
        "identitet",
        gamla.identitet.into(),
        nya.identitet.clone().into(),
    );
    jämför("adress", gamla.adress.into(), nya.adress.clone().into());
    jämför("epost", gamla.epost.into(), nya.epost.clone().into());
    jämför(
        "momsregnr",
        gamla.momsregnr.into(),
        nya.momsregnr.clone().into(),
    );
    jämför(
        "betalningsvillkor",
        gamla.betalningsvillkor.into(),
        nya.betalningsvillkor.into(),
    );
    jämför(
        "omvänd_skattskyldighet",
        gamla.omvänd_skattskyldighet.kod().into(),
        nya.omvänd_skattskyldighet.kod().into(),
    );
    if ändrat.is_empty() {
        return Ok(());
    }
    sqlx::query(
        "UPDATE kund SET namn = ?, identitet = ?, adress = ?, epost = ?, momsregnr = ?,
             betalningsvillkor = ?, omvand_skattskyldighet = ?
         WHERE id = ? AND foretag_id = ?",
    )
    .bind(&nya.namn)
    .bind(&nya.identitet)
    .bind(&nya.adress)
    .bind(&nya.epost)
    .bind(&nya.momsregnr)
    .bind(i64::from(nya.betalningsvillkor))
    .bind(nya.omvänd_skattskyldighet.kod())
    .bind(id.0)
    .bind(företag.0)
    .execute(ärende.conn())
    .await?;
    ärende
        .logga(
            Händelse::KundÄndrad,
            &nya.namn,
            json!({ "id": id.0, "ändringar": Value::Object(ändrat) }),
        )
        .await?;
    ärende.slutför().await?;
    Ok(())
}

/// Arkiverar en kund så att den inte visas i väljarna men finns kvar i registret, och loggar
/// `kund arkiverad`. En redan arkiverad kund lämnas som den är.
pub async fn arkivera(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    företag: FöretagsId,
    id: KundId,
) -> Result<(), KundFel> {
    let mut ärende = Ärende::börja(db, klocka, aktör, företag, Handling::HanteraKunder).await?;
    let kund = läs(ärende.conn(), företag, id).await?;
    if kund.status == Kundstatus::Arkiverad {
        return Ok(());
    }
    sqlx::query("UPDATE kund SET status = 'arkiverad' WHERE id = ? AND foretag_id = ?")
        .bind(id.0)
        .bind(företag.0)
        .execute(ärende.conn())
        .await?;
    ärende
        .logga(Händelse::KundArkiverad, &kund.namn, json!({ "id": id.0 }))
        .await?;
    ärende.slutför().await?;
    Ok(())
}

/// Raderar en kund som saknar fakturor och loggar `kund raderad` med kundens uppgifter. En kund
/// med fakturor arkiveras i stället.
pub async fn radera(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    företag: FöretagsId,
    id: KundId,
) -> Result<(), KundFel> {
    let mut ärende = Ärende::börja(db, klocka, aktör, företag, Handling::HanteraKunder).await?;
    let kund = läs(ärende.conn(), företag, id).await?;
    regler::kan_raderas(antal_fakturor(ärende.conn(), id).await?)
        .map_err(KundFel::KanInteRaderas)?;
    sqlx::query("DELETE FROM kund WHERE id = ? AND foretag_id = ?")
        .bind(id.0)
        .bind(företag.0)
        .execute(ärende.conn())
        .await?;
    let giltig = GiltigKund {
        namn: kund.namn.clone(),
        identitet: kund.identitet,
        adress: kund.adress,
        epost: kund.epost,
        momsregnr: kund.momsregnr,
        betalningsvillkor: kund.betalningsvillkor,
        omvänd_skattskyldighet: kund.omvänd_skattskyldighet,
    };
    ärende
        .logga(Händelse::KundRaderad, &kund.namn, uppgifter(id.0, &giltig))
        .await?;
    ärende.slutför().await?;
    Ok(())
}

/// Hela registret, aktiva och arkiverade kunder, i namnordning.
pub async fn kunder(
    db: &Databas,
    aktör: &Aktör,
    företag: FöretagsId,
) -> Result<Vec<Kund>, ÅtkomstFel> {
    let mut tx = db.pool().begin().await?;
    atkomst::kräv(&mut tx, aktör, företag, Handling::LäsaFöretag).await?;
    lista(&mut tx, företag, "status IN ('aktiv', 'arkiverad')").await
}

/// Kunderna som väljarna visar: de aktiva, i namnordning.
pub async fn väljbara(
    db: &Databas,
    aktör: &Aktör,
    företag: FöretagsId,
) -> Result<Vec<Kund>, ÅtkomstFel> {
    let mut tx = db.pool().begin().await?;
    atkomst::kräv(&mut tx, aktör, företag, Handling::LäsaFöretag).await?;
    lista(&mut tx, företag, "status = 'aktiv'").await
}

/// En kund i företagets register.
pub async fn kunden(
    db: &Databas,
    aktör: &Aktör,
    företag: FöretagsId,
    id: KundId,
) -> Result<Kund, KundFel> {
    let mut tx = db.pool().begin().await?;
    atkomst::kräv(&mut tx, aktör, företag, Handling::LäsaFöretag).await?;
    läs(&mut tx, företag, id).await
}

/// Om kunden har fakturor, så att den bara kan arkiveras.
pub async fn har_fakturor(
    db: &Databas,
    aktör: &Aktör,
    företag: FöretagsId,
    id: KundId,
) -> Result<bool, KundFel> {
    let mut tx = db.pool().begin().await?;
    atkomst::kräv(&mut tx, aktör, företag, Handling::LäsaFöretag).await?;
    läs(&mut tx, företag, id).await?;
    Ok(antal_fakturor(&mut tx, id).await? > 0)
}

async fn antal_fakturor(conn: &mut SqliteConnection, id: KundId) -> Result<u64, sqlx::Error> {
    let mut antal = 0;
    for &sql in KUNDENS_FAKTUROR {
        let n: i64 = sqlx::query_scalar(sql)
            .bind(id.0)
            .fetch_one(&mut *conn)
            .await?;
        antal += n.unsigned_abs();
    }
    Ok(antal)
}

type Rad = (
    i64,
    String,
    String,
    String,
    String,
    String,
    i64,
    String,
    String,
    String,
);

const KOLUMNER: &str = "id, namn, identitet, adress, epost, momsregnr, betalningsvillkor,
     omvand_skattskyldighet, status, skapad";

async fn lista(
    conn: &mut SqliteConnection,
    företag: FöretagsId,
    villkor: &'static str,
) -> Result<Vec<Kund>, ÅtkomstFel> {
    // `villkor` är en konstant i den här modulen, aldrig indata.
    let rader: Vec<Rad> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {KOLUMNER} FROM kund WHERE foretag_id = ? AND {villkor}
         ORDER BY namn COLLATE NOCASE, id"
    )))
    .bind(företag.0)
    .fetch_all(conn)
    .await?;
    Ok(rader.into_iter().map(kund).collect::<Result<_, _>>()?)
}

/// Kunden om den finns i företagets register; en föreslagen kund (status `ny`) hör inte dit.
async fn läs(
    conn: &mut SqliteConnection,
    företag: FöretagsId,
    id: KundId,
) -> Result<Kund, KundFel> {
    let rad: Option<Rad> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {KOLUMNER} FROM kund
         WHERE id = ? AND foretag_id = ? AND status IN ('aktiv', 'arkiverad')"
    )))
    .bind(id.0)
    .bind(företag.0)
    .fetch_optional(conn)
    .await?;
    Ok(kund(rad.ok_or(KundFel::FinnsInte)?)?)
}

fn kund(
    (id, namn, identitet, adress, epost, momsregnr, villkor, omvänd, status, skapad): Rad,
) -> Result<Kund, DatabasFel> {
    let korrupt = |vad: &str| DatabasFel::Korrupt(format!("kund {id}: {vad}"));
    Ok(Kund {
        id: KundId(id),
        namn,
        identitet,
        adress,
        epost,
        momsregnr,
        betalningsvillkor: u16::try_from(villkor).map_err(|_| korrupt("betalningsvillkor"))?,
        omvänd_skattskyldighet: OmvändSkattskyldighet::från_kod(&omvänd)
            .ok_or_else(|| korrupt("omvand_skattskyldighet"))?,
        status: Kundstatus::från_kod(&status).ok_or_else(|| korrupt("status"))?,
        skapad: skapad.parse().map_err(|_| korrupt("skapad"))?,
    })
}

/// Kundens uppgifter i behandlingshistoriken.
fn uppgifter(id: i64, k: &GiltigKund) -> Value {
    json!({
        "id": id,
        "namn": k.namn,
        "identitet": k.identitet,
        "adress": k.adress,
        "epost": k.epost,
        "momsregnr": k.momsregnr,
        "betalningsvillkor": k.betalningsvillkor,
        "omvänd_skattskyldighet": k.omvänd_skattskyldighet.kod(),
    })
}
