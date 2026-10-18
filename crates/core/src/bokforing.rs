//! Rapporter → Bokföring: grundboken och huvudboken (L3, L4).
//!
//! **Grundboken** är alla bokförda verifikat i registreringsordning, alltså i den ordning de
//! bokfördes (bokförd-tidpunkten), per period efter verifikatdatum. **Huvudboken** är samma
//! rader per konto, också i registreringsordning, med ingående balans, löpande saldo och
//! utgående balans. Båda räknas bara ur bokförda verifikat; utkast syns aldrig (L2).
//!
//! Urvalet är ett räkenskapsår och en följd av dess perioder. Huvudbokens ingående balans för
//! en period mitt i året är årets ingående balans (inskriven eller preliminär, se
//! [`crate::saldo`]) plus årets transaktioner före den första perioden i urvalet.

use std::collections::BTreeMap;

use jiff::Timestamp;
use jiff::civil::Date;
use maya_domain::belopp::{Belopp, Saldo};
use maya_domain::csv::Csv;
use maya_domain::kontoplan::Kontonummer;
use maya_domain::kontourval::Kontourval;
use maya_domain::rakenskapsar::{Period, Räkenskapsår};
use maya_pdf::Dokument;
use sqlx::SqliteConnection;

use crate::aktor::{Aktör, Handling, Kanal};
use crate::atkomst::{self, ÅtkomstFel};
use crate::foretag::{self, FöretagsId, datum};
use crate::saldo::{self, Status};
use crate::utskrift::{self, Pdf, UtskriftFel};
use crate::verifikat::{VerifikatId, Verifikationsnummer, kanalens_namn, verifikationsnummer};
use crate::{Databas, DatabasFel, Klocka};

/// Vilka perioder en grundbok eller huvudbok gäller: ett räkenskapsår och perioderna `från`
/// t.o.m. `till` i det. Ett datum anger perioden det ligger i; `None` är årets första
/// respektive sista period.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Periodurval {
    /// Räkenskapsårets startdatum.
    pub år: Date,
    pub från: Option<Date>,
    pub till: Option<Date>,
}

#[derive(Debug, thiserror::Error)]
pub enum RapportFel {
    #[error("företaget har inget räkenskapsår som börjar det datumet")]
    OkäntRäkenskapsår,
    /// Perioderna ligger inte i året, eller `från` är efter `till`.
    #[error("perioderna finns inte i räkenskapsåret")]
    OkändPeriod,
    #[error(transparent)]
    Åtkomst(#[from] ÅtkomstFel),
    #[error(transparent)]
    Pdf(#[from] maya_pdf::PdfFel),
}

impl From<UtskriftFel> for RapportFel {
    fn from(e: UtskriftFel) -> Self {
        match e {
            UtskriftFel::Åtkomst(e) => RapportFel::Åtkomst(e),
            UtskriftFel::Pdf(e) => RapportFel::Pdf(e),
            UtskriftFel::OkäntRäkenskapsår => RapportFel::OkäntRäkenskapsår,
        }
    }
}

impl From<sqlx::Error> for RapportFel {
    fn from(e: sqlx::Error) -> Self {
        RapportFel::Åtkomst(e.into())
    }
}

impl From<DatabasFel> for RapportFel {
    fn from(e: DatabasFel) -> Self {
        RapportFel::Åtkomst(e.into())
    }
}

/// Vem som bokförde och när: registreringstidpunkten (H1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bokföring {
    pub tid: Timestamp,
    /// Användarens visningsnamn.
    pub namn: String,
}

/// En rad i ett bokfört verifikat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bokföringsrad {
    pub konto: Kontonummer,
    pub kontonamn: String,
    pub text: Option<String>,
    pub debet: Belopp,
    pub kredit: Belopp,
}

