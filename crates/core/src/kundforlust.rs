//! Kundförlust och återkallelse (#141): skriva bort det som återstår av en utfärdad kundfaktura
//! som en konstaterad kundförlust, och ta tillbaka förlusten, med verifikatet i samma
//! transaktion som fakturahändelsen (ADR 0003, L5).
//!
//! Förlusten prövas med spärrarna i [`maya_domain::kundforlust`] mot det som återstår i
//! transaktionen och bokförs enligt tabellen över automatkonteringar: 6351 netto och utgående
//! moms per sats, fördelat proportionellt mot fakturans gällande utfärdandeverifikat (efter en
//! eventuell Rätta kontering, [`crate::kontering::gällande_i`]), mot 1510. *Återkalla
//! kundförlust* bokför ett nytt verifikat daterat i dag som vänder förlustens verifikat, och
//! förlusten räknas inte längre i reskontran, så att fakturan kan betalas igen. Förlusten och
//! återkallelsen ändras aldrig (L1). Status och det som återstår räknas fram och lagras inte.

use std::fmt;
use std::str::FromStr;

use jiff::civil::Date;
use maya_domain::kundforlust::{
    Förutsättningar, NyKundförlust, Skäl, Spärr, beskrivning, återkallelse,
    återkallelsens_beskrivning,
};
use serde_json::json;
use sqlx::SqliteConnection;

use crate::aktor::{Aktör, Handling};
use crate::atkomst::{self, Ärende, ÅtkomstFel};
use crate::behandlingshistorik::Händelse;
use crate::betalning::{self, Faktura};
use crate::foretag::{FöretagsId, datum};
use crate::kundfaktura::{KundfakturaFel, KundfakturaId};
use crate::verifikat::{
    self, Bokfört, Flöde, Länk, Signatur, VerifikatFel, VerifikatId, signatur
};
use crate::{Databas, DatabasFel, Klocka};

/// En kundförlusts id i installationen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct KundförlustId(pub(crate) i64);

impl fmt::Display for KundförlustId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for KundförlustId {
    type Err = std::num::ParseIntError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse().map(KundförlustId)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum KundförlustFel {
    /// Spärrarna i `domain`, med samma koder som panelen visar.
    #[error("kundförlusten kan inte bokföras")]
    Spärrat(Vec<Spärr>),
    /// Ingen utfärdad kundfaktura med det id:t i företaget.
    #[error("fakturan finns inte")]
    FinnsInte,
    /// Ingen kundförlust med det id:t i företaget.
    #[error("kundförlusten finns inte")]
    IngenKundförlust,
    /// Kundförlusten är redan återkallad.
    #[error("kundförlusten är redan återkallad")]
    RedanÅterkallad,
    /// Kundförlusten är daterad efter i dag; återkallelsen, som dateras i dag, kan inte
    /// komma före den.
    #[error("kundförlusten är daterad efter i dag och kan återkallas först den dagen")]
    FöreFörlusten,
    /// Förlustens verifikat kunde inte bokföras eller vändas, t.ex. en återkallelse i dag av
    /// en förlust daterad senare.
    #[error(transparent)]
    Verifikat(VerifikatFel),
    #[error(transparent)]
    Åtkomst(#[from] ÅtkomstFel),
}

impl From<sqlx::Error> for KundförlustFel {
    fn from(e: sqlx::Error) -> Self {
        KundförlustFel::Åtkomst(e.into())
    }
}

impl From<DatabasFel> for KundförlustFel {
    fn from(e: DatabasFel) -> Self {
        KundförlustFel::Åtkomst(e.into())
    }
}

impl From<VerifikatFel> for KundförlustFel {
    fn from(e: VerifikatFel) -> Self {
        match e {
            VerifikatFel::Åtkomst(e) => KundförlustFel::Åtkomst(e),
            e => KundförlustFel::Verifikat(e),
        }
    }
}

impl From<KundfakturaFel> for KundförlustFel {
    fn from(e: KundfakturaFel) -> Self {
        match e {
            KundfakturaFel::Åtkomst(e) => KundförlustFel::Åtkomst(e),
            KundfakturaFel::Verifikat(e) => e.into(),
            KundfakturaFel::FinnsInte => KundförlustFel::FinnsInte,
            e => KundförlustFel::Åtkomst(ÅtkomstFel::Databas(DatabasFel::Korrupt(e.to_string()))),
        }
    }
}

/// En bokförd kundförlust.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bokförd {
    pub id: KundförlustId,
    /// `None` när fordran inte är bokförd och förlusten bara är en status.
    pub verifikat: Option<Bokfört>,
}

/// En återkallad kundförlust.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Återkallad {
    /// Verifikatet som vänder förlustens, eller `None` när förlusten inte hade något.
    pub verifikat: Option<Bokfört>,
    /// Det som återstår av fakturan när förlusten inte längre räknas, i öre.
    pub återstår: i64,
}

