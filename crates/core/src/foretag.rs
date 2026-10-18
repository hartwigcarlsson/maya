//! Företag: att lägga upp ett företag med första räkenskapsåret, kontoplanen och åtkomst för
//! den som lade upp det, och att läsa företagets uppgifter.

use std::fmt;
use std::str::FromStr;

use jiff::civil::Date;
use maya_domain::foretag::{
    Bokföringsmetod, Byte, Fältfel, Momsperiod, NyttFöretag, Uppgifter,
    kontrollera_första_årets_slut, Ändringsläge,
};
use maya_domain::kontoplan::bas;
use maya_domain::orgnr::Organisationsnummer;
use maya_domain::rakenskapsar::Period;
use maya_domain::rakenskapsar::Räkenskapsår as DomänÅr;
use sqlx::SqliteConnection;

use crate::aktor::{self, Aktör, Handling};
use crate::atkomst::{self, Ärende, ÅtkomstFel};
use crate::behandlingshistorik::Händelse;
use crate::{Databas, DatabasFel, Klocka, installationslogg, kontoplan};

/// Ett företags id i installationen. Vem som helst kan skriva ett id; `core` avgör om aktören
/// har åtkomst till företaget.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FöretagsId(pub(crate) i64);

impl fmt::Display for FöretagsId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for FöretagsId {
    type Err = std::num::ParseIntError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse().map(FöretagsId)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum FöretagsFel {
    /// Formuläret klarar inte kontrollen i `domain`; samma koder som formuläret visar.
    #[error("formuläret är inte rätt ifyllt")]
    Ogiltigt(Vec<Fältfel>),
    #[error("{0} finns redan i Maya. Be någon som har åtkomst till företaget om en inbjudan.")]
    OrgnrFinns(Organisationsnummer),
    /// Företaget har bokförda verifikat, underlag eller en ingående balans och kan inte raderas
    /// (A1, Å2).
    #[error("företaget har räkenskapsinformation och kan inte raderas")]
    HarRäkenskapsinformation,
    #[error(transparent)]
    Åtkomst(#[from] ÅtkomstFel),
}

impl From<sqlx::Error> for FöretagsFel {
    fn from(e: sqlx::Error) -> Self {
        FöretagsFel::Åtkomst(e.into())
    }
}

impl From<DatabasFel> for FöretagsFel {
    fn from(e: DatabasFel) -> Self {
        FöretagsFel::Åtkomst(e.into())
    }
}

/// Ett företag i en lista, t.ex. företagsväljaren.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Företagsrad {
    pub id: FöretagsId,
    pub orgnr: Organisationsnummer,
    pub namn: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Räkenskapsår {
    pub start: Date,
    pub slut: Date,
    pub perioder: Vec<Period>,
}

/// Ett företags uppgifter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Företag {
    pub id: FöretagsId,
    pub orgnr: Organisationsnummer,
    pub namn: String,
    /// Namn, adress, kontaktuppgifter, säte, momsregistreringsnummer och F-skatt.
    pub uppgifter: Uppgifter,
    /// Räkenskapsåren i ordning.
    pub räkenskapsår: Vec<Räkenskapsår>,
    /// Bokföringsmetoden med gäller-från-datum, äldst först.
    pub bokföringsmetoder: Vec<(Date, Bokföringsmetod)>,
    /// Momsperioden med gäller-från-datum, äldst först.
    pub momsperioder: Vec<(Date, Momsperiod)>,
}

/// Lägger upp ett företag ur formuläret: företaget, första räkenskapsåret med perioder,
/// bokföringsmetod och momsperiod från årets start, en kopia av BAS, åtkomst för aktören och
/// posten i behandlingshistoriken, i en transaktion. Bara webben (beslut i #61).
pub async fn lägg_upp(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    formulär: &NyttFöretag,
) -> Result<FöretagsId, FöretagsFel> {
    if !aktor::tillåter(aktör.kanal, Handling::LäggaUppFöretag) {
        return Err(ÅtkomstFel::KanalenFårInte.into());
    }
    let Some(användare) = aktör.användare else {
        return Err(ÅtkomstFel::IngenÅtkomst.into());
    };
    let nytt = formulär.kontrollera().map_err(FöretagsFel::Ogiltigt)?;
    let nu = klocka.nu();
    // `BEGIN IMMEDIATE` serialiserar uppläggningarna, så kontrollen nedan räcker; UNIQUE på
    // orgnr är skyddsnätet.
    let mut tx = db.pool().begin_with("BEGIN IMMEDIATE").await?;

    let finns: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM foretag WHERE orgnr = ?)")
        .bind(nytt.orgnr.siffror())
        .fetch_one(&mut *tx)
        .await?;
    if finns {
        return Err(FöretagsFel::OrgnrFinns(nytt.orgnr));
    }
    let id = FöretagsId(
        sqlx::query_scalar(
            "INSERT INTO foretag (orgnr, namn, skapad) VALUES (?, ?, ?) RETURNING id",
        )
        .bind(nytt.orgnr.siffror())
        .bind(&nytt.namn)
        .bind(nu.to_string())
        .fetch_one(&mut *tx)
        .await?,
    );
    sqlx::query("INSERT INTO atkomst (foretag_id, anvandare_id) VALUES (?, ?)")
        .bind(id.0)
        .bind(användare.0)
        .execute(&mut *tx)
        .await?;
    let år = &nytt.första_år;
    lägg_till_räkenskapsår(&mut tx, id, år.start(), år.slut(), &år.perioder()).await?;
    let start = år.start().to_string();
    sqlx::query("INSERT INTO bokforingsmetod (foretag_id, galler_fran, metod) VALUES (?, ?, ?)")
        .bind(id.0)
        .bind(&start)
        .bind(nytt.bokföringsmetod.kod())
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO momsperiod (foretag_id, galler_fran, momsperiod) VALUES (?, ?, ?)")
        .bind(id.0)
        .bind(&start)
        .bind(nytt.momsperiod.kod())
        .execute(&mut *tx)
        .await?;
    kontoplan::kopiera_bas(&mut tx, id).await?;