/// Ett bokfört verifikat i grundboken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grundbokspost {
    pub id: VerifikatId,
    pub nummer: Verifikationsnummer,
    pub datum: Date,
    pub beskrivning: String,
    pub bokförd: Bokföring,
    pub rader: Vec<Bokföringsrad>,
    /// Summan av debet.
    pub summa: Belopp,
}

/// En period i grundboken med dess verifikat i registreringsordning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grundboksperiod {
    pub period: Period,
    pub verifikat: Vec<Grundbokspost>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grundbok {
    pub år: Räkenskapsår,
    /// Urvalets perioder i ordning, också de utan verifikat.
    pub perioder: Vec<Grundboksperiod>,
}

/// Grundboken: de bokförda verifikaten i urvalets perioder, per period och i
/// registreringsordning inom perioden.
pub async fn grundbok(
    db: &Databas,
    aktör: &Aktör,
    företag: FöretagsId,
    urval: &Periodurval,
) -> Result<Grundbok, RapportFel> {
    let mut tx = db.pool().begin().await?;
    atkomst::kräv(&mut tx, aktör, företag, Handling::LäsaFöretag).await?;
    let (år_id, år, perioder) = välj(&mut tx, företag, urval).await?;
    let poster = bokförda(&mut tx, företag, år_id).await?;
    let perioder = perioder
        .into_iter()
        .map(|period| Grundboksperiod {
            period,
            verifikat: poster
                .iter()
                .filter(|p| period.start <= p.datum && p.datum <= period.slut)
                .cloned()
                .collect(),
        })
        .collect();
    Ok(Grundbok { år, perioder })
}

/// En transaktion på ett konto i huvudboken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transaktion {
    pub id: VerifikatId,
    pub nummer: Verifikationsnummer,
    pub datum: Date,
    pub bokförd: Bokföring,
    /// Radens text, eller verifikatets beskrivning om raden saknar text.
    pub text: String,
    pub debet: Belopp,
    pub kredit: Belopp,
    /// Kontots saldo efter transaktionen.
    pub saldo: Saldo,
}

/// Ett konto i huvudboken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Huvudbokskonto {
    pub konto: Kontonummer,
    pub namn: String,
    /// Saldot när urvalets första period börjar.
    pub ingående: Saldo,
    /// Transaktionerna i urvalets perioder, i registreringsordning.
    pub transaktioner: Vec<Transaktion>,
    /// Saldot när urvalets sista period slutar.
    pub utgående: Saldo,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Huvudbok {
    pub år: Räkenskapsår,
    pub perioder: Vec<Period>,
    /// Årets ingående balans status: en preliminär ska visas som preliminär.
    pub status: Status,
    /// Konton i urvalet med ett ingående saldo eller transaktioner, i kontoordning.
    pub konton: Vec<Huvudbokskonto>,
}