/// Återkallelsen av en kundförlust.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Återkallelse {
    pub verifikat: Option<Länk>,
    pub återkallad: Signatur,
}

/// En kundförlust på fakturan, återkallad eller inte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Kundförlustpost {
    pub id: KundförlustId,
    pub datum: Date,
    pub skäl: Skäl,
    /// Skälet i text, t.ex. "Konkurs" eller "Annat: …".
    pub skälet: String,
    /// Det som skrevs bort, i öre.
    pub belopp: i64,
    pub verifikat: Option<Länk>,
    pub bokförd: Signatur,
    /// `Some` när förlusten är återkallad och inte längre räknas i reskontran.
    pub återkallad: Option<Återkallelse>,
}

/// Fakturan som förlusten gäller, läst i transaktionen.
struct Fakturan {
    nummer: String,
    kund: String,
    total: i64,
    fakturadatum: Date,
}

impl Fakturan {
    /// Fakturan som verifikatets referens: "kundfaktura 1007, Kund AB".
    fn referens(&self) -> String {
        format!("kundfaktura {}, {}", self.nummer, self.kund)
    }
}

async fn läs_fakturan(
    conn: &mut SqliteConnection,
    företag: FöretagsId,
    faktura: KundfakturaId,
) -> Result<Fakturan, KundförlustFel> {
    // #143: en extern faktura har det andra systemets nummer; en upplåst skrivs inte av.
    let rad: Option<(String, String, i64, String)> = sqlx::query_as(
        "SELECT coalesce(CAST(nummer AS TEXT), fakturanummer), kunduppgifter ->> '$.namn',
                total, fakturadatum
         FROM kundfaktura
         WHERE id = ? AND foretag_id = ? AND status IN ('utfardad', 'registrerad')",
    )
    .bind(faktura.0)
    .bind(företag.0)
    .fetch_optional(&mut *conn)
    .await?;
    let (nummer, kund, total, fakturadatum) = rad.ok_or(KundförlustFel::FinnsInte)?;
    Ok(Fakturan {
        nummer,
        kund,
        total,
        fakturadatum: datum(&fakturadatum)?,
    })
}

/// Det som de gällande kundförlusterna har skrivit bort fakturan, i öre: noll eller beloppet för
/// den enda förlust som inte är återkallad. Det som återstår räknas utan det.
pub(crate) async fn förlorat_i(
    conn: &mut SqliteConnection,
    faktura: KundfakturaId,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT coalesce(sum(belopp), 0) FROM kundforlust_i_reskontran WHERE kundfaktura_id = ?",
    )
    .bind(faktura.0)
    .fetch_one(conn)
    .await
}

/// Det som förlustpanelen prövas mot: det som återstår, fakturadatumet, om en förlust redan
/// gäller, det gällande utfärdandets rader och räkenskapsåren.
pub async fn förutsättningar(
    db: &Databas,
    aktör: &Aktör,
    företag: FöretagsId,
    faktura: KundfakturaId,
) -> Result<Förutsättningar, KundförlustFel> {
    let mut tx = db.pool().begin().await?;
    atkomst::kräv(&mut tx, aktör, företag, Handling::LäsaFöretag).await?;
    let fakturan = läs_fakturan(&mut tx, företag, faktura).await?;
    läs_förutsättningar(&mut tx, företag, faktura, &fakturan).await
}