    let mut ärende = Ärende::för_nytt_företag(tx, aktör, id, nu);
    ärende
        .logga(
            Händelse::FöretagSkapat,
            &nytt.orgnr.to_string(),
            serde_json::json!({
                "orgnr": nytt.orgnr.to_string(),
                "namn": nytt.namn,
                "första_räkenskapsår": { "start": start, "slut": år.slut().to_string() },
                "bokföringsmetod": nytt.bokföringsmetod.kod(),
                "momsperiod": nytt.momsperiod.kod(),
                "kontoplan": bas::VERSION,
            }),
        )
        .await?;
    // Skaparen får åtkomst som vem som helst, och det står i historiken.
    let epost: String = sqlx::query_scalar("SELECT epost FROM anvandare WHERE id = ?")
        .bind(användare.0)
        .fetch_one(ärende.conn())
        .await?;
    ärende
        .logga(
            Händelse::ÅtkomstGiven,
            &epost,
            serde_json::json!({ "användare": användare.0, "epost": epost }),
        )
        .await?;
    ärende.slutför().await?;
    Ok(id)
}

pub(crate) async fn lägg_till_räkenskapsår(
    conn: &mut SqliteConnection,
    företag: FöretagsId,
    start: Date,
    slut: Date,
    perioder: &[Period],
) -> Result<i64, sqlx::Error> {
    let år: i64 = sqlx::query_scalar(
        "INSERT INTO rakenskapsar (foretag_id, start, slut) VALUES (?, ?, ?) RETURNING id",
    )
    .bind(företag.0)
    .bind(start.to_string())
    .bind(slut.to_string())
    .fetch_one(&mut *conn)
    .await?;
    for period in perioder {
        sqlx::query("INSERT INTO period (rakenskapsar_id, start, slut) VALUES (?, ?, ?)")
            .bind(år)
            .bind(period.start.to_string())
            .bind(period.slut.to_string())
            .execute(&mut *conn)
            .await?;
    }
    Ok(år)
}

