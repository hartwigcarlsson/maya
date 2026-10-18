//! Resultat- och balansräkningen ur bokföringen. Saldona kommer från [`crate::saldo`] (bara
//! bokförda verifikat, aldrig utkast) och ställs upp enligt INK2-kopplingstabellen i
//! `maya_domain::rapport`.

use jiff::civil::Date;
use maya_domain::belopp::{Belopp, Saldo};
use maya_domain::kontoplan::Kontonummer;
use maya_domain::rapport::{Rapport, Årsdata};

use crate::aktor::{Aktör, Handling};
use crate::atkomst;
use crate::foretag::{self, FöretagsId};
use crate::saldo::{self, SaldoFel, Saldobalans, Status};
use crate::verifikat::{VerifikatId, Verifikationsnummer};
use crate::{Databas, DatabasFel, kontoplan};

pub use maya_domain::rapport::Urval;

/// Rapporten i urvalet för räkenskapsåret som börjar `år`, med föregående år som jämförelse.
pub async fn rapport(
    db: &Databas,
    aktör: &Aktör,
    företag: FöretagsId,
    år: Date,
    urval: &Urval,
) -> Result<Rapport, SaldoFel> {
    let mut tx = db.pool().begin().await?;
    atkomst::kräv(&mut tx, aktör, företag, Handling::LäsaFöretag).await?;
    let balanser = saldo::saldobalanser(&mut tx, företag).await?;
    let mut rörelser = saldo::månadsrörelser(&mut tx, företag).await?;
    drop(tx);
    let i = balanser
        .iter()
        .position(|sb| sb.år.start() == år)
        .ok_or(SaldoFel::OkäntRäkenskapsår)?;
    let mut årsdata = |sb: &Saldobalans| Årsdata {
        år: sb.år,
        ingående: sb
            .konton
            .iter()
            .filter(|k| k.ingående.öre() != 0)
            .map(|k| (k.konto, k.ingående))
            .collect(),
        preliminär: sb.status == Status::Preliminär,
        rörelser: rörelser.remove(&sb.år.start()).unwrap_or_default(),
    };
    let detta = årsdata(&balanser[i]);
    let föregående = i.checked_sub(1).map(|f| årsdata(&balanser[f]));
    let konton = kontoplan::konton(db, aktör, företag).await?;
    let namn: Vec<_> = konton.into_iter().map(|k| (k.nummer, k.namn)).collect();
    Ok(Rapport::ställ_upp(
        urval,
        &detta,
        föregående.as_ref(),
        &namn,
    ))
}

/// (verifikat, nummer, datum, beskrivning, radtext, debet, kredit) ur databasen.
type Utdragsfält = (i64, i64, String, String, Option<String>, i64, i64);

/// En rapport som CSV för kalkylark.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Csv {
    /// `<rapport>_<orgnr>_<år>_<ÅÅÅÅMM>.csv`
    pub filnamn: String,
    /// UTF-8 med BOM, semikolon och decimalkomma, en rad per konto.
    pub innehåll: String,
}

/// Rapporten i urvalet som CSV, med kolumnerna som rapporten visar.
pub async fn csv(
    db: &Databas,
    aktör: &Aktör,
    företag: FöretagsId,
    år: Date,
    urval: &Urval,
) -> Result<Csv, SaldoFel> {
    let r = rapport(db, aktör, företag, år, urval).await?;
    let orgnr = foretag::företaget(db, aktör, företag).await?.orgnr;
    Ok(Csv {
        filnamn: format!("{}.csv", r.filnamn(&orgnr.to_string())),
        innehåll: r.csv(),
    })
}

/// Ett kontos bokförda verifikat i perioden som en rapport visar, med löpande saldo (debet
/// positivt): panelen bakom ett konto.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Kontoutdrag {
    pub konto: Kontonummer,
    pub kontonamn: String,
    pub från: Date,
    pub till: Date,
    /// Den ingående balansen, när perioden börjar med den (balansräkningen).
    pub ingående: Option<Saldo>,
    pub rader: Vec<Utdragsrad>,
    pub utgående: Saldo,
}

/// En verifikatrad på kontot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Utdragsrad {
    pub verifikat: VerifikatId,
    pub nummer: Verifikationsnummer,
    pub datum: Date,
    pub beskrivning: String,
    pub text: Option<String>,
    pub debet: Belopp,
    pub kredit: Belopp,
    /// Saldot efter raden.
    pub saldo: Saldo,
}

/// Kontots verifikat i perioden som urvalet ger för räkenskapsåret som börjar `år`, i datum-
/// och nummerordning. Bara bokförda verifikat.
pub async fn kontoutdrag(
    db: &Databas,
    aktör: &Aktör,
    företag: FöretagsId,
    år: Date,
    konto: Kontonummer,
    urval: &Urval,
) -> Result<Kontoutdrag, SaldoFel> {
    let mut tx = db.pool().begin().await?;
    atkomst::kräv(&mut tx, aktör, företag, Handling::LäsaFöretag).await?;
    let balans = saldo::saldobalanser(&mut tx, företag)
        .await?
        .into_iter()
        .find(|sb| sb.år.start() == år)
        .ok_or(SaldoFel::OkäntRäkenskapsår)?;
    let (från, till, med_ingående) = urval.kontoperiod(&balans.år);
    let rader: Vec<Utdragsfält> = sqlx::query_as(
        "SELECT v.id, v.nummer, v.datum, v.beskrivning, r.text, r.debet, r.kredit
         FROM verifikatrad r JOIN verifikat v ON v.id = r.verifikat_id
         WHERE v.foretag_id = ? AND v.bokford IS NOT NULL AND r.konto = ?
           AND v.datum >= ? AND v.datum <= ?
         ORDER BY v.datum, v.nummer, r.radnr",
    )
    .bind(företag.0)
    .bind(i64::from(konto.get()))
    .bind(från.to_string())
    .bind(till.to_string())
    .fetch_all(&mut *tx)
    .await?;
    let kontonamn: Option<String> =
        sqlx::query_scalar("SELECT namn FROM konto WHERE foretag_id = ? AND nummer = ?")
            .bind(företag.0)
            .bind(i64::from(konto.get()))
            .fetch_optional(&mut *tx)
            .await?;
    drop(tx);

    let ingående = med_ingående.then(|| {
        balans
            .konton
            .iter()
            .find(|k| k.konto == konto)
            .map(|k| k.ingående)
            .unwrap_or_default()
    });
    let mut saldo = ingående.unwrap_or_default();
    let rader = rader
        .into_iter()
        .map(|(id, nummer, datum, beskrivning, text, debet, kredit)| {
            let (debet, kredit) = (Belopp::från_öre(debet), Belopp::från_öre(kredit));
            saldo += Saldo::av(debet, kredit);
            Ok(Utdragsrad {
                verifikat: VerifikatId(id),
                nummer: Verifikationsnummer(
                    u32::try_from(nummer)
                        .map_err(|_| DatabasFel::Korrupt(format!("ett nummer: {nummer}")))?,
                ),
                datum: foretag::datum(&datum)?,
                beskrivning,
                text: text.filter(|t| !t.is_empty()),
                debet,
                kredit,
                saldo,
            })
        })
        .collect::<Result<Vec<_>, DatabasFel>>()?;
    Ok(Kontoutdrag {
        konto,
        kontonamn: kontonamn.unwrap_or_default(),
        från,
        till,
        ingående,
        rader,
        utgående: saldo,
    })
}
