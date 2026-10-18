//! SIE4-exporten: en 4E-fil per räkenskapsår, för revisorn eller ett annat system.
//!
//! Filen innehåller huvudet, `#RAR 0` och `-1` när föregående år finns i Maya, `#KONTO` och
//! `#SRU` (ur INK2-tabellen) för konton med saldo eller transaktioner något av åren, `#IB`,
//! `#UB` och `#RES` för år 0 och -1 (den preliminära ingående balansen där den gäller) och de
//! bokförda verifikaten i serien V, med `regdatum` = bokföringsdagen och `sign` = visningsnamnet.
//! Utkast, `#PSALDO`, `#RTRANS`/`#BTRANS` och dimensioner skrivs inte.
//!
//! **Återläsning (A4)**: filen är en överföring. Innan den lämnas ut läser `maya-sie` tillbaka
//! den (med `#KSUMMA`), och antalet verifikat, debet och kredit per konto och IB/UB/RES stäms av
//! mot databasen, läst för sig: avstämningen ställer egna frågor (antal och summor direkt ur
//! raderna, UB = IB + årets rader) och använder inget av det som filen byggdes av. Varje
//! avvikelse ger ett fel och ingen fil. En godkänd export
//! loggas i behandlingshistoriken med filens SHA-256, i samma transaktion som läsningen.

use std::collections::BTreeMap;

use jiff::civil::Date;
use maya_domain::kontoplan::{Kontonummer, bas, ink2};
use maya_domain::orgnr::Organisationsnummer;
use maya_domain::tid;
use maya_sie as sie;
use sha2::{Digest, Sha256};
use sqlx::SqliteConnection;

use crate::aktor::{Aktör, Handling};
use crate::atkomst::{Ärende, ÅtkomstFel};
use crate::behandlingshistorik::Händelse;
use crate::foretag::{FöretagsId, datum};
use crate::saldo::{Saldobalans, saldobalanser};
use crate::{Databas, DatabasFel, Klocka};

/// Programmet i `#PROGRAM`.
const PROGRAM: &str = "Maya";
/// Verifikatserien. Maya har en serie per räkenskapsår.
const SERIE: &str = "V";

/// En godkänd export.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sie4Export {
    /// `SIE4_<orgnr>_<år>.se`, där året är räkenskapsårets startår.
    pub filnamn: String,
    /// Filen i CP437.
    pub innehåll: Vec<u8>,
}