/// Företagen som aktören har åtkomst till, i namnordning.
pub async fn mina_företag(db: &Databas, aktör: &Aktör) -> Result<Vec<Företagsrad>, DatabasFel> {
    let Some(användare) = aktör.användare else {
        return Ok(Vec::new());
    };
    if !aktor::tillåter(aktör.kanal, Handling::LäsaFöretag) {
        return Ok(Vec::new());
    }
    let rader: Vec<(i64, String, String)> = sqlx::query_as(
        "SELECT f.id, f.orgnr, f.namn FROM foretag f
         JOIN atkomst a ON a.foretag_id = f.id
         JOIN anvandare u ON u.id = a.anvandare_id
         WHERE a.anvandare_id = ? AND u.avaktiverad IS NULL
         ORDER BY f.namn COLLATE NOCASE, f.id",
    )
    .bind(användare.0)
    .fetch_all(db.pool())
    .await?;
    rader
        .into_iter()
        .map(|(id, orgnr, namn)| {
            Ok(Företagsrad {
                id: FöretagsId(id),
                orgnr: tolka_orgnr(&orgnr)?,
                namn,
            })
        })
        .collect()
}

/// Företagets uppgifter, räkenskapsår och historiken för bokföringsmetod och momsperiod.
pub async fn företaget(
    db: &Databas,
    aktör: &Aktör,
    id: FöretagsId,
) -> Result<Företag, ÅtkomstFel> {
    let mut tx = db.pool().begin().await?;
    atkomst::kräv(&mut tx, aktör, id, Handling::LäsaFöretag).await?;
    Ok(läs_företag(&mut tx, id).await?)
}

async fn läs_företag(
    conn: &mut SqliteConnection,
    id: FöretagsId,
) -> Result<Företag, DatabasFel> {
    let (orgnr, uppgifter) = läs_uppgifter(&mut *conn, id).await?;
    let år: Vec<(i64, String, String)> = sqlx::query_as(
        "SELECT id, start, slut FROM rakenskapsar WHERE foretag_id = ? ORDER BY start",
    )
    .bind(id.0)
    .fetch_all(&mut *conn)
    .await?;
    let mut räkenskapsår = Vec::new();
    for (år_id, start, slut) in år {
        let perioder: Vec<(String, String)> = sqlx::query_as(
            "SELECT start, slut FROM period WHERE rakenskapsar_id = ? ORDER BY start",
        )
        .bind(år_id)
        .fetch_all(&mut *conn)
        .await?;
        räkenskapsår.push(Räkenskapsår {
            start: datum(&start)?,
            slut: datum(&slut)?,
            perioder: perioder
                .iter()
                .map(|(start, slut)| {
                    Ok(Period {
                        start: datum(start)?,
                        slut: datum(slut)?,
                    })
                })
                .collect::<Result<_, DatabasFel>>()?,
        });
    }
    let metoder: Vec<(String, String)> = sqlx::query_as(
        "SELECT galler_fran, metod FROM bokforingsmetod WHERE foretag_id = ? ORDER BY galler_fran",
    )
    .bind(id.0)
    .fetch_all(&mut *conn)
    .await?;
    let momsperioder: Vec<(String, String)> = sqlx::query_as(
        "SELECT galler_fran, momsperiod FROM momsperiod WHERE foretag_id = ? ORDER BY galler_fran",
    )
    .bind(id.0)
    .fetch_all(&mut *conn)
    .await?;
    let korrupt = |vad: &str| DatabasFel::Korrupt(vad.to_string());
    Ok(Företag {
        id,
        orgnr,
        namn: uppgifter.namn.clone(),
        uppgifter,
        räkenskapsår,
        bokföringsmetoder: metoder
            .iter()
            .map(|(från, kod)| {
                Ok((
                    datum(från)?,
                    Bokföringsmetod::från_kod(kod).ok_or_else(|| korrupt("bokforingsmetod"))?,
                ))
            })
            .collect::<Result<_, DatabasFel>>()?,
        momsperioder: momsperioder
            .iter()
            .map(|(från, kod)| {
                Ok((
                    datum(från)?,
                    Momsperiod::från_kod(kod).ok_or_else(|| korrupt("momsperiod"))?,
                ))
            })
            .collect::<Result<_, DatabasFel>>()?,
    })
}