/// Huvudboken för kontona i `konton` och urvalets perioder: per konto ingående balans,
/// transaktionerna i registreringsordning med löpande saldo, och utgående balans.
pub async fn huvudbok(
    db: &Databas,
    aktör: &Aktör,
    företag: FöretagsId,
    urval: &Periodurval,
    konton: &Kontourval,
) -> Result<Huvudbok, RapportFel> {
    let mut tx = db.pool().begin().await?;
    atkomst::kräv(&mut tx, aktör, företag, Handling::LäsaFöretag).await?;
    let (år_id, år, perioder) = välj(&mut tx, företag, urval).await?;
    let poster = bokförda(&mut tx, företag, år_id).await?;
    let saldobalans = saldo::saldobalanser(&mut tx, företag)
        .await?
        .into_iter()
        .find(|sb| sb.år.start() == år.start())
        .ok_or(RapportFel::OkäntRäkenskapsår)?;
    let namn: BTreeMap<i64, String> =
        sqlx::query_as("SELECT nummer, namn FROM konto WHERE foretag_id = ?")
            .bind(företag.0)
            .fetch_all(&mut *tx)
            .await?
            .into_iter()
            .collect();
    let (första, sista) = (perioder[0].start, perioder[perioder.len() - 1].slut);

    let mut böcker: BTreeMap<Kontonummer, Huvudbokskonto> = BTreeMap::new();
    fn bok<'a>(
        böcker: &'a mut BTreeMap<Kontonummer, Huvudbokskonto>,
        namn: &BTreeMap<i64, String>,
        konto: Kontonummer,
    ) -> &'a mut Huvudbokskonto {
        böcker.entry(konto).or_insert_with(|| Huvudbokskonto {
            konto,
            namn: namn
                .get(&i64::from(konto.get()))
                .cloned()
                .unwrap_or_default(),
            ingående: Saldo::NOLL,
            transaktioner: Vec::new(),
            utgående: Saldo::NOLL,
        })
    }
    for k in saldobalans
        .konton
        .iter()
        .filter(|k| konton.omfattar(k.konto))
    {
        bok(&mut böcker, &namn, k.konto).ingående = k.ingående;
    }
    for post in poster.iter().filter(|p| p.datum <= sista) {
        for rad in post.rader.iter().filter(|r| konton.omfattar(r.konto)) {
            let b = bok(&mut böcker, &namn, rad.konto);
            let förändring = Saldo::av(rad.debet, rad.kredit);
            if post.datum < första {
                b.ingående += förändring;
                continue;
            }
            let saldo = b.transaktioner.last().map_or(b.ingående, |t| t.saldo) + förändring;
            b.transaktioner.push(Transaktion {
                id: post.id,
                nummer: post.nummer,
                datum: post.datum,
                bokförd: post.bokförd.clone(),
                text: rad.text.clone().unwrap_or_else(|| post.beskrivning.clone()),
                debet: rad.debet,
                kredit: rad.kredit,
                saldo,
            });
        }
    }
    let konton = böcker
        .into_values()
        .filter_map(|mut k| {
            k.utgående = k.transaktioner.last().map_or(k.ingående, |t| t.saldo);
            (k.ingående != Saldo::NOLL || !k.transaktioner.is_empty()).then_some(k)
        })
        .collect();
    Ok(Huvudbok {
        år,
        perioder,
        status: saldobalans.status,
        konton,
    })
}

/// Räkenskapsårets id, året och urvalets perioder.
async fn välj(
    conn: &mut SqliteConnection,
    företag: FöretagsId,
    urval: &Periodurval,
) -> Result<(i64, Räkenskapsår, Vec<Period>), RapportFel> {
    let rad: Option<(i64, String)> =
        sqlx::query_as("SELECT id, slut FROM rakenskapsar WHERE foretag_id = ? AND start = ?")
            .bind(företag.0)
            .bind(urval.år.to_string())
            .fetch_optional(&mut *conn)
            .await?;
    let (id, slut) = rad.ok_or(RapportFel::OkäntRäkenskapsår)?;
    let år = Räkenskapsår::befintligt(urval.år, datum(&slut)?);
    let alla = år.perioder();
    let index = |d: Option<Date>, standard: usize| match d {
        None => Some(standard),
        Some(d) => alla.iter().position(|p| p.start <= d && d <= p.slut),
    };
    let (Some(från), Some(till)) = (index(urval.från, 0), index(urval.till, alla.len() - 1))
    else {
        return Err(RapportFel::OkändPeriod);
    };
    if från > till {
        return Err(RapportFel::OkändPeriod);
    }
    Ok((id, år, alla[från..=till].to_vec()))
}

