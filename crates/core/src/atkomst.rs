//! Åtkomst: om en aktör får röra ett företag. Varje användningsfall som rör ett företag börjar
//! här, så att `core` auktoriserar och inte bara webben (ADR 0003).
//!
//! Ett användningsfall som skriver börjar ett [`Ärende`]: kanalpolicyn och åtkomsten kontrolleras,
//! och ändringen och dess post i behandlingshistoriken görs i samma SQLite-transaktion. Ett
//! användningsfall som bara läser anropar [`kräv`] i sin egen transaktion.

use jiff::Timestamp;
use sqlx::{Sqlite, SqliteConnection, Transaction};

use crate::aktor::{self, Aktör, AnvändarId, Handling};
use crate::behandlingshistorik::{self, Händelse};
use crate::foretag::FöretagsId;
use crate::inloggning::{
    Inloggad, InloggningsFel, LÄNK_GILTIG, Registrant, Registreringslänk, Token, hash, tidpunkt,
};
use crate::{Databas, DatabasFel, Klocka};

#[derive(Debug, thiserror::Error)]
pub enum ÅtkomstFel {
    #[error("kanalen får inte göra det här")]
    KanalenFårInte,
    /// Också när företaget inte finns, så att ingen kan pröva sig fram till vilka företag som
    /// finns.
    #[error("du har inte åtkomst till företaget")]
    IngenÅtkomst,
    #[error(transparent)]
    Databas(#[from] DatabasFel),
}

impl From<sqlx::Error> for ÅtkomstFel {
    fn from(e: sqlx::Error) -> Self {
        ÅtkomstFel::Databas(e.into())
    }
}

/// Kontrollerar kanalpolicyn för `handling` och att aktören är en aktiv användare med åtkomst
/// till företaget.
pub(crate) async fn kräv(
    conn: &mut SqliteConnection,
    aktör: &Aktör,
    företag: FöretagsId,
    handling: Handling,
) -> Result<(), ÅtkomstFel> {
    if !aktor::tillåter(aktör.kanal, handling) {
        return Err(ÅtkomstFel::KanalenFårInte);
    }
    let Some(användare) = aktör.användare else {
        return Err(ÅtkomstFel::IngenÅtkomst);
    };
    let har: bool = sqlx::query_scalar(
        "SELECT EXISTS (
             SELECT 1 FROM atkomst a JOIN anvandare u ON u.id = a.anvandare_id
             WHERE a.foretag_id = ? AND a.anvandare_id = ? AND u.avaktiverad IS NULL)",
    )
    .bind(företag.0)
    .bind(användare.0)
    .fetch_one(conn)
    .await?;
    if har {
        Ok(())
    } else {
        Err(ÅtkomstFel::IngenÅtkomst)
    }
}

/// Ett användningsfall som skriver i ett företag: en transaktion där åtkomsten är kontrollerad.
/// Ändringarna och posterna i behandlingshistoriken sparas tillsammans i [`Ärende::slutför`],
/// eller inte alls.
pub(crate) struct Ärende {
    tx: Transaction<'static, Sqlite>,
    pub aktör: Aktör,
    pub företag: FöretagsId,
    /// Tidpunkten som ärendet gäller, från klockan.
    pub nu: Timestamp,
}

impl Ärende {
    /// Börjar en skrivande transaktion (`BEGIN IMMEDIATE`, så att SQLite serialiserar
    /// skrivningarna) och kontrollerar kanalpolicyn och åtkomsten.
    pub async fn börja(
        db: &Databas,
        klocka: &Klocka,
        aktör: &Aktör,
        företag: FöretagsId,
        handling: Handling,
    ) -> Result<Self, ÅtkomstFel> {
        let mut tx = db.pool().begin_with("BEGIN IMMEDIATE").await?;
        kräv(&mut tx, aktör, företag, handling).await?;
        Ok(Ärende {
            tx,
            aktör: *aktör,
            företag,
            nu: klocka.nu(),
        })
    }