async fn läs_förutsättningar(
    conn: &mut SqliteConnection,
    företag: FöretagsId,
    faktura: KundfakturaId,
    fakturan: &Fakturan,
) -> Result<Förutsättningar, KundförlustFel> {
    let återstår =
        betalning::återstår_i(&mut *conn, Faktura::Kund(faktura), fakturan.total).await?;
    let kundförlust = förlorat_i(&mut *conn, faktura).await? != 0;
    // En fordran som inte är bokförd (kontantmetoden, #149) ger bara status: inget verifikat.
    // #143: en extern fakturas fordran bokfördes av registreringen.
    let utfärdandet = if crate::fordran::bokförd_i(&mut *conn, Faktura::Kund(faktura)).await? {
        let bokföringen = match crate::extern_kundfaktura::registreringens_verifikat_i(
            &mut *conn, företag, faktura,
        )
        .await?
        {
            Some(v) => v,
            None => crate::kontering::gällande_i(&mut *conn, företag, faktura)
                .await?
                .verifikat
                .ok_or(VerifikatFel::IngetVerifikat)?,
        };
        Some(domänrader(&mut *conn, företag, bokföringen).await?)
    } else {
        None
    };
    let räkenskapsår = verifikat::läs_förutsättningar(&mut *conn, företag)
        .await?
        .räkenskapsår;
    Ok(Förutsättningar {
        återstår,
        fakturadatum: fakturan.fakturadatum,
        kundförlust,
        utfärdandet,
        räkenskapsår,
    })
}

/// Raderna i det bokförda verifikatet `id` som `domain` konterar med.
async fn domänrader(
    conn: &mut SqliteConnection,
    företag: FöretagsId,
    id: VerifikatId,
) -> Result<Vec<maya_domain::verifikat::Rad>, KundförlustFel> {
    let v = verifikat::läs_verifikat(conn, företag, id)
        .await?
        .ok_or(VerifikatFel::IngetVerifikat)?;
    v.rader
        .into_iter()
        .map(|r| {
            Ok(maya_domain::verifikat::Rad {
                konto: r.konto.ok_or_else(|| {
                    DatabasFel::Korrupt("ett bokfört verifikat utan konto".into())
                })?,
                debet: r.debet,
                kredit: r.kredit,
                text: r.text,
            })
        })
        .collect()
}

/// Bokför en konstaterad kundförlust på det som återstår av fakturan, med skäl och datum, och
/// dess verifikat daterat på förlustens datum med fakturan som referens, i samma transaktion:
/// 6351 netto och utgående moms proportionellt mot 1510.
pub async fn bokför(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    företag: FöretagsId,
    faktura: KundfakturaId,
    formulär: &NyKundförlust,
) -> Result<Bokförd, KundförlustFel> {
    let mut ärende =
        Ärende::börja(db, klocka, aktör, företag, Handling::BokföraKundförlust).await?;
    let fakturan = läs_fakturan(ärende.conn(), företag, faktura).await?;
    let f = läs_förutsättningar(ärende.conn(), företag, faktura, &fakturan).await?;
    let k = formulär.kontrollera(&f).map_err(KundförlustFel::Spärrat)?;
    let kontering = k.kontering();
    let bokfört = if kontering.is_empty() {
        None
    } else {
        Some(
            verifikat::bokför_i_ärende(
                &mut ärende,
                &verifikat::formulär_av(k.datum, &beskrivning(&fakturan.referens()), &kontering),
                &[],
            )
            .await?,
        )
    };
    let (nu, kanal, av) = (
        ärende.nu.to_string(),
        ärende.aktör.kanal.namn(),
        ärende.aktör.användare.map(|a| a.0),
    );
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO kundforlust
             (foretag_id, kundfaktura_id, datum, skal, annat, belopp, verifikat_id,
              bokford, bokford_kanal, bokford_av)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(företag.0)
    .bind(faktura.0)
    .bind(k.datum.to_string())
    .bind(k.skäl.kod())
    .bind(&k.annat)
    .bind(k.belopp)
    .bind(bokfört.map(|b| b.id.0))
    .bind(nu)
    .bind(kanal)
    .bind(av)
    .fetch_one(ärende.conn())
    .await?;
    let moms: i64 = kontering
        .iter()
        .filter(|r| r.konto.get() != maya_domain::kundforlust::KUNDFÖRLUSTER)
        .map(|r| r.debet.öre())
        .sum();
    ärende
        .logga(
            Händelse::KundförlustBokförd,
            &fakturan.nummer,
            json!({
                "kundförlust": id,
                "kundfaktura": faktura.0,
                "fakturanummer": fakturan.nummer,
                "motpart": fakturan.kund,
                "datum": k.datum.to_string(),
                "skäl": k.skäl.kod(),
                "skälet": k.skälet(),
                "summa_öre": k.belopp,
                "moms_öre": moms,
                "verifikat": bokfört.map(|b| json!({ "id": b.id.0, "nummer": b.nummer.to_string() })),
            }),
        )
        .await?;
    ärende.slutför().await?;
    Ok(Bokförd {
        id: KundförlustId(id),
        verifikat: bokfört,
    })
}