/// Årets bokförda verifikat med rader, i registreringsordning: bokförd-tidpunkten och sedan
/// numret. Tidpunkten jämförs som tid och inte som text, eftersom texten kan ha olika många
/// decimaler.
async fn bokförda(
    conn: &mut SqliteConnection,
    företag: FöretagsId,
    år_id: i64,
) -> Result<Vec<Grundbokspost>, DatabasFel> {
    type Rad = (
        i64,
        i64,
        String,
        String,
        String,
        String,
        Option<String>,
        i64,
        String,
        i64,
        i64,
        Option<String>,
    );
    let rader: Vec<Rad> = sqlx::query_as(
        "SELECT v.id, v.nummer, v.datum, v.beskrivning, v.bokford, v.bokford_kanal, a.visningsnamn,
                r.konto, coalesce(k.namn, ''), r.debet, r.kredit, r.text
         FROM verifikat v
         JOIN verifikatrad r ON r.verifikat_id = v.id
         LEFT JOIN anvandare a ON a.id = v.bokford_av
         LEFT JOIN konto k ON k.foretag_id = v.foretag_id AND k.nummer = r.konto
         WHERE v.foretag_id = ? AND v.rakenskapsar_id = ? AND v.bokford IS NOT NULL
         ORDER BY v.id, r.radnr",
    )
    .bind(företag.0)
    .bind(år_id)
    .fetch_all(conn)
    .await?;
    let korrupt = |vad: &str| DatabasFel::Korrupt(format!("verifikat.{vad}"));
    let mut poster: BTreeMap<i64, Grundbokspost> = BTreeMap::new();
    for (
        id,
        nummer,
        datum_,
        beskrivning,
        bokförd,
        kanal,
        namn,
        konto,
        kontonamn,
        debet,
        kredit,
        text,
    ) in rader
    {
        let post = match poster.entry(id) {
            std::collections::btree_map::Entry::Occupied(e) => e.into_mut(),
            std::collections::btree_map::Entry::Vacant(e) => e.insert(Grundbokspost {
                id: VerifikatId(id),
                nummer: verifikationsnummer(nummer)?,
                datum: datum(&datum_)?,
                beskrivning,
                bokförd: Bokföring {
                    tid: bokförd.parse().map_err(|_| korrupt("bokford"))?,
                    namn: match namn {
                        Some(namn) => namn,
                        None => kanalens_namn(
                            Kanal::från_namn(&kanal).ok_or_else(|| korrupt("bokford_kanal"))?,
                        ),
                    },
                },
                rader: Vec::new(),
                summa: Belopp::NOLL,
            }),
        };
        post.summa = Belopp::från_öre(post.summa.öre() + debet);
        post.rader.push(Bokföringsrad {
            konto: kontonummer(konto)?,
            kontonamn,
            text,
            debet: Belopp::från_öre(debet),
            kredit: Belopp::från_öre(kredit),
        });
    }
    let mut poster: Vec<_> = poster.into_values().collect();
    poster.sort_by_key(|p| (p.bokförd.tid, p.nummer));
    Ok(poster)
}

/// En exporterad fil med namnet att spara den under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fil {
    pub filnamn: String,
    pub innehåll: Vec<u8>,
}

/// Grundboken som CSV: en rad per verifikatrad, i grundbokens ordning.
pub async fn grundbok_csv(
    db: &Databas,
    aktör: &Aktör,
    företag: FöretagsId,
    urval: &Periodurval,
) -> Result<Fil, RapportFel> {
    let g = grundbok(db, aktör, företag, urval).await?;
    let mut csv = Csv::new(&[
        "Period",
        "Bokförd",
        "Bokförd av",
        "Verifikat",
        "Verifikatdatum",
        "Beskrivning",
        "Konto",
        "Kontonamn",
        "Text",
        "Debet",
        "Kredit",
    ]);
    for p in &g.perioder {
        let period = p.period.start.strftime("%Y-%m").to_string();
        for v in &p.verifikat {
            for r in &v.rader {
                csv.rad([
                    period.clone(),
                    tid(v.bokförd.tid),
                    v.bokförd.namn.clone(),
                    v.nummer.to_string(),
                    v.datum.to_string(),
                    v.beskrivning.clone(),
                    r.konto.to_string(),
                    r.kontonamn.clone(),
                    r.text.clone().unwrap_or_default(),
                    belopp(r.debet),
                    belopp(r.kredit),
                ]);
            }
        }
    }
    Ok(Fil {
        filnamn: filnamn(
            db,
            företag,
            "grundbok",
            g.år,
            &g.perioder.iter().map(|p| p.period).collect::<Vec<_>>(),
            "csv",
        )
        .await?,
        innehåll: csv.innehåll(),
    })
}