    /// Ett ärende för ett företag som just skapades i `tx`, där åtkomsten redan är given.
    pub fn för_nytt_företag(
        tx: Transaction<'static, Sqlite>,
        aktör: &Aktör,
        företag: FöretagsId,
        nu: Timestamp,
    ) -> Self {
        Ärende {
            tx,
            aktör: *aktör,
            företag,
            nu,
        }
    }

    pub fn conn(&mut self) -> &mut SqliteConnection {
        &mut self.tx
    }

    /// Skriver en post i företagets behandlingshistorik, med ärendets aktör och tid.
    pub async fn logga(
        &mut self,
        händelse: Händelse,
        objekt: &str,
        beskrivning: serde_json::Value,
    ) -> Result<(), sqlx::Error> {
        behandlingshistorik::skriv(
            &mut self.tx,
            self.företag,
            self.nu,
            &self.aktör,
            händelse,
            objekt,
            beskrivning,
        )
        .await
    }

    pub async fn slutför(self) -> Result<(), sqlx::Error> {
        self.tx.commit().await
    }
}

// Delning: Inställningar → Åtkomst (ADR 0005). Åtkomst är en relation utan roller; den som
// har åtkomst kan bjuda in, ge och ta åtkomst men inte ta den sista. Att ändra kräver en färsk
// passkey, och en användare ser bara de användare hen delar ett företag med.

#[derive(Debug, thiserror::Error)]
pub enum DelningsFel {
    #[error("använd din passkey igen; det krävs för att bjuda in och ändra åtkomst")]
    KräverFärskPasskey,
    #[error(transparent)]
    OgiltigEpost(#[from] maya_domain::epost::OgiltigEpost),
    #[error("användaren har redan åtkomst till företaget")]
    HarRedanÅtkomst,
    /// Också för en användare som finns men som aktören inte delar något företag med.
    #[error("det finns ingen sådan användare")]
    OkändAnvändare,
    #[error("den sista åtkomsten till företaget går inte att ta bort")]
    SistaÅtkomsten,
    #[error(transparent)]
    Åtkomst(#[from] ÅtkomstFel),
}

impl From<sqlx::Error> for DelningsFel {
    fn from(e: sqlx::Error) -> Self {
        DelningsFel::Åtkomst(e.into())
    }
}

/// En användare i åtkomstlistan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Medlem {
    pub id: AnvändarId,
    pub epost: String,
    pub visningsnamn: String,
    pub avaktiverad: bool,
}

/// En inbjudan som inte är använd och inte har löpt ut.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ÖppenInbjudan {
    pub epost: String,
    pub skapad: Timestamp,
    pub giltig_till: Timestamp,
    /// Visningsnamnet på den som bjöd in.
    pub inbjuden_av: String,
}

/// Det som sidan Åtkomst visar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Åtkomstlista {
    /// Användarna med åtkomst till företaget, i e-postordning.
    pub användare: Vec<Medlem>,
    /// Öppna inbjudningar, äldst först.
    pub inbjudningar: Vec<ÖppenInbjudan>,
    /// Aktiva användare som aktören delar ett annat företag med och som saknar åtkomst hit:
    /// de som aktören kan ge åtkomst utan inbjudan.
    pub kandidater: Vec<Medlem>,
}

/// Vad en inbjudan blev.
#[derive(Debug, Clone)]
pub enum Inbjudan {
    /// En ny användare: länken visas för kopiering och skapar användaren med åtkomst.
    Länk(Registreringslänk),
    /// E-postadressen har redan ett konto, som har fått åtkomst direkt.
    ÅtkomstGiven,
}

type Medlemsrad = (i64, String, String, bool);

fn medlemmar(rader: Vec<Medlemsrad>) -> Vec<Medlem> {
    rader
        .into_iter()
        .map(|(id, epost, visningsnamn, avaktiverad)| Medlem {
            id: AnvändarId(id),
            epost,
            visningsnamn,
            avaktiverad,
        })
        .collect()
}