/// Återkallar kundförlusten: bokför ett nytt verifikat daterat i dag som vänder förlustens
/// verifikat, länkat till förlusten, så att fakturan kan betalas igen. Förlusten och dess
/// verifikat finns kvar och visar återkallelsen (L1). En förlust återkallas högst en gång.
pub async fn återkalla(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    företag: FöretagsId,
    kundförlust: KundförlustId,
) -> Result<Återkallad, KundförlustFel> {
    let mut ärende =
        Ärende::börja(db, klocka, aktör, företag, Handling::BokföraKundförlust).await?;
    let rad: Option<(i64, String, i64, Option<i64>, bool)> = sqlx::query_as(
        "SELECT kundfaktura_id, datum, belopp, verifikat_id,
                EXISTS (SELECT 1 FROM kundforlust_aterkallelse a WHERE a.kundforlust_id = f.id)
         FROM kundforlust f WHERE id = ? AND foretag_id = ?",
    )
    .bind(kundförlust.0)
    .bind(företag.0)
    .fetch_optional(ärende.conn())
    .await?;
    let (faktura, dag, belopp, förlustens, återkallad) =
        rad.ok_or(KundförlustFel::IngenKundförlust)?;
    if återkallad {
        return Err(KundförlustFel::RedanÅterkallad);
    }
    let idag = maya_domain::tid::datum(ärende.nu);
    if idag < datum(&dag)? {
        return Err(KundförlustFel::FöreFörlusten);
    }
    let faktura = KundfakturaId(faktura);
    let fakturan = läs_fakturan(ärende.conn(), företag, faktura).await?;
    let bokfört = match förlustens {
        None => None,
        Some(v) => {
            let rader = domänrader(ärende.conn(), företag, VerifikatId(v)).await?;
            Some(
                verifikat::bokför_i_ärende(
                    &mut ärende,
                    &verifikat::formulär_av(
                        idag,
                        &återkallelsens_beskrivning(&fakturan.referens()),
                        &återkallelse(&rader),
                    ),
                    &[],
                )
                .await?,
            )
        }
    };
    let (nu, kanal, av) = (
        ärende.nu.to_string(),
        ärende.aktör.kanal.namn(),
        ärende.aktör.användare.map(|a| a.0),
    );
    sqlx::query(
        "INSERT INTO kundforlust_aterkallelse
             (kundforlust_id, verifikat_id, aterkallad, aterkallad_kanal, aterkallad_av)
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(kundförlust.0)
    .bind(bokfört.map(|b| b.id.0))
    .bind(nu)
    .bind(kanal)
    .bind(av)
    .execute(ärende.conn())
    .await?;
    let återstår =
        betalning::återstår_i(ärende.conn(), Faktura::Kund(faktura), fakturan.total).await?;
    let förlustens_nummer = verifikat::länk(ärende.conn(), förlustens)
        .await?
        .and_then(|l| l.nummer)
        .map(|n| n.to_string());
    ärende
        .logga(
            Händelse::KundförlustÅterkallad,
            &fakturan.nummer,
            json!({
                "kundförlust": kundförlust.0,
                "kundfaktura": faktura.0,
                "fakturanummer": fakturan.nummer,
                "motpart": fakturan.kund,
                "summa_öre": belopp,
                "återstår_öre": återstår,
                "verifikat": förlustens.map(|id| json!({ "id": id, "nummer": förlustens_nummer })),
                "återkallelse": bokfört.map(|b| json!({ "id": b.id.0, "nummer": b.nummer.to_string() })),
            }),
        )
        .await?;
    ärende.slutför().await?;
    Ok(Återkallad {
        verifikat: bokfört,
        återstår,
    })
}

/// Fakturans kundförluster i ordning, också de återkallade.
pub async fn kundförluster(
    db: &Databas,
    aktör: &Aktör,
    företag: FöretagsId,
    faktura: KundfakturaId,
) -> Result<Vec<Kundförlustpost>, KundförlustFel> {
    let mut tx = db.pool().begin().await?;
    atkomst::kräv(&mut tx, aktör, företag, Handling::LäsaFöretag).await?;
    type Rad = (
        i64,
        String,
        String,
        String,
        i64,
        Option<i64>,
        String,
        String,
        Option<i64>,
        Option<String>,
        Option<i64>,
        Option<String>,
        Option<String>,
        Option<i64>,
        Option<String>,
    );
    let rader: Vec<Rad> = sqlx::query_as(
        "SELECT f.id, f.datum, f.skal, f.annat, f.belopp, f.verifikat_id,
                f.bokford, f.bokford_kanal, f.bokford_av, b.visningsnamn,
                a.verifikat_id, a.aterkallad, a.aterkallad_kanal, a.aterkallad_av, ab.visningsnamn
         FROM kundforlust f
         LEFT JOIN anvandare b ON b.id = f.bokford_av
         LEFT JOIN kundforlust_aterkallelse a ON a.kundforlust_id = f.id
         LEFT JOIN anvandare ab ON ab.id = a.aterkallad_av
         WHERE f.kundfaktura_id = ? AND f.foretag_id = ?
         ORDER BY f.id",
    )
    .bind(faktura.0)
    .bind(företag.0)
    .fetch_all(&mut *tx)
    .await?;
    let mut poster = Vec::new();
    for (
        id,
        dag,
        skäl,
        annat,
        belopp,
        verifikat_id,
        tid,
        kanal,
        av,
        namn,
        återkallelsen,
        återkallad,
        återkallad_kanal,
        återkallad_av,
        återkallad_namn,
    ) in rader
    {
        let skäl =
            Skäl::från_kod(&skäl).ok_or_else(|| DatabasFel::Korrupt("kundforlust.skal".into()))?;
        let återkallad = match (återkallad, återkallad_kanal) {
            (Some(tid), Some(kanal)) => Some(Återkallelse {
                verifikat: verifikat::länk(&mut tx, återkallelsen).await?,
                återkallad: signatur(&tid, &kanal, återkallad_av, återkallad_namn)?,
            }),
            _ => None,
        };
        poster.push(Kundförlustpost {
            id: KundförlustId(id),
            datum: datum(&dag)?,
            skäl,
            skälet: skäl.text(&annat),
            belopp,
            verifikat: verifikat::länk(&mut tx, verifikat_id).await?,
            bokförd: signatur(&tid, &kanal, av, namn)?,
            återkallad,
        });
    }
    Ok(poster)
}

/// Fakturan som en kundförlusts verifikat, eller återkallelsen av det, bokfördes från, för
/// länken från verifikatet och spärren mot en vanlig rättelse.
pub(crate) async fn fakturan_för_verifikat(
    conn: &mut SqliteConnection,
    verifikat: VerifikatId,
) -> Result<Option<(Flöde, i64, String)>, DatabasFel> {
    let rad: Option<(i64, String, String)> = sqlx::query_as(
        "SELECT k.id, coalesce(CAST(k.nummer AS TEXT), k.fakturanummer),
                k.kunduppgifter ->> '$.namn'
         FROM kundforlust f JOIN kundfaktura k ON k.id = f.kundfaktura_id
         WHERE f.verifikat_id = ?1
            OR f.id IN (SELECT kundforlust_id FROM kundforlust_aterkallelse WHERE verifikat_id = ?1)",
    )
    .bind(verifikat.0)
    .fetch_optional(conn)
    .await?;
    Ok(rad.map(|(id, nummer, kund)| {
        (
            Flöde::Kundfaktura,
            id,
            format!("kundförlusten på kundfaktura {nummer} till {kund}"),
        )
    }))
}