type UppgiftsRad = (
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    bool,
);

async fn läs_uppgifter(
    conn: &mut SqliteConnection,
    id: FöretagsId,
) -> Result<(Organisationsnummer, Uppgifter), DatabasFel> {
    let (orgnr, namn, gatuadress, postnummer, postort, epost, telefon, säte, momsregnr, f_skatt): UppgiftsRad =
        sqlx::query_as(
            "SELECT orgnr, namn, gatuadress, postnummer, postort, epost, telefon, sate, momsregnr,
                    f_skatt
             FROM foretag WHERE id = ?",
        )
        .bind(id.0)
        .fetch_one(conn)
        .await?;
    Ok((
        tolka_orgnr(&orgnr)?,
        Uppgifter {
            namn,
            gatuadress,
            postnummer,
            postort,
            epost,
            telefon,
            säte,
            momsregnr,
            f_skatt,
        },
    ))
}

/// Ändrar företagets uppgifter. Det som ändrades loggas i behandlingshistoriken som
/// `företag ändrat`, med från- och till-värde per fält; utan ändring loggas inget.
pub async fn ändra_uppgifter(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    id: FöretagsId,
    formulär: &Uppgifter,
) -> Result<(), FöretagsFel> {
    let mut ärende = Ärende::börja(db, klocka, aktör, id, Handling::ÄndraFöretag).await?;
    let nya = formulär.kontrollera().map_err(FöretagsFel::Ogiltigt)?;
    let (orgnr, gamla) = läs_uppgifter(ärende.conn(), id).await?;
    let mut ändrat = serde_json::Map::new();
    let mut jämför = |fält: &str, från: serde_json::Value, till: serde_json::Value| {
        if från != till {
            ändrat.insert(
                fält.into(),
                serde_json::json!({ "från": från, "till": till }),
            );
        }
    };
    jämför("namn", gamla.namn.into(), nya.namn.clone().into());
    jämför(
        "gatuadress",
        gamla.gatuadress.into(),
        nya.gatuadress.clone().into(),
    );
    jämför(
        "postnummer",
        gamla.postnummer.into(),
        nya.postnummer.clone().into(),
    );
    jämför("postort", gamla.postort.into(), nya.postort.clone().into());
    jämför("epost", gamla.epost.into(), nya.epost.clone().into());
    jämför("telefon", gamla.telefon.into(), nya.telefon.clone().into());
    jämför("säte", gamla.säte.into(), nya.säte.clone().into());
    jämför(
        "momsregnr",
        gamla.momsregnr.into(),
        nya.momsregnr.clone().into(),
    );
    jämför("f_skatt", gamla.f_skatt.into(), nya.f_skatt.into());
    if ändrat.is_empty() {
        return Ok(());
    }
    sqlx::query(
        "UPDATE foretag SET namn = ?, gatuadress = ?, postnummer = ?, postort = ?, epost = ?,
             telefon = ?, sate = ?, momsregnr = ?, f_skatt = ?
         WHERE id = ?",
    )
    .bind(&nya.namn)
    .bind(&nya.gatuadress)
    .bind(&nya.postnummer)
    .bind(&nya.postort)
    .bind(&nya.epost)
    .bind(&nya.telefon)
    .bind(&nya.säte)
    .bind(&nya.momsregnr)
    .bind(nya.f_skatt)
    .bind(id.0)
    .execute(ärende.conn())
    .await?;
    ärende
        .logga(
            Händelse::FöretagÄndrat,
            &orgnr.to_string(),
            serde_json::Value::Object(ändrat),
        )
        .await?;
    ärende.slutför().await?;
    Ok(())
}