/// Användarna med åtkomst till företaget, öppna inbjudningar och vilka aktören kan ge åtkomst.
pub async fn åtkomst(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    företag: FöretagsId,
) -> Result<Åtkomstlista, ÅtkomstFel> {
    let mut tx = db.pool().begin().await?;
    kräv(&mut tx, aktör, företag, Handling::LäsaFöretag).await?;
    let användare = sqlx::query_as(
        "SELECT u.id, u.epost, u.visningsnamn, u.avaktiverad IS NOT NULL FROM atkomst a
         JOIN anvandare u ON u.id = a.anvandare_id
         WHERE a.foretag_id = ? ORDER BY u.epost",
    )
    .bind(företag.0)
    .fetch_all(&mut *tx)
    .await?;
    let kandidater = sqlx::query_as(
        "SELECT u.id, u.epost, u.visningsnamn, 0 FROM anvandare u
         WHERE u.avaktiverad IS NULL
           AND u.id IN (SELECT a.anvandare_id FROM atkomst a
                        JOIN atkomst egen ON egen.foretag_id = a.foretag_id
                        WHERE egen.anvandare_id = ?1)
           AND u.id NOT IN (SELECT anvandare_id FROM atkomst WHERE foretag_id = ?2)
         ORDER BY u.epost",
    )
    .bind(aktör.användare.map(|a| a.0))
    .bind(företag.0)
    .fetch_all(&mut *tx)
    .await?;
    let inbjudningar: Vec<(String, String, String, String)> = sqlx::query_as(
        "SELECT i.epost, i.skapad, i.giltig_till, u.visningsnamn FROM inbjudan i
         JOIN anvandare u ON u.id = i.skapad_av
         WHERE i.foretag_id = ? AND i.anvand IS NULL ORDER BY i.skapad, i.rowid",
    )
    .bind(företag.0)
    .fetch_all(&mut *tx)
    .await?;
    let nu = klocka.nu();
    let mut öppna = Vec::new();
    for (epost, skapad, giltig_till, inbjuden_av) in inbjudningar {
        let giltig_till = tidpunkt(&giltig_till)?;
        if nu < giltig_till {
            öppna.push(ÖppenInbjudan {
                epost,
                skapad: tidpunkt(&skapad)?,
                giltig_till,
                inbjuden_av,
            });
        }
    }
    Ok(Åtkomstlista {
        användare: medlemmar(användare),
        inbjudningar: öppna,
        kandidater: medlemmar(kandidater),
    })
}

/// Börjar ett ärende som ändrar åtkomsten: kräver en färsk passkey och åtkomst till företaget.
async fn ändra_åtkomst(
    db: &Databas,
    klocka: &Klocka,
    inloggad: &Inloggad,
    företag: FöretagsId,
) -> Result<Ärende, DelningsFel> {
    if !inloggad.har_färsk_passkey(klocka) {
        return Err(DelningsFel::KräverFärskPasskey);
    }
    Ok(Ärende::börja(
        db,
        klocka,
        &inloggad.aktör,
        företag,
        Handling::HanteraÅtkomst,
    )
    .await?)
}

