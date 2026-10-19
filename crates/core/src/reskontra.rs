//! Reskontrorna som delsystem (#151, R4): reskontran per datum och avstämningen mot 1510 och
//! 2440.
//!
//! **Reskontran per datum** är de öppna posterna per faktura med det som återstod av dem vid
//! utgången av dagen, som bokföringen visar det ([`fordran::obetalt_vid_i`]): totalen minus
//! krediter, betalningar och kundförluster daterade senast den dagen och inte vända då.
//! Kundfakturor och leverantörsfakturor är var sin reskontra. En kreditfaktura som hänvisar till
//! sitt original räknas av mot det och står inte själv.
//!
//! **Avstämningen** jämför summan av det som återstår på fakturor vars fordran eller skuld är
//! bokförd med saldot på reskontrakontot per samma datum, och differensen är det som avviker.
//! Varje post visar sin del ([`Öppenpost::bokfört`]), efter fakturans [`Läge`] på dagen:
//!
//! - *Bokförd*: ett verifikat daterat senast dagen bokförde fordran eller skulden och var inte
//!   vänt då ([`fordran::bokför_i`]). Hela det som återstår står på kontot.
//! - *Inte bokförd* (kontantmetoden): betalningar och krediter bokförs mot intäkt eller kostnad
//!   och moms, utom den del av en kredit som överstiger det obetalda, som bokförs på kontot och
//!   ger ett negativt återstående belopp (ett tillgodohavande). Posten står på kontot med det
//!   som återstår när det är negativt, annars med noll.
//! - *Väntar på T2*: dagen är efter räkenskapsårets slut, fakturan var obetald vid årets slut
//!   och årets T2-verifikat är inte bokfört. Det som händer efter årets slut bokförs på kontot
//!   ([`fordran::bokförd_på_i`]), men det obetalda vid årets slut kommer dit först med T2. Posten
//!   står på kontot med det som återstår minus det som T2 ska bokföra.
//! - *Upplåst*: Rätta registrering eller Rätta avskrift har vänt verifikatet som bokförde
//!   fordran eller skulden, och betalningarna och krediterna finns kvar. Posten står på kontot
//!   med det som återstår minus totalen, alltså med det som betalningarna och krediterna
//!   bokförde, tills fakturan registreras om.
//!
//! Spärren mot att stänga ett år med en differens kommer i etapp 5, som använder samma rapport
//! i årsarkivet och saldospecifikationen.

use jiff::civil::Date;
use maya_domain::betalning::Reskontra;
use sqlx::SqliteConnection;

use crate::aktor::{Aktör, Handling};
use crate::atkomst::{self, ÅtkomstFel};
use crate::betalning::Faktura;
use crate::foretag::{FöretagsId, datum};
use crate::kundfaktura::KundfakturaId;
use crate::leverantorsfaktura::LeverantörsfakturaId;
use crate::verifikat::VerifikatId;
use crate::{Databas, DatabasFel, fordran, saldo};

/// Fakturans läge på dagen, som avgör hur mycket av det som återstår som står på
/// reskontrakontot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Läge {
    /// Fordran eller skulden är bokförd på kontot.
    Bokförd,
    /// Fordran eller skulden är inte bokförd (kontantmetoden).
    InteBokförd,
    /// Det obetalda vid årets slut väntar på årets T2-verifikat.
    VäntarPåT2,
    /// Verifikatet som bokförde fordran eller skulden är vänt, och fakturan ska registreras om.
    Upplåst,
}

impl Läge {
    /// Läget som det står i reskontran och CSV:n.
    pub fn namn(self) -> &'static str {
        match self {
            Läge::Bokförd => "Bokförd",
            Läge::InteBokförd => "Inte bokförd",
            Läge::VäntarPåT2 => "Väntar på T2",
            Läge::Upplåst => "Upplåst",
        }
    }
}

/// En öppen post: en faktura med något kvar eller något på reskontrakontot på dagen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Öppenpost {
    pub faktura: Faktura,
    /// Mayas nummer i fakturaserien, eller det andra systemets eller leverantörens.
    pub fakturanummer: String,
    pub motpart: String,
    pub fakturadatum: Date,
    pub förfallodag: Option<Date>,
    /// I öre; negativt för en leverantörs kreditfaktura utan hänvisning.
    pub total: i64,
    /// Det som återstod vid dagens utgång, i öre. Negativt är ett tillgodohavande.
    pub återstår: i64,
    /// Det av posten som står på reskontrakontot, i öre: fordran på 1510 eller skulden på 2440.
    pub bokfört: i64,
    pub läge: Läge,
}