/// Byter bokföringsmetod från början av ett senare räkenskapsår och loggar
/// `bokföringsmetod bytt`. Reglerna finns i [`Byte::kontrollera_bokföringsmetod`].
pub async fn byt_bokföringsmetod(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    id: FöretagsId,
    byte: &Byte,
) -> Result<(), FöretagsFel> {
    let mut ärende = Ärende::börja(db, klocka, aktör, id, Handling::ÄndraFöretag).await?;
    let läge = ändringsläge_i(ärende.conn(), id).await?;
    let (från, metod) = byte
        .kontrollera_bokföringsmetod(&läge)
        .map_err(FöretagsFel::Ogiltigt)?;
    let nu = maya_domain::foretag::gäller(&läge.bokföringsmetoder, från)
        .ok_or_else(|| DatabasFel::Korrupt("bokforingsmetod saknas".into()))?;
    sqlx::query("INSERT INTO bokforingsmetod (foretag_id, galler_fran, metod) VALUES (?, ?, ?)")
        .bind(id.0)
        .bind(från.to_string())
        .bind(metod.kod())
        .execute(ärende.conn())
        .await?;
    ärende
        .logga(
            Händelse::BokföringsmetodBytt,
            &från.to_string(),
            serde_json::json!({
                "gäller_från": från.to_string(),
                "från": nu.kod(),
                "till": metod.kod(),
            }),
        )
        .await?;
    ärende.slutför().await?;
    Ok(())
}

/// Byter momsperiod från början av en momsperiod och loggar `momsperiod bytt`. Reglerna finns
/// i [`Byte::kontrollera_momsperiod`].
pub async fn byt_momsperiod(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    id: FöretagsId,
    byte: &Byte,
) -> Result<(), FöretagsFel> {
    let mut ärende = Ärende::börja(db, klocka, aktör, id, Handling::ÄndraFöretag).await?;
    let läge = ändringsläge_i(ärende.conn(), id).await?;
    let (från, ny) = byte
        .kontrollera_momsperiod(&läge)
        .map_err(FöretagsFel::Ogiltigt)?;
    let nu = maya_domain::foretag::gäller(&läge.momsperioder, från)
        .ok_or_else(|| DatabasFel::Korrupt("momsperiod saknas".into()))?;
    sqlx::query("INSERT INTO momsperiod (foretag_id, galler_fran, momsperiod) VALUES (?, ?, ?)")
        .bind(id.0)
        .bind(från.to_string())
        .bind(ny.kod())
        .execute(ärende.conn())
        .await?;
    ärende
        .logga(
            Händelse::MomsperiodBytt,
            &från.to_string(),
            serde_json::json!({
                "gäller_från": från.to_string(),
                "från": nu.kod(),
                "till": ny.kod(),
            }),
        )
        .await?;
    ärende.slutför().await?;
    Ok(())
}

/// Flyttar slutet på företagets första räkenskapsår, lägger om dess perioder och loggar
/// `räkenskapsår ändrat`. Reglerna finns i [`kontrollera_första_årets_slut`].
pub async fn ändra_första_årets_slut(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    id: FöretagsId,
    slut: &str,
) -> Result<(), FöretagsFel> {
    let mut ärende = Ärende::börja(db, klocka, aktör, id, Handling::ÄndraFöretag).await?;
    let läge = ändringsläge_i(ärende.conn(), id).await?;
    let år = kontrollera_första_årets_slut(slut, &läge).map_err(FöretagsFel::Ogiltigt)?;
    let gammalt = läge.räkenskapsår[0].slut();
    if gammalt == år.slut() {
        return Ok(());
    }
    let år_id: i64 =
        sqlx::query_scalar("SELECT id FROM rakenskapsar WHERE foretag_id = ? AND start = ?")
            .bind(id.0)
            .bind(år.start().to_string())
            .fetch_one(ärende.conn())
            .await?;
    sqlx::query("UPDATE rakenskapsar SET slut = ? WHERE id = ?")
        .bind(år.slut().to_string())
        .bind(år_id)
        .execute(ärende.conn())
        .await?;
    sqlx::query("DELETE FROM period WHERE rakenskapsar_id = ?")
        .bind(år_id)
        .execute(ärende.conn())
        .await?;
    for period in år.perioder() {
        sqlx::query("INSERT INTO period (rakenskapsar_id, start, slut) VALUES (?, ?, ?)")
            .bind(år_id)
            .bind(period.start.to_string())
            .bind(period.slut.to_string())
            .execute(ärende.conn())
            .await?;
    }
    ärende
        .logga(
            Händelse::RäkenskapsårÄndrat,
            &år.start().to_string(),
            serde_json::json!({
                "start": år.start().to_string(),
                "slut": { "från": gammalt.to_string(), "till": år.slut().to_string() },
            }),
        )
        .await?;
    ärende.slutför().await?;
    Ok(())
}