/// Bjuder in `epost` till företaget. En ny e-postadress får en engångslänk som gäller i
/// [`LÄNK_GILTIG`]; en adress som redan har ett konto får bara åtkomst, så att samma person
/// inte får två konton.
pub async fn bjud_in(
    db: &Databas,
    klocka: &Klocka,
    inloggad: &Inloggad,
    företag: FöretagsId,
    epost: &str,
) -> Result<Inbjudan, DelningsFel> {
    let epost = maya_domain::epost::normalisera(epost)?;
    let mut ärende = ändra_åtkomst(db, klocka, inloggad, företag).await?;
    let befintlig: Option<i64> = sqlx::query_scalar("SELECT id FROM anvandare WHERE epost = ?")
        .bind(&epost)
        .fetch_optional(ärende.conn())
        .await?;
    if let Some(id) = befintlig {
        ge(&mut ärende, AnvändarId(id), &epost).await?;
        ärende.slutför().await?;
        return Ok(Inbjudan::ÅtkomstGiven);
    }
    let token = Token::ny();
    let giltig_till = ärende.nu + LÄNK_GILTIG;
    let mut handle = [0u8; 16];
    getrandom::fill(&mut handle).expect("slumptal från operativsystemet");
    let skapad = ärende.nu.to_string();
    sqlx::query(
        "INSERT INTO inbjudan (hash, foretag_id, epost, handle, skapad, skapad_av, giltig_till)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(hash(token.hemlighet()))
    .bind(företag.0)
    .bind(&epost)
    .bind(&handle[..])
    .bind(skapad)
    .bind(inloggad.användare.id.0)
    .bind(giltig_till.to_string())
    .execute(ärende.conn())
    .await?;
    ärende
        .logga(
            Händelse::InbjudanSkapad,
            &epost,
            serde_json::json!({ "epost": epost, "giltig_till": giltig_till.to_string() }),
        )
        .await?;
    ärende.slutför().await?;
    Ok(Inbjudan::Länk(Registreringslänk::ny(token, giltig_till)))
}

/// Ger `användare` åtkomst till företaget. Aktören måste redan dela ett företag med hen.
pub async fn ge_åtkomst(
    db: &Databas,
    klocka: &Klocka,
    inloggad: &Inloggad,
    företag: FöretagsId,
    användare: AnvändarId,
) -> Result<(), DelningsFel> {
    let mut ärende = ändra_åtkomst(db, klocka, inloggad, företag).await?;
    let epost: Option<String> = sqlx::query_scalar(
        "SELECT u.epost FROM anvandare u
         WHERE u.id = ?1 AND u.id IN (SELECT a.anvandare_id FROM atkomst a
                                      JOIN atkomst egen ON egen.foretag_id = a.foretag_id
                                      WHERE egen.anvandare_id = ?2)",
    )
    .bind(användare.0)
    .bind(inloggad.användare.id.0)
    .fetch_optional(ärende.conn())
    .await?;
    let epost = epost.ok_or(DelningsFel::OkändAnvändare)?;
    ge(&mut ärende, användare, &epost).await?;
    ärende.slutför().await?;
    Ok(())
}

async fn ge(
    ärende: &mut Ärende, användare: AnvändarId, epost: &str
) -> Result<(), DelningsFel> {
    let företag = ärende.företag;
    let ny = sqlx::query(
        "INSERT INTO atkomst (foretag_id, anvandare_id) VALUES (?, ?) ON CONFLICT DO NOTHING",
    )
    .bind(företag.0)
    .bind(användare.0)
    .execute(ärende.conn())
    .await?
    .rows_affected();
    if ny == 0 {
        return Err(DelningsFel::HarRedanÅtkomst);
    }
    ärende
        .logga(
            Händelse::ÅtkomstGiven,
            epost,
            serde_json::json!({ "användare": användare.0, "epost": epost }),
        )
        .await?;
    Ok(())
}

/// Tar `användare`s åtkomst till företaget, också aktörens egen, men aldrig den sista: minst
/// en aktiv användare med åtkomst ska finnas kvar.
pub async fn ta_åtkomst(
    db: &Databas,
    klocka: &Klocka,
    inloggad: &Inloggad,
    företag: FöretagsId,
    användare: AnvändarId,
) -> Result<(), DelningsFel> {
    let mut ärende = ändra_åtkomst(db, klocka, inloggad, företag).await?;
    let epost: Option<String> = sqlx::query_scalar(
        "SELECT u.epost FROM atkomst a JOIN anvandare u ON u.id = a.anvandare_id
         WHERE a.foretag_id = ? AND a.anvandare_id = ?",
    )
    .bind(företag.0)
    .bind(användare.0)
    .fetch_optional(ärende.conn())
    .await?;
    let epost = epost.ok_or(DelningsFel::OkändAnvändare)?;
    let andra_aktiva: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM atkomst a JOIN anvandare u ON u.id = a.anvandare_id
         WHERE a.foretag_id = ? AND a.anvandare_id <> ? AND u.avaktiverad IS NULL",
    )
    .bind(företag.0)
    .bind(användare.0)
    .fetch_one(ärende.conn())
    .await?;
    if andra_aktiva == 0 {
        return Err(DelningsFel::SistaÅtkomsten);
    }
    sqlx::query("DELETE FROM atkomst WHERE foretag_id = ? AND anvandare_id = ?")
        .bind(företag.0)
        .bind(användare.0)
        .execute(ärende.conn())
        .await?;
    ärende
        .logga(
            Händelse::ÅtkomstTagen,
            &epost,
            serde_json::json!({ "användare": användare.0, "epost": epost }),
        )
        .await?;
    ärende.slutför().await?;
    Ok(())
}