/// En reskontra per dagen med avstämningen mot sitt konto. Beloppen är i öre, en fordran
/// respektive en skuld positiv.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reskontradel {
    pub reskontra: Reskontra,
    /// I fakturadatumets ordning.
    pub poster: Vec<Öppenpost>,
    /// Summan av det som återstår.
    pub återstår: i64,
    /// Summan av postendas del på kontot: reskontrans sida i avstämningen.
    pub bokfört: i64,
    /// Kontots saldo vid dagens utgång ur de bokförda verifikaten, med 2440 i kredit positivt.
    pub saldo: i64,
    /// `bokfört - saldo`: noll när reskontran och huvudboken stämmer.
    pub differens: i64,
}

/// Kund- och leverantörsreskontran vid utgången av `dag`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reskontran {
    pub dag: Date,
    pub kund: Reskontradel,
    pub leverantör: Reskontradel,
}

/// Kund- och leverantörsreskontran vid utgången av `dag`, med avstämningen mot 1510 och 2440.
pub async fn reskontran(
    db: &Databas,
    aktör: &Aktör,
    företag: FöretagsId,
    dag: Date,
) -> Result<Reskontran, ÅtkomstFel> {
    let mut tx = db.pool().begin().await?;
    atkomst::kräv(&mut tx, aktör, företag, Handling::LäsaFöretag).await?;
    Ok(reskontran_i(&mut tx, företag, dag).await?)
}

pub(crate) async fn reskontran_i(
    conn: &mut SqliteConnection,
    företag: FöretagsId,
    dag: Date,
) -> Result<Reskontran, DatabasFel> {
    let saldobalanser = saldo::saldobalanser_till(&mut *conn, företag, Some(dag)).await?;
    // Det senaste året som har börjat: dess utgående balans är saldot vid dagens utgång.
    let senaste = saldobalanser.iter().rev().find(|sb| sb.år.start() <= dag);
    let saldot = |r: Reskontra| {
        let öre = senaste
            .and_then(|sb| sb.konton.iter().find(|k| k.konto == r.konto()))
            .map_or(0, |k| k.utgående.öre());
        match r {
            Reskontra::Kund => öre,
            Reskontra::Leverantör => -öre,
        }
    };
    let mut delar = Vec::new();
    for r in [Reskontra::Kund, Reskontra::Leverantör] {
        let mut poster = Vec::new();
        for f in fakturorna_i(&mut *conn, företag, r, dag, None).await? {
            if let Some(post) = posten_i(&mut *conn, företag, f, dag).await? {
                poster.push(post);
            }
        }
        let återstår = poster.iter().map(|p| p.återstår).sum();
        let bokfört = poster.iter().map(|p| p.bokfört).sum();
        let saldo = saldot(r);
        delar.push(Reskontradel {
            reskontra: r,
            poster,
            återstår,
            bokfört,
            saldo,
            differens: bokfört - saldo,
        });
    }
    let leverantör = delar.pop().expect("två reskontror");
    let kund = delar.pop().expect("två reskontror");
    Ok(Reskontran {
        dag,
        kund,
        leverantör,
    })
}

/// Det av fakturan som står på reskontrakontot vid utgången av `dag` enligt reskontran
/// ([`Öppenpost::bokfört`]), i öre. Ett flöde som inte bokför mot kontot får inte ändra det
/// (#151).
pub(crate) async fn på_kontot_i(
    conn: &mut SqliteConnection,
    företag: FöretagsId,
    faktura: Faktura,
    dag: Date,
) -> Result<i64, DatabasFel> {
    let (reskontra, id) = match faktura {
        Faktura::Kund(id) => (Reskontra::Kund, id.0),
        Faktura::Leverantör(id) => (Reskontra::Leverantör, id.0),
    };
    let mut bokfört = 0;
    for f in fakturorna_i(&mut *conn, företag, reskontra, dag, Some(id)).await? {
        if let Some(post) = posten_i(&mut *conn, företag, f, dag).await? {
            bokfört += post.bokfört;
        }
    }
    Ok(bokfört)
}

/// Om det som reskontran säger står på fakturans reskontrakonto har ändrats från `före`
/// ([`på_kontot_i`]) med precis det som `verifikat` bokförde där. En rättelse eller
/// återkallelse som ändrar mer skulle få kontot att avvika från reskontran: något har bokförts
/// utifrån det som rättas, och den spärras (#151).
pub(crate) async fn följer_kontot_i(
    conn: &mut SqliteConnection,
    företag: FöretagsId,
    faktura: Faktura,
    före: i64,
    verifikat: Option<VerifikatId>,
) -> Result<bool, DatabasFel> {
    let (reskontra, tecken) = match faktura {
        Faktura::Kund(_) => (Reskontra::Kund, 1),
        Faktura::Leverantör(_) => (Reskontra::Leverantör, -1),
    };
    let bokfört: i64 = match verifikat {
        None => 0,
        Some(v) => {
            sqlx::query_scalar(
                "SELECT coalesce(sum(debet - kredit), 0) FROM verifikatrad
             WHERE verifikat_id = ? AND konto = ?",
            )
            .bind(v.0)
            .bind(i64::from(reskontra.konto().get()))
            .fetch_one(&mut *conn)
            .await?
        }
    };
    let efter = på_kontot_i(conn, företag, faktura, Date::MAX).await?;
    Ok(efter - före == tecken * bokfört)
}