/// Raderar ett företag som saknar räkenskapsinformation (bokförda verifikat och underlag;
/// fakturor när de finns) med kontoplan, räkenskapsår, utkast, åtkomst och
/// behandlingshistorik. Raderingen skrivs i installationsloggen som `företag raderat` med
/// orgnr och namn, aktör och tid, i samma transaktion.
pub async fn radera(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    id: FöretagsId,
) -> Result<(), FöretagsFel> {
    let mut ärende = Ärende::börja(db, klocka, aktör, id, Handling::RaderaFöretag).await?;
    let nu = ärende.nu;
    let conn = ärende.conn();
    if har_räkenskapsinformation_i(&mut *conn, id).await? {
        return Err(FöretagsFel::HarRäkenskapsinformation);
    }
    let (orgnr, uppgifter) = läs_uppgifter(&mut *conn, id).await?;
    let åtkomst: Vec<i64> =
        sqlx::query_scalar("SELECT anvandare_id FROM atkomst WHERE foretag_id = ?")
            .bind(id.0)
            .fetch_all(&mut *conn)
            .await?;
    // Företaget, kontoplanen och behandlingshistoriken går bara att radera med en markering i
    // `foretag_radering`, som schemat bara godtar för ett företag utan räkenskapsinformation.
    // Företagsraden tas bort före kontoplanen och historiken; främmande nycklar prövas vid
    // COMMIT. Markeringen tas bort i samma transaktion.
    sqlx::query("PRAGMA defer_foreign_keys = ON")
        .execute(&mut *conn)
        .await?;
    for sql in [
        "INSERT INTO foretag_radering (foretag_id) VALUES (?1)",
        // Inga underlag finns (de är räkenskapsinformation), men listan täcker varje tabell
        // med företagets data.
        "DELETE FROM underlagskoppling
         WHERE underlag_id IN (SELECT id FROM underlag WHERE foretag_id = ?1)",
        "DELETE FROM underlag WHERE foretag_id = ?1",
        "DELETE FROM verifikatrad
         WHERE verifikat_id IN (SELECT id FROM verifikat WHERE foretag_id = ?1)",
        "DELETE FROM verifikat WHERE foretag_id = ?1",
        "DELETE FROM period
         WHERE rakenskapsar_id IN (SELECT id FROM rakenskapsar WHERE foretag_id = ?1)",
        "DELETE FROM ingaende_balans
         WHERE rakenskapsar_id IN (SELECT id FROM rakenskapsar WHERE foretag_id = ?1)",
        "DELETE FROM rakenskapsar WHERE foretag_id = ?1",
        "DELETE FROM bokforingsmetod WHERE foretag_id = ?1",
        "DELETE FROM momsperiod WHERE foretag_id = ?1",
        "DELETE FROM atkomst WHERE foretag_id = ?1",
        "DELETE FROM inbjudan WHERE foretag_id = ?1",
        // #133 Kundregister: kunder utan fakturor.
        "DELETE FROM kund WHERE foretag_id = ?1",
        "DELETE FROM foretag WHERE id = ?1",
        "DELETE FROM konto WHERE foretag_id = ?1",
        "DELETE FROM behandlingshistorik WHERE foretag_id = ?1",
        "DELETE FROM foretag_radering WHERE foretag_id = ?1",
    ] {
        sqlx::query(sql).bind(id.0).execute(&mut *conn).await?;
    }
    let logg_id = installationslogg::skriv(
        &mut *conn,
        nu,
        aktör,
        installationslogg::Händelse::FöretagRaderat,
        &orgnr.to_string(),
        serde_json::json!({ "orgnr": orgnr.to_string(), "namn": uppgifter.namn }),
    )
    .await?;
    // Raderingen syns på sidan Installation för dem som hade åtkomst (story 36).
    installationslogg::skriv_åtkomst(&mut *conn, logg_id, &åtkomst).await?;
    ärende.slutför().await?;
    Ok(())
}