/// En giltig inbjudan: oanvänd, inte för gammal, och e-postadressen har inget konto än.
pub(crate) struct GiltigInbjudan {
    pub registrant: Registrant,
    företag: FöretagsId,
    hash: Vec<u8>,
}

pub(crate) async fn giltig_inbjudan(
    conn: &mut SqliteConnection,
    nu: Timestamp,
    token: &str,
) -> Result<GiltigInbjudan, InloggningsFel> {
    let hash = hash(token);
    let rad: Option<(i64, String, Vec<u8>, String)> = sqlx::query_as(
        "SELECT foretag_id, epost, handle, giltig_till FROM inbjudan
         WHERE hash = ? AND anvand IS NULL
           AND NOT EXISTS (SELECT 1 FROM anvandare u WHERE u.epost = inbjudan.epost)",
    )
    .bind(&hash)
    .fetch_optional(conn)
    .await?;
    let Some((företag, epost, handle, giltig_till)) = rad else {
        return Err(InloggningsFel::OgiltigLänk);
    };
    if nu >= tidpunkt(&giltig_till)? {
        return Err(InloggningsFel::OgiltigLänk);
    }
    let handle = handle
        .try_into()
        .map_err(|_| DatabasFel::Korrupt("inbjudan.handle".into()))?;
    Ok(GiltigInbjudan {
        registrant: Registrant {
            handle,
            visningsnamn: epost.clone(),
            epost,
        },
        företag: FöretagsId(företag),
        hash,
    })
}

/// Förbrukar inbjudan: skapar användaren med åtkomst till företaget och loggar det med den nya
/// användaren som aktör. Anroparen sparar passkeyn och sessionen i samma transaktion.
pub(crate) async fn använd_inbjudan(
    conn: &mut SqliteConnection,
    nu: Timestamp,
    token: &str,
) -> Result<AnvändarId, InloggningsFel> {
    let inbjudan = giltig_inbjudan(conn, nu, token).await?;
    let epost = inbjudan.registrant.epost;
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO anvandare (handle, epost, visningsnamn, skapad) VALUES (?, ?, ?, ?)
         RETURNING id",
    )
    .bind(&inbjudan.registrant.handle[..])
    .bind(&epost)
    .bind(&inbjudan.registrant.visningsnamn)
    .bind(nu.to_string())
    .fetch_one(&mut *conn)
    .await?;
    sqlx::query("UPDATE inbjudan SET anvand = ?, anvandare_id = ? WHERE hash = ?")
        .bind(nu.to_string())
        .bind(id)
        .bind(&inbjudan.hash)
        .execute(&mut *conn)
        .await?;
    sqlx::query("INSERT INTO atkomst (foretag_id, anvandare_id) VALUES (?, ?)")
        .bind(inbjudan.företag.0)
        .bind(id)
        .execute(&mut *conn)
        .await?;
    let aktör = Aktör::webb(AnvändarId(id));
    let beskrivning = serde_json::json!({ "användare": id, "epost": epost });
    for händelse in [Händelse::InbjudanAnvänd, Händelse::ÅtkomstGiven] {
        behandlingshistorik::skriv(
            conn,
            inbjudan.företag,
            nu,
            &aktör,
            händelse,
            &epost,
            beskrivning.clone(),
        )
        .await?;
    }
    Ok(AnvändarId(id))
}