/// En faktura i reskontran som kan ha något kvar på dagen.
struct Fakturan {
    faktura: Faktura,
    fakturanummer: String,
    motpart: String,
    fakturadatum: Date,
    förfallodag: Option<Date>,
    total: i64,
    upplåst: bool,
}

/// Reskontrans fakturor med fakturadatum senast `dag`: utfärdade, registrerade och upplåsta,
/// utom kreditfakturor som hänvisar till ett original. I fakturadatumets ordning.
async fn fakturorna_i(
    conn: &mut SqliteConnection,
    företag: FöretagsId,
    reskontra: Reskontra,
    dag: Date,
    bara: Option<i64>,
) -> Result<Vec<Fakturan>, DatabasFel> {
    type Rad = (
        i64,
        String,
        Option<String>,
        String,
        Option<String>,
        i64,
        bool,
    );
    let sql = match reskontra {
        Reskontra::Kund => {
            "SELECT id, coalesce(CAST(nummer AS TEXT), fakturanummer), kunduppgifter ->> '$.namn',
                    fakturadatum, forfallodag, total, status = 'upplast'
             FROM kundfaktura
             WHERE foretag_id = ?1 AND status IN ('utfardad', 'registrerad', 'upplast')
               AND krediterar IS NULL AND fakturadatum <= ?2 AND total IS NOT NULL
               AND (?3 IS NULL OR id = ?3)
             ORDER BY fakturadatum, id"
        }
        Reskontra::Leverantör => {
            "SELECT f.id, f.fakturanummer, l.namn, f.fakturadatum, f.forfallodag, f.total,
                    f.status = 'upplast'
             FROM leverantorsfaktura f LEFT JOIN leverantor l ON l.id = f.leverantor_id
             WHERE f.foretag_id = ?1 AND f.status IN ('registrerad', 'upplast')
               AND f.krediterar IS NULL AND f.fakturadatum <= ?2 AND f.total IS NOT NULL
               AND (?3 IS NULL OR f.id = ?3)
             ORDER BY f.fakturadatum, f.id"
        }
    };
    let rader: Vec<Rad> = sqlx::query_as(sql)
        .bind(företag.0)
        .bind(dag.to_string())
        .bind(bara)
        .fetch_all(&mut *conn)
        .await?;
    rader
        .into_iter()
        .map(
            |(id, fakturanummer, motpart, fakturadatum, förfallodag, total, upplåst)| {
                Ok(Fakturan {
                    faktura: match reskontra {
                        Reskontra::Kund => Faktura::Kund(KundfakturaId(id)),
                        Reskontra::Leverantör => Faktura::Leverantör(LeverantörsfakturaId(id)),
                    },
                    fakturanummer,
                    motpart: motpart.unwrap_or_default(),
                    fakturadatum: datum(&fakturadatum)?,
                    förfallodag: förfallodag.as_deref().map(datum).transpose()?,
                    total,
                    upplåst,
                })
            },
        )
        .collect()
}

/// Fakturan som öppen post på dagen, eller `None` när inget återstår och inget står på kontot.
async fn posten_i(
    conn: &mut SqliteConnection,
    företag: FöretagsId,
    f: Fakturan,
    dag: Date,
) -> Result<Option<Öppenpost>, DatabasFel> {
    let återstår = fordran::obetalt_vid_i(&mut *conn, f.faktura, dag).await?;
    let (läge, bokfört) = if fordran::bokförd_vid_i(&mut *conn, f.faktura, dag).await? {
        (Läge::Bokförd, återstår)
    } else if f.upplåst && fordran::var_bokförd_vid_i(&mut *conn, f.faktura, dag).await? {
        (Läge::Upplåst, återstår - f.total)
    } else if let Some(obetalt) =
        fordran::väntar_på_t2_vid_i(&mut *conn, företag, f.faktura, f.fakturadatum, dag).await?
    {
        (Läge::VäntarPåT2, återstår - obetalt)
    } else {
        (Läge::InteBokförd, återstår.min(0))
    };
    if återstår == 0 && bokfört == 0 {
        return Ok(None);
    }
    let läge = match läge {
        Läge::VäntarPåT2 | Läge::InteBokförd if f.upplåst => Läge::Upplåst,
        läge => läge,
    };
    Ok(Some(Öppenpost {
        faktura: f.faktura,
        fakturanummer: f.fakturanummer,
        motpart: f.motpart,
        fakturadatum: f.fakturadatum,
        förfallodag: f.förfallodag,
        total: f.total,
        återstår,
        bokfört,
        läge,
    }))
}