/// Om företaget har räkenskapsinformation (bokförda verifikat, underlag och en inskriven
/// ingående balans, Å2; fakturor när de finns), så att det inte kan raderas.
pub async fn har_räkenskapsinformation(
    db: &Databas,
    aktör: &Aktör,
    id: FöretagsId,
) -> Result<bool, ÅtkomstFel> {
    let mut tx = db.pool().begin().await?;
    atkomst::kräv(&mut tx, aktör, id, Handling::LäsaFöretag).await?;
    Ok(har_räkenskapsinformation_i(&mut tx, id).await?)
}

async fn har_räkenskapsinformation_i(
    conn: &mut SqliteConnection,
    id: FöretagsId,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM verifikat WHERE foretag_id = ?1 AND bokford IS NOT NULL)
             OR EXISTS (SELECT 1 FROM underlag WHERE foretag_id = ?1)
             OR EXISTS (SELECT 1 FROM ingaende_balans b
                        JOIN rakenskapsar r ON r.id = b.rakenskapsar_id
                        WHERE r.foretag_id = ?1)",
    )
    .bind(id.0)
    .fetch_one(conn)
    .await
}

/// Det ett byte prövas mot, för företaget som aktören redan har åtkomst till.
pub async fn ändringsläge(
    db: &Databas,
    aktör: &Aktör,
    id: FöretagsId,
) -> Result<Ändringsläge, ÅtkomstFel> {
    let mut tx = db.pool().begin().await?;
    atkomst::kräv(&mut tx, aktör, id, Handling::LäsaFöretag).await?;
    Ok(ändringsläge_i(&mut tx, id).await?)
}

async fn ändringsläge_i(
    conn: &mut SqliteConnection,
    id: FöretagsId,
) -> Result<Ändringsläge, DatabasFel> {
    let företag = läs_företag(&mut *conn, id).await?;
    let (bokförda, daterade): (Option<String>, Option<String>) = sqlx::query_as(
        "SELECT max(CASE WHEN bokford IS NOT NULL THEN datum END), max(datum)
         FROM verifikat WHERE foretag_id = ?",
    )
    .bind(id.0)
    .fetch_one(&mut *conn)
    .await?;
    Ok(Ändringsläge {
        räkenskapsår: företag
            .räkenskapsår
            .iter()
            .map(|år| DomänÅr::befintligt(år.start, år.slut))
            .collect(),
        bokföringsmetoder: företag.bokföringsmetoder,
        momsperioder: företag.momsperioder,
        senast_bokförda: bokförda.as_deref().map(datum).transpose()?,
        senast_daterade: daterade.as_deref().map(datum).transpose()?,
    })
}

fn tolka_orgnr(siffror: &str) -> Result<Organisationsnummer, DatabasFel> {
    Organisationsnummer::tolka(siffror).map_err(|_| DatabasFel::Korrupt("foretag.orgnr".into()))
}

pub(crate) fn datum(text: &str) -> Result<Date, DatabasFel> {
    text.parse()
        .map_err(|_| DatabasFel::Korrupt(format!("ett datum: {text}")))
}