#[derive(Debug, thiserror::Error)]
pub enum Sie4Fel {
    #[error("företaget har inget räkenskapsår som börjar det datumet")]
    OkäntRäkenskapsår,
    /// Den skrivna filen gick inte att läsa tillbaka.
    #[error("SIE4-filen kunde inte läsas tillbaka: {0}")]
    Återläsning(#[from] sie::LäsFel),
    /// Den tillbakalästa filen stämmer inte med databasen. Varje rad är en avvikelse.
    #[error("SIE4-filen stämmer inte med bokföringen: {}", .0.join("; "))]
    Avvikelse(Vec<String>),
    #[error(transparent)]
    Åtkomst(#[from] ÅtkomstFel),
}

impl From<sqlx::Error> for Sie4Fel {
    fn from(e: sqlx::Error) -> Self {
        Sie4Fel::Åtkomst(e.into())
    }
}

impl From<DatabasFel> for Sie4Fel {
    fn from(e: DatabasFel) -> Self {
        Sie4Fel::Åtkomst(e.into())
    }
}

/// Exporterar räkenskapsåret som börjar `år` som en SIE 4E-fil, efter återläsning och
/// avstämning, och loggar exporten.
pub async fn exportera(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    företag: FöretagsId,
    år: Date,
) -> Result<Sie4Export, Sie4Fel> {
    exportera_med(db, klocka, aktör, företag, år, |_| {}, |_| {}).await
}

/// [`exportera`], med `läsfel` på de lästa verifikaten före uppbyggnaden och `förvanska` mellan
/// uppbyggnaden och skrivningen, så att tester kan visa att avstämningen stoppar en fil som inte
/// stämmer med databasen.
pub(crate) async fn exportera_med(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    företag: FöretagsId,
    år: Date,
    läsfel: impl FnOnce(&mut Vec<Bokfört>),
    förvanska: impl FnOnce(&mut sie::Sie4),
) -> Result<Sie4Export, Sie4Fel> {
    let mut ärende = Ärende::börja(db, klocka, aktör, företag, Handling::ExporteraSie4).await?;
    let saldobalanser = saldobalanser(ärende.conn(), företag).await?;
    let i = saldobalanser
        .iter()
        .position(|sb| sb.år.start() == år)
        .ok_or(Sie4Fel::OkäntRäkenskapsår)?;
    let åren: Vec<(i32, &Saldobalans)> = [Some((0, &saldobalanser[i]))]
        .into_iter()
        .chain([i.checked_sub(1).map(|f| (-1, &saldobalanser[f]))])
        .flatten()
        .collect();

    let uppgifter = företaget(ärende.conn(), företag).await?;
    let mut verifikat = verifikaten(ärende.conn(), företag, år).await?;
    läsfel(&mut verifikat);
    let namn = kontonamn(ärende.conn(), företag).await?;
    let mut fil = bygg(
        klocka,
        aktör,
        &mut ärende,
        &uppgifter,
        &åren,
        &verifikat,
        &namn,
    )
    .await?;
    förvanska(&mut fil);
    let innehåll = sie::skriv(&fil);

    let läst = sie::läs(&innehåll)?;
    let årsnr: Vec<(i32, Date)> = åren.iter().map(|(n, sb)| (*n, sb.år.start())).collect();
    let facit = facit(ärende.conn(), företag, &årsnr).await?;
    let avvikelser = stäm_av(&läst, &facit);
    if !avvikelser.is_empty() {
        return Err(Sie4Fel::Avvikelse(avvikelser));
    }

    let (start, slut) = (år.to_string(), saldobalanser[i].år.slut().to_string());
    let filnamn = format!("SIE4_{}_{}.se", uppgifter.orgnr.siffror(), år.year());
    ärende
        .logga(
            Händelse::Sie4Exporterad,
            &format!("{start} – {slut}"),
            serde_json::json!({
                "räkenskapsår": { "start": start, "slut": slut },
                "filnamn": filnamn,
                "antal_verifikat": verifikat.len(),
                "byte": innehåll.len(),
                "sha256": hex::encode(Sha256::digest(&innehåll)),
            }),
        )
        .await?;
    ärende.slutför().await?;
    Ok(Sie4Export { filnamn, innehåll })
}

struct Uppgifter {
    orgnr: Organisationsnummer,
    namn: String,
}

/// Ett bokfört verifikat som det står i databasen.
pub(crate) struct Bokfört {
    nummer: u32,
    datum: Date,
    beskrivning: String,
    bokfört: Date,
    signatur: Option<String>,
    /// (konto, debet, kredit, text) i öre.
    rader: Vec<(Kontonummer, i64, i64, Option<String>)>,
}

async fn bygg(
    klocka: &Klocka,
    aktör: &Aktör,
    ärende: &mut Ärende,
    uppgifter: &Uppgifter,
    åren: &[(i32, &Saldobalans)],
    verifikat: &[Bokfört],
    namn: &BTreeMap<Kontonummer, String>,
) -> Result<sie::Sie4, DatabasFel> {
    let exporterad_av = match aktör.användare {
        Some(a) => {
            sqlx::query_scalar("SELECT visningsnamn FROM anvandare WHERE id = ?")
                .bind(a.0)
                .fetch_optional(ärende.conn())
                .await?
        }
        None => None,
    };
    let år0 = åren[0].1;

    // Konton med saldo något av åren eller rader i exportåret, med SRU-koden efter tecknet på
    // exportårets utgående saldo ("Om netto +" i INK2-tabellen = debet).
    let mut konton: BTreeMap<Kontonummer, i64> = BTreeMap::new();
    for (_, sb) in åren {
        for k in &sb.konton {
            konton.entry(k.konto).or_default();
        }
    }
    for k in &år0.konton {
        konton.insert(k.konto, k.utgående.öre());
    }
    for v in verifikat {
        for (konto, ..) in &v.rader {
            konton.entry(*konto).or_default();
        }
    }

    let saldoposter = |välj: fn(&crate::saldo::Kontosaldo) -> i64, balans: bool| {
        åren
            .iter()
            .flat_map(|(årsnr, sb)| {
                sb.konton
                    .iter()
                    .filter(move |k| k.konto.typ().balanskonto() == balans && välj(k) != 0)
                    .map(move |k| sie::Saldopost {
                        årsnr: *årsnr,
                        konto: u32::from(k.konto.get()),
                        öre: välj(k),
                    })
            })
            .collect()
    };

    Ok(sie::Sie4 {
        program: PROGRAM.into(),
        version: env!("CARGO_PKG_VERSION").into(),
        genererad: klocka.idag(),
        genererad_av: exporterad_av,
        orgnr: Some(uppgifter.orgnr.to_string()),
        företagsform: Some("AB".into()),
        företagsnamn: uppgifter.namn.clone(),
        kontoplanstyp: Some(bas::VERSION.replace(' ', "")),
        räkenskapsår: åren
            .iter()
            .map(|(årsnr, sb)| sie::Räkenskapsår {
                årsnr: *årsnr,
                start: sb.år.start(),
                slut: sb.år.slut(),
            })
            .collect(),
        konton: konton
            .iter()
            .map(|(&konto, &utgående)| sie::Konto {
                nummer: u32::from(konto.get()),
                namn: namn.get(&konto).cloned().unwrap_or_default(),
                sru: ink2::koppling(konto).map(|k| u32::from(k.fält(utgående >= 0).kod())),
            })
            .collect(),
        ingående: saldoposter(|k| k.ingående.öre(), true),
        utgående: saldoposter(|k| k.utgående.öre(), true),
        resultat: saldoposter(|k| k.utgående.öre(), false),
        verifikat: verifikat
            .iter()
            .map(|v| sie::Verifikation {
                serie: SERIE.into(),
                nummer: v.nummer,
                datum: v.datum,
                text: v.beskrivning.clone(),
                regdatum: Some(v.bokfört),
                sign: v.signatur.clone(),
                transaktioner: v
                    .rader
                    .iter()
                    .map(|(konto, debet, kredit, text)| sie::Transaktion {
                        konto: u32::from(konto.get()),
                        öre: debet - kredit,
                        text: text.clone(),
                    })
                    .collect(),
            })
            .collect(),
    })
}

/// Det som filen stäms av mot, läst ur databasen för sig.
struct Facit {
    antal_verifikat: usize,
    /// Debet och kredit per konto i exportåret.
    per_konto: BTreeMap<u32, (i64, i64)>,
    /// `#IB`, `#UB` och `#RES` per (årsnr, konto), utan nollor.
    ingående: BTreeMap<(i32, u32), i64>,
    utgående: BTreeMap<(i32, u32), i64>,
    resultat: BTreeMap<(i32, u32), i64>,
}

/// Läser [`Facit`] för `åren` (årsnr, start; år 0 först) med egna frågor: antalet bokförda
/// verifikat och summorna per konto direkt ur raderna, IB ur en ny läsning av saldobalanserna,
/// UB = IB + årets rader och RES = årets rader på resultatkonton.
async fn facit(
    conn: &mut SqliteConnection,
    företag: FöretagsId,
    åren: &[(i32, Date)],
) -> Result<Facit, DatabasFel> {
    let saldobalanser = saldobalanser(&mut *conn, företag).await?;
    let mut facit = Facit {
        antal_verifikat: 0,
        per_konto: BTreeMap::new(),
        ingående: BTreeMap::new(),
        utgående: BTreeMap::new(),
        resultat: BTreeMap::new(),
    };
    for &(årsnr, start) in åren {
        let antal: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM verifikat v
             JOIN rakenskapsar å ON å.id = v.rakenskapsar_id
             WHERE v.foretag_id = ? AND å.start = ? AND v.bokford IS NOT NULL",
        )
        .bind(företag.0)
        .bind(start.to_string())
        .fetch_one(&mut *conn)
        .await?;
        let summor: Vec<(i64, i64, i64)> = sqlx::query_as(
            "SELECT r.konto, sum(r.debet), sum(r.kredit) FROM verifikatrad r
             JOIN verifikat v ON v.id = r.verifikat_id
             JOIN rakenskapsar å ON å.id = v.rakenskapsar_id
             WHERE v.foretag_id = ? AND å.start = ? AND v.bokford IS NOT NULL
             GROUP BY r.konto",
        )
        .bind(företag.0)
        .bind(start.to_string())
        .fetch_all(&mut *conn)
        .await?;

        let ib: BTreeMap<Kontonummer, i64> = saldobalanser
            .iter()
            .find(|sb| sb.år.start() == start)
            .map(|sb| {
                sb.konton
                    .iter()
                    .filter(|k| k.konto.typ().balanskonto())
                    .map(|k| (k.konto, k.ingående.öre()))
                    .collect()
            })
            .unwrap_or_default();
        let mut ub = ib.clone();
        let mut res: BTreeMap<Kontonummer, i64> = BTreeMap::new();
        for &(konto, debet, kredit) in &summor {
            let nr = kontonummer(konto)?;
            if årsnr == 0 {
                facit.per_konto.insert(u32::from(nr.get()), (debet, kredit));
            }
            let saldon = if nr.typ().balanskonto() {
                &mut ub
            } else {
                &mut res
            };
            *saldon.entry(nr).or_default() += debet - kredit;
        }
        if årsnr == 0 {
            facit.antal_verifikat = usize::try_from(antal).unwrap_or_default();
        }
        for (ut, saldon) in [
            (&mut facit.ingående, ib),
            (&mut facit.utgående, ub),
            (&mut facit.resultat, res),
        ] {
            ut.extend(
                saldon
                    .into_iter()
                    .filter(|(_, öre)| *öre != 0)
                    .map(|(konto, öre)| ((årsnr, u32::from(konto.get())), öre)),
            );
        }
    }
    Ok(facit)
}

/// Avvikelserna mellan den tillbakalästa filen och databasen: antal verifikat i serien V,
/// debet och kredit per konto i exportåret, och IB/UB/RES per år och konto.
fn stäm_av(fil: &sie::Sie4, facit: &Facit) -> Vec<String> {
    let mut avvikelser = Vec::new();
    let i_serien = fil.verifikat.iter().filter(|v| v.serie == SERIE).count();
    if i_serien != fil.verifikat.len() || i_serien != facit.antal_verifikat {
        avvikelser.push(format!(
            "filen har {} verifikat ({i_serien} i serien {SERIE}), bokföringen {}",
            fil.verifikat.len(),
            facit.antal_verifikat
        ));
    }

    let mut i_filen: BTreeMap<u32, (i64, i64)> = BTreeMap::new();
    for t in fil.verifikat.iter().flat_map(|v| &v.transaktioner) {
        let (debet, kredit) = i_filen.entry(t.konto).or_default();
        if t.öre >= 0 {
            *debet += t.öre;
        } else {
            *kredit -= t.öre;
        }
    }
    for konto in i_filen
        .keys()
        .chain(facit.per_konto.keys())
        .collect::<std::collections::BTreeSet<_>>()
    {
        let (f, d) = (
            i_filen.get(konto).copied().unwrap_or_default(),
            facit.per_konto.get(konto).copied().unwrap_or_default(),
        );
        if f != d {
            avvikelser.push(format!(
                "konto {konto}: filen har debet {} och kredit {}, bokföringen {} och {}",
                f.0, f.1, d.0, d.1
            ));
        }
    }

    for (etikett, poster, i_db) in [
        ("#IB", &fil.ingående, &facit.ingående),
        ("#UB", &fil.utgående, &facit.utgående),
        ("#RES", &fil.resultat, &facit.resultat),
    ] {
        let mut i_filen: BTreeMap<(i32, u32), i64> = BTreeMap::new();
        for p in poster {
            *i_filen.entry((p.årsnr, p.konto)).or_default() += p.öre;
        }
        i_filen.retain(|_, öre| *öre != 0);
        for nyckel in i_filen
            .keys()
            .chain(i_db.keys())
            .collect::<std::collections::BTreeSet<_>>()
        {
            let (f, d) = (i_filen.get(nyckel), i_db.get(nyckel));
            if f != d {
                avvikelser.push(format!(
                    "{etikett} {} {}: filen har {}, bokföringen {}",
                    nyckel.0,
                    nyckel.1,
                    f.copied().unwrap_or_default(),
                    d.copied().unwrap_or_default()
                ));
            }
        }
    }
    avvikelser
}

async fn företaget(
    conn: &mut SqliteConnection,
    företag: FöretagsId,
) -> Result<Uppgifter, DatabasFel> {
    let (orgnr, namn): (String, String) =
        sqlx::query_as("SELECT orgnr, namn FROM foretag WHERE id = ?")
            .bind(företag.0)
            .fetch_one(conn)
            .await?;
    Ok(Uppgifter {
        orgnr: Organisationsnummer::tolka(&orgnr)
            .map_err(|_| DatabasFel::Korrupt(format!("ett organisationsnummer: {orgnr}")))?,
        namn,
    })
}

async fn kontonamn(
    conn: &mut SqliteConnection,
    företag: FöretagsId,
) -> Result<BTreeMap<Kontonummer, String>, DatabasFel> {
    let rader: Vec<(i64, String)> =
        sqlx::query_as("SELECT nummer, namn FROM konto WHERE foretag_id = ?")
            .bind(företag.0)
            .fetch_all(conn)
            .await?;
    rader
        .into_iter()
        .map(|(nr, namn)| Ok((kontonummer(nr)?, namn)))
        .collect()
}

/// De bokförda verifikaten i räkenskapsåret som börjar `år`, i nummerordning.
async fn verifikaten(
    conn: &mut SqliteConnection,
    företag: FöretagsId,
    år: Date,
) -> Result<Vec<Bokfört>, DatabasFel> {
    type Rad = (i64, i64, String, String, String, Option<String>);
    let rader: Vec<Rad> = sqlx::query_as(
        "SELECT v.id, v.nummer, v.datum, v.beskrivning, v.bokford, u.visningsnamn
         FROM verifikat v
         JOIN rakenskapsar r ON r.id = v.rakenskapsar_id
         LEFT JOIN anvandare u ON u.id = v.bokford_av
         WHERE v.foretag_id = ? AND r.start = ? AND v.bokford IS NOT NULL
         ORDER BY v.nummer",
    )
    .bind(företag.0)
    .bind(år.to_string())
    .fetch_all(&mut *conn)
    .await?;
    type Radrad = (i64, i64, i64, i64, Option<String>);
    let alla_rader: Vec<Radrad> = sqlx::query_as(
        "SELECT r.verifikat_id, r.konto, r.debet, r.kredit, r.text
         FROM verifikatrad r
         JOIN verifikat v ON v.id = r.verifikat_id
         JOIN rakenskapsar å ON å.id = v.rakenskapsar_id
         WHERE v.foretag_id = ? AND å.start = ? AND v.bokford IS NOT NULL
         ORDER BY r.verifikat_id, r.radnr",
    )
    .bind(företag.0)
    .bind(år.to_string())
    .fetch_all(&mut *conn)
    .await?;
    let mut per_verifikat: BTreeMap<i64, Vec<_>> = BTreeMap::new();
    for (id, konto, debet, kredit, text) in alla_rader {
        per_verifikat
            .entry(id)
            .or_default()
            .push((kontonummer(konto)?, debet, kredit, text));
    }
    let korrupt = |vad: &str| DatabasFel::Korrupt(format!("verifikat.{vad}"));
    rader
        .into_iter()
        .map(|(id, nummer, datum_, beskrivning, bokford, signatur)| {
            let bokford: jiff::Timestamp = bokford.parse().map_err(|_| korrupt("bokford"))?;
            Ok(Bokfört {
                nummer: u32::try_from(nummer).map_err(|_| korrupt("nummer"))?,
                datum: datum(&datum_)?,
                beskrivning,
                bokfört: tid::datum(bokford),
                signatur,
                rader: per_verifikat.remove(&id).unwrap_or_default(),
            })
        })
        .collect()
}

fn kontonummer(nummer: i64) -> Result<Kontonummer, DatabasFel> {
    u16::try_from(nummer)
        .ok()
        .and_then(Kontonummer::new)
        .ok_or_else(|| DatabasFel::Korrupt(format!("ett kontonummer: {nummer}")))
}