/// Huvudboken som CSV: per konto en rad med ingående balans, en rad per verifikatrad med
/// löpande saldo och en rad med utgående balans.
pub async fn huvudbok_csv(
    db: &Databas,
    aktör: &Aktör,
    företag: FöretagsId,
    urval: &Periodurval,
    konton: &Kontourval,
) -> Result<Fil, RapportFel> {
    let h = huvudbok(db, aktör, företag, urval, konton).await?;
    let mut csv = Csv::new(&[
        "Konto",
        "Kontonamn",
        "Bokförd",
        "Verifikat",
        "Verifikatdatum",
        "Text",
        "Debet",
        "Kredit",
        "Saldo",
    ]);
    let ingående = match h.status {
        Status::Inskriven => "Ingående balans",
        Status::Preliminär => "Ingående balans (preliminär)",
    };
    for k in &h.konton {
        let balans = |text: &str, saldo: Saldo| {
            [
                k.konto.to_string(),
                k.namn.clone(),
                String::new(),
                String::new(),
                String::new(),
                text.to_string(),
                String::new(),
                String::new(),
                saldo_text(saldo),
            ]
        };
        csv.rad(balans(ingående, k.ingående));
        for t in &k.transaktioner {
            csv.rad([
                k.konto.to_string(),
                k.namn.clone(),
                tid(t.bokförd.tid),
                t.nummer.to_string(),
                t.datum.to_string(),
                t.text.clone(),
                belopp(t.debet),
                belopp(t.kredit),
                saldo_text(t.saldo),
            ]);
        }
        csv.rad(balans("Utgående balans", k.utgående));
    }
    Ok(Fil {
        filnamn: filnamn(db, företag, "huvudbok", h.år, &h.perioder, "csv").await?,
        innehåll: csv.innehåll(),
    })
}

/// Grundboken som PDF/A-2b (A2). Skriv ut och exporten är samma PDF.
pub async fn grundbok_pdf(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    företag: FöretagsId,
    urval: &Periodurval,
) -> Result<Pdf, RapportFel> {
    let g = grundbok(db, aktör, företag, urval).await?;
    let perioder: Vec<Period> = g.perioder.iter().map(|p| p.period).collect();
    let dokument = Dokument::Grundbok(maya_pdf::Grundbok {
        företag: pdf_företag(db, aktör, företag).await?,
        urval: pdf_urval(g.år, &perioder),
        perioder: g
            .perioder
            .iter()
            .map(|p| maya_pdf::Grundboksperiod {
                start: p.period.start,
                verifikat: p
                    .verifikat
                    .iter()
                    .map(|v| maya_pdf::Grundbokspost {
                        nummer: v.nummer.to_string(),
                        datum: v.datum,
                        beskrivning: v.beskrivning.clone(),
                        bokförd: v.bokförd.tid,
                        bokförd_av: v.bokförd.namn.clone(),
                        rader: v
                            .rader
                            .iter()
                            .map(|r| maya_pdf::Rad {
                                konto: Some(r.konto),
                                kontonamn: r.kontonamn.clone(),
                                text: r.text.clone(),
                                debet: r.debet,
                                kredit: r.kredit,
                            })
                            .collect(),
                    })
                    .collect(),
            })
            .collect(),
    });
    let filnamn = filnamn(db, företag, "grundbok", g.år, &perioder, "pdf").await?;
    Ok(utskrift::skriv(db, klocka, aktör, filnamn, &dokument).await?)
}

/// Huvudboken som PDF/A-2b (A2). Skriv ut och exporten är samma PDF.
pub async fn huvudbok_pdf(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    företag: FöretagsId,
    urval: &Periodurval,
    konton: &Kontourval,
) -> Result<Pdf, RapportFel> {
    let h = huvudbok(db, aktör, företag, urval, konton).await?;
    let dokument = Dokument::Huvudbok(maya_pdf::Huvudbok {
        företag: pdf_företag(db, aktör, företag).await?,
        urval: pdf_urval(h.år, &h.perioder),
        konton_urval: konton.to_string(),
        preliminär: h.status == Status::Preliminär,
        konton: h
            .konton
            .iter()
            .map(|k| maya_pdf::Huvudbokskonto {
                nummer: k.konto,
                namn: k.namn.clone(),
                ingående: k.ingående,
                utgående: k.utgående,
                transaktioner: k
                    .transaktioner
                    .iter()
                    .map(|t| maya_pdf::Transaktion {
                        nummer: t.nummer.to_string(),
                        datum: t.datum,
                        text: t.text.clone(),
                        debet: t.debet,
                        kredit: t.kredit,
                        saldo: t.saldo,
                    })
                    .collect(),
            })
            .collect(),
    });
    let filnamn = filnamn(db, företag, "huvudbok", h.år, &h.perioder, "pdf").await?;
    Ok(utskrift::skriv(db, klocka, aktör, filnamn, &dokument).await?)
}

async fn pdf_företag(
    db: &Databas,
    aktör: &Aktör,
    företag: FöretagsId,
) -> Result<maya_pdf::Företag, ÅtkomstFel> {
    let f = foretag::företaget(db, aktör, företag).await?;
    Ok(maya_pdf::Företag {
        namn: f.namn,
        orgnr: f.orgnr.to_string(),
    })
}

fn pdf_urval(år: Räkenskapsår, perioder: &[Period]) -> maya_pdf::Bokföringsurval {
    maya_pdf::Bokföringsurval {
        år_start: år.start(),
        år_slut: år.slut(),
        från: perioder.first().map_or(år.start(), |p| p.start),
        till: perioder.last().map_or(år.start(), |p| p.start),
    }
}

/// `<rapport>_<orgnr>_<år>_<ÅÅÅÅMM>.<ändelse>`, med räkenskapsårets startår och urvalets
/// sista period.
pub(crate) async fn filnamn(
    db: &Databas,
    företag: FöretagsId,
    rapport: &str,
    år: Räkenskapsår,
    perioder: &[Period],
    ändelse: &str,
) -> Result<String, DatabasFel> {
    let orgnr: String = sqlx::query_scalar("SELECT orgnr FROM foretag WHERE id = ?")
        .bind(företag.0)
        .fetch_one(db.pool())
        .await?;
    let sista = perioder.last().map_or(år.slut(), |p| p.start);
    Ok(format!(
        "{rapport}_{orgnr}_{}_{}.{ändelse}",
        år.start().strftime("%Y"),
        sista.strftime("%Y%m"),
    ))
}

/// Tid i Stockholm till sekunden.
fn tid(t: Timestamp) -> String {
    t.to_zoned(maya_domain::tid::stockholm())
        .strftime("%Y-%m-%d %H:%M:%S")
        .to_string()
}

/// Kronor med decimalkomma och utan tusentalsavgränsare; tomt för noll.
fn belopp(b: Belopp) -> String {
    if b == Belopp::NOLL {
        String::new()
    } else {
        saldo_text(Saldo::från_öre(b.öre()))
    }
}

/// Kronor med decimalkomma, utan tusentalsavgränsare och med minus för kredit.
fn saldo_text(s: Saldo) -> String {
    maya_domain::csv::belopp(s.öre())
}

fn kontonummer(nummer: i64) -> Result<Kontonummer, DatabasFel> {
    u16::try_from(nummer)
        .ok()
        .and_then(Kontonummer::new)
        .ok_or_else(|| DatabasFel::Korrupt(format!("ett kontonummer: {nummer}")))
}
