//! Att göra, företagets startsida: dagens datum, nyckeltalen ur bokföringen och sakerna att
//! göra. I etapp 1 är sakerna varningen efter en återställning, driftvarningar, verifikatutkast
//! och okopplade underlag; varje ny sort läggs till som en variant i [`Sak`] och räknas i
//! [`antal`]. Etapp 2 (#152) lägger till förfallna kundfakturor och leverantörsfakturor att
//! betala, och nyckeltalen Kundfordringar och Leverantörsskulder.
//!
//! Fristerna (T1) räknas i `maya_domain::att_gora`. De varnar men spärrar inte, och Maya bokför
//! aldrig automatiskt.

use std::collections::BTreeMap;

use jiff::Timestamp;
use jiff::civil::Date;
use maya_domain::att_gora::{self as regler, Frist, Utkastläge};
use maya_domain::belopp::{Belopp, Saldo};
use maya_domain::foretag::{Bokföringsmetod, gäller};
use maya_domain::kontoplan::Kontonummer;
use maya_domain::rakenskapsar::Räkenskapsår;
use sqlx::SqliteConnection;

use crate::aktor::{self, Aktör, Handling};
use crate::atkomst::{self, ÅtkomstFel};
use crate::config::Config;
use crate::drift::{self, Driftvarning};
use crate::foretag::{self, FöretagsId};
use crate::inloggning::tidpunkt;
use crate::kundfaktura::{self, KundfakturaId};
use crate::leverantorsfaktura::LeverantörsfakturaId;
use crate::saldo;
use crate::underlag::{self, Underlag};
use crate::verifikat::VerifikatId;
use crate::{Databas, DatabasFel, Klocka};

/// Företagskontot i BAS.
const FÖRETAGSKONTOT: u16 = 1930;
/// Årets resultat, som inte räknas in i resultatet.
const ÅRETS_RESULTAT: u16 = 8999;
/// Kundfordringar i BAS: kundreskontrans konto.
const KUNDFORDRINGAR: u16 = 1510;
/// Leverantörsskulder i BAS: leverantörsreskontrans konto.
const LEVERANTÖRSSKULDER: u16 = 2440;

/// Det som Att göra visar för ett företag.
#[derive(Debug, Clone, PartialEq)]
pub struct AttGöra {
    /// Dagens datum i Stockholm.
    pub idag: Date,
    pub nyckeltal: Nyckeltal,
    /// Återställningarna som användaren inte har kvitterat, driftvarningarna, utkasten i
    /// id-ordning och de okopplade underlagen äldst först.
    pub saker: Vec<Sak>,
}

/// Nyckeltalen överst, räknade ur bokförda verifikat daterade till och med i dag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Nyckeltal {
    /// Saldot på 1930 per i dag, debet positivt.
    pub företagskontot: Saldo,
    /// Klass 3–8 utan 8999 i innevarande räkenskapsår, med tecknet vänt så att vinst är
    /// positiv.
    pub resultat_i_år: Saldo,
    /// Innevarande räkenskapsår, om det finns i Maya.
    pub år: Option<Räkenskapsår>,
    /// Kundfordringarna: 1510 ur bokföringen, eller reskontran under kontantmetoden.
    pub kundfordringar: Reskontratal,
    /// Leverantörsskulderna: 2440 ur bokföringen med kredit positivt, eller reskontran under
    /// kontantmetoden.
    pub leverantörsskulder: Reskontratal,
}

/// Nyckeltalet för en reskontra (#152).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Reskontratal {
    /// Saldot på reskontrakontot per i dag ur bokföringen under fakturametoden; under
    /// kontantmetoden det som återstår på reskontrans fakturor.
    pub saldo: Saldo,
    /// Det som återstår på fakturorna efter förfallodagen, ur reskontran.
    pub förfallet: Saldo,
    /// Saldot är reskontrans, inte bokföringens: kontantmetoden gäller i dag.
    pub ur_reskontran: bool,
}

/// En sak att göra.
#[derive(Debug, Clone, PartialEq)]
pub enum Sak {
    /// Installationen har återställts ur en backup: det som registrerades efter backupen måste
    /// registreras om. Visas överst för varje användare tills hen kvitterar.
    Återställning(Återställningsvarning),
    /// Något som är fel i driften. Samma i alla företag, och visas överst.
    Driftvarning(Driftvarning),
    Utkast(Utkastsak),
    /// Ett underlag utan koppling till något verifikat: "Vad är det här?".
    Underlag(Underlag),
    /// #152: en förfallen kundfaktura eller en obetald leverantörsfaktura, i förfallodagens
    /// ordning.
    Faktura(Fakturasak),
}

/// Fakturan i en [`Fakturasak`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reskontrafaktura {
    Kund(KundfakturaId),
    Leverantör(LeverantörsfakturaId),
}

/// En faktura med något kvar att betala: en utfärdad kundfaktura efter förfallodagen, eller en
/// registrerad leverantörsfaktura, vars förfallodag avgör var den står i tidslinjen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fakturasak {
    pub faktura: Reskontrafaktura,
    /// Kundfakturans nummer i serien eller leverantörens fakturanummer.
    pub nummer: String,
    /// Kundens eller leverantörens namn.
    pub motpart: String,
    /// I öre.
    pub total: i64,
    /// Det som återstår i öre, större än noll.
    pub återstår: i64,
    pub förfallodag: Date,
    /// Leverantörsfakturans BG/PG/IBAN och OCR eller meddelande att kopiera. Tomt för en
    /// kundfaktura.
    pub kopieringschips: Vec<(&'static str, String)>,
    /// Kundens e-post på kundfakturan. Tomt för en leverantörsfaktura.
    pub kontakt: String,
}

/// Id för en återställning med `maya restore`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ÅterställningsId(i64);

/// Vem som helst kan skriva ett id; kvittensen gäller bara aktörens egen varning.
impl std::str::FromStr for ÅterställningsId {
    type Err = std::num::ParseIntError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse().map(ÅterställningsId)
    }
}

impl std::fmt::Display for ÅterställningsId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// En återställning som användaren ännu inte har kvitterat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Återställningsvarning {
    pub id: ÅterställningsId,
    /// När återställningen gjordes.
    pub tid: Timestamp,
    /// Backupens tidpunkt: det som registrerades efter den finns inte längre.
    pub backup_tid: Timestamp,
}

/// Ett verifikatutkast som väntar på att bokföras.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Utkastsak {
    pub id: VerifikatId,
    pub datum: Option<Date>,
    pub beskrivning: String,
    /// Summan av debet.
    pub summa: Belopp,
    pub skapad: Timestamp,
    /// Antalet kopplade underlag.
    pub underlag: usize,
    /// Minst en rad på ett konto i grupp 191.
    pub kassa: bool,
}

impl Utkastsak {
    pub fn läge(&self) -> Utkastläge {
        Utkastläge {
            datum: self.datum,
            kassa: self.kassa,
        }
    }

    pub fn frist(&self, idag: Date) -> Frist {
        regler::frist(&self.läge(), idag)
    }
}

/// Att göra för företaget i dag. Driftvarningarna räknas ur `config` och backupens körningar.
pub async fn att_göra(
    db: &Databas,
    klocka: &Klocka,
    config: &Config,
    aktör: &Aktör,
    företag: FöretagsId,
) -> Result<AttGöra, ÅtkomstFel> {
    let idag = klocka.idag();
    let mut tx = db.pool().begin().await?;
    atkomst::kräv(&mut tx, aktör, företag, Handling::LäsaFöretag).await?;
    let fakturor = fakturor(&mut tx, företag).await?;
    let nyckeltal = nyckeltal(&mut tx, företag, idag, &fakturor).await?;
    let mut saker: Vec<Sak> = återställningar(&mut tx, aktör)
        .await?
        .into_iter()
        .map(Sak::Återställning)
        .collect();
    let utkast = utkast(&mut tx, företag).await?;
    tx.commit().await?;
    saker.extend(
        drift::driftvarningar(db, klocka, config)
            .await?
            .into_iter()
            .map(Sak::Driftvarning),
    );
    saker.extend(utkast.into_iter().map(Sak::Utkast));
    let okopplade = underlag::lista(db, aktör, företag, underlag::Urval::Okopplade).await?;
    saker.extend(okopplade.into_iter().map(Sak::Underlag));
    // #152 Fakturorna: kundfakturorna när de har förfallit, leverantörsfakturorna alla.
    saker.extend(
        fakturor
            .into_iter()
            .filter(|f| {
                matches!(f.faktura, Reskontrafaktura::Leverantör(_)) || f.förfallodag < idag
            })
            .map(Sak::Faktura),
    );
    Ok(AttGöra {
        idag,
        nyckeltal,
        saker,
    })
}

/// Antalet saker att göra i vart och ett av företagen som aktören har åtkomst till, för
/// toppmenyn och företagsväljaren.
pub async fn antal(
    db: &Databas,
    klocka: &Klocka,
    config: &Config,
    aktör: &Aktör,
) -> Result<BTreeMap<FöretagsId, usize>, DatabasFel> {
    let företagen = foretag::mina_företag(db, aktör).await?;
    if företagen.is_empty() {
        return Ok(BTreeMap::new());
    }
    let varningar = drift::driftvarningar(db, klocka, config).await?.len();
    let mut antal: BTreeMap<FöretagsId, usize> =
        företagen.iter().map(|f| (f.id, varningar)).collect();
    // #152 Leverantörsfakturorna räknas också när de förfaller inom 7 dagar, med det som
    // återstår räknat som i `fakturor`. Kundfakturorna räknas nedan.
    let rader: Vec<(i64, i64)> = sqlx::query_as(
        "SELECT foretag_id, count(*) FROM verifikat WHERE bokford IS NULL GROUP BY foretag_id
         UNION ALL
         SELECT u.foretag_id, count(*) FROM underlag u
         WHERE NOT EXISTS (SELECT 1 FROM underlagskoppling k WHERE k.underlag_id = u.id)
           AND NOT EXISTS (SELECT 1 FROM leverantorsfaktura f WHERE f.underlag_id = u.id)
         GROUP BY u.foretag_id
         UNION ALL
         SELECT f.foretag_id, count(*) FROM leverantorsfaktura f
         WHERE f.status <> 'utkast' AND f.forfallodag <= date(?1, '+7 days')
           AND f.total > coalesce((SELECT sum(b.belopp + b.oresutjamning + b.kursdifferens)
                                   FROM betalning_i_reskontran b
                                   WHERE b.leverantorsfaktura_id = f.id), 0)
         GROUP BY f.foretag_id",
    )
    .bind(klocka.idag().to_string())
    .fetch_all(db.pool())
    .await?;
    for (företag, n) in rader {
        if let Some(a) = antal.get_mut(&FöretagsId(företag)) {
            *a += usize::try_from(n).unwrap_or_default();
        }
    }
    // #152 Kundfakturorna räknas när de har förfallit och något återstår, med det som återstår
    // ur `kundfaktura::lista_i` som i `fakturor`: en gällande kundförlust (#141) har skrivit
    // bort resten, så den fakturan räknas inte.
    let idag = klocka.idag();
    let mut conn = db.pool().acquire().await?;
    for (företag, a) in antal.iter_mut() {
        *a += kundfaktura::lista_i(&mut conn, *företag, None)
            .await?
            .iter()
            .filter(|f| {
                f.status != kundfaktura::Status::Utkast
                    && f.återstår > 0
                    && f.förfallodag.is_some_and(|d| d < idag)
            })
            .count();
    }
    // Återställningsvarningen visas i alla användarens företag.
    let varningar = återställningar(&mut conn, aktör).await?.len();
    for a in antal.values_mut() {
        *a += varningar;
    }
    Ok(antal)
}

/// Antalet utkast per företag som aktören har åtkomst till, för företagsväljaren (berättelse 52).
pub async fn antal_utkast(
    db: &Databas,
    aktör: &Aktör,
) -> Result<BTreeMap<FöretagsId, usize>, DatabasFel> {
    let företagen = foretag::mina_företag(db, aktör).await?;
    let rader: Vec<(i64, i64)> = sqlx::query_as(
        "SELECT foretag_id, count(*) FROM verifikat WHERE bokford IS NULL GROUP BY foretag_id",
    )
    .fetch_all(db.pool())
    .await?;
    Ok(rader
        .into_iter()
        .map(|(f, n)| (FöretagsId(f), usize::try_from(n).unwrap_or_default()))
        .filter(|(f, _)| företagen.iter().any(|m| m.id == *f))
        .collect())
}

/// Kvitterar varningen efter återställningen `id` för aktörens användare. Andra användares
/// varningar ligger kvar. En redan kvitterad eller okänd varning ändrar inget.
pub async fn kvittera_återställning(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    id: ÅterställningsId,
) -> Result<(), ÅtkomstFel> {
    let Some(användare) = aktör.användare else {
        return Err(ÅtkomstFel::KanalenFårInte);
    };
    if !aktor::tillåter(aktör.kanal, Handling::KvitteraÅterställning) {
        return Err(ÅtkomstFel::KanalenFårInte);
    }
    sqlx::query(
        "UPDATE aterstallningsvarning SET kvitterad = ?
         WHERE aterstallning_id = ? AND anvandare_id = ? AND kvitterad IS NULL",
    )
    .bind(klocka.nu().to_string())
    .bind(id.0)
    .bind(användare.0)
    .execute(db.pool())
    .await?;
    Ok(())
}

/// Återställningarna som aktörens användare inte har kvitterat, äldst först.
async fn återställningar(
    conn: &mut SqliteConnection,
    aktör: &Aktör,
) -> Result<Vec<Återställningsvarning>, DatabasFel> {
    let Some(användare) = aktör.användare else {
        return Ok(Vec::new());
    };
    let rader: Vec<(i64, String, String)> = sqlx::query_as(
        "SELECT a.id, a.tid, a.backup_tid FROM aterstallning a
         JOIN aterstallningsvarning v ON v.aterstallning_id = a.id
         WHERE v.anvandare_id = ? AND v.kvitterad IS NULL
         ORDER BY a.id",
    )
    .bind(användare.0)
    .fetch_all(&mut *conn)
    .await?;
    rader
        .into_iter()
        .map(|(id, tid, backup_tid)| {
            Ok(Återställningsvarning {
                id: ÅterställningsId(id),
                tid: tidpunkt(&tid)?,
                backup_tid: tidpunkt(&backup_tid)?,
            })
        })
        .collect()
}

async fn nyckeltal(
    conn: &mut SqliteConnection,
    företag: FöretagsId,
    idag: Date,
    fakturor: &[Fakturasak],
) -> Result<Nyckeltal, DatabasFel> {
    let saldobalanser = saldo::saldobalanser_till(conn, företag, Some(idag)).await?;
    // Det senaste året som har börjat: dess utgående balans är saldot per i dag.
    let senaste = saldobalanser.iter().rev().find(|sb| sb.år.start() <= idag);
    let saldot = |konto: u16| {
        senaste
            .and_then(|sb| {
                sb.konton
                    .iter()
                    .find(|k| k.konto.get() == konto)
                    .map(|k| k.utgående)
            })
            .unwrap_or_default()
    };
    let företagskontot = saldot(FÖRETAGSKONTOT);
    let innevarande = senaste.filter(|sb| sb.år.innehåller(idag));
    let resultat_i_år = innevarande
        .map(|sb| {
            sb.konton
                .iter()
                .filter(|k| (3..=8).contains(&k.konto.klass()) && k.konto.get() != ÅRETS_RESULTAT)
                .map(|k| k.förändring)
                .sum::<Saldo>()
        })
        .map(|s| Saldo::från_öre(-s.öre()))
        .unwrap_or_default();
    // #152 Under kontantmetoden är fordringarna och skulderna inte bokförda: rutorna visar
    // reskontrans obetalda belopp.
    let metoder = foretag::läs_företag(conn, företag).await?.bokföringsmetoder;
    let kontant = gäller(&metoder, idag).or(metoder.first().map(|(_, m)| *m))
        == Some(Bokföringsmetod::Kontant);
    let reskontratal = |kund: bool, bokfört: Saldo| {
        let fakturorna = || {
            fakturor
                .iter()
                .filter(move |f| matches!(f.faktura, Reskontrafaktura::Kund(_)) == kund)
        };
        let obetalt = Saldo::från_öre(fakturorna().map(|f| f.återstår).sum());
        Reskontratal {
            saldo: if kontant { obetalt } else { bokfört },
            förfallet: Saldo::från_öre(
                fakturorna()
                    .filter(|f| f.förfallodag < idag)
                    .map(|f| f.återstår)
                    .sum(),
            ),
            ur_reskontran: kontant,
        }
    };
    Ok(Nyckeltal {
        företagskontot,
        resultat_i_år,
        år: innevarande.map(|sb| sb.år),
        kundfordringar: reskontratal(true, saldot(KUNDFORDRINGAR)),
        leverantörsskulder: reskontratal(
            false,
            Saldo::från_öre(-saldot(LEVERANTÖRSSKULDER).öre()),
        ),
    })
}

/// Reskontrornas fakturor med något kvar att betala, i förfallodagens ordning: de utfärdade
/// kundfakturorna och de registrerade leverantörsfakturorna (#152). Det som återstår på en
/// kundfaktura räknas av `kundfaktura::lista_i`, samma som i listan: en gällande kundförlust
/// (#141) har skrivit bort resten, så fakturan är varken obetald eller förfallen.
async fn fakturor(
    conn: &mut SqliteConnection,
    företag: FöretagsId,
) -> Result<Vec<Fakturasak>, DatabasFel> {
    let mut fakturor = Vec::new();
    for f in kundfaktura::lista_i(&mut *conn, företag, None).await? {
        let (Some(nummer), Some(förfallodag)) = (f.nummer, f.förfallodag) else {
            continue;
        };
        if f.status == kundfaktura::Status::Utkast || f.återstår <= 0 {
            continue;
        }
        let kontakt: Option<String> =
            sqlx::query_scalar("SELECT kunduppgifter ->> '$.epost' FROM kundfaktura WHERE id = ?")
                .bind(f.id.0)
                .fetch_one(&mut *conn)
                .await?;
        fakturor.push(Fakturasak {
            faktura: Reskontrafaktura::Kund(f.id),
            nummer: nummer.to_string(),
            motpart: f.kund,
            total: f.total,
            återstår: f.återstår,
            förfallodag,
            kopieringschips: Vec::new(),
            kontakt: kontakt.unwrap_or_default(),
        });
    }
    // Det som återstår på en leverantörsfaktura: totalen minus betalningarna i reskontran med
    // öresutjämning och kursdifferens (#147), samma regel som `betalning::betalt_i`.
    type Rad = (
        i64,
        String,
        String,
        i64,
        i64,
        String,
        String,
        String,
        String,
        String,
    );
    let rader: Vec<Rad> = sqlx::query_as(
        "SELECT f.id, f.fakturanummer, l.namn, f.total,
                f.total - coalesce((SELECT sum(b.belopp + b.oresutjamning + b.kursdifferens)
                                    FROM betalning_i_reskontran b
                                    WHERE b.leverantorsfaktura_id = f.id), 0),
                f.forfallodag, l.bankgiro, l.plusgiro, l.iban, f.referens
         FROM leverantorsfaktura f JOIN leverantor l ON l.id = f.leverantor_id
         WHERE f.foretag_id = ? AND f.status <> 'utkast'
         ORDER BY f.forfallodag, f.id",
    )
    .bind(företag.0)
    .fetch_all(&mut *conn)
    .await?;
    for (id, nummer, motpart, total, återstår, förfallodag, bg, pg, iban, referens) in rader {
        if återstår <= 0 {
            continue;
        }
        fakturor.push(Fakturasak {
            faktura: Reskontrafaktura::Leverantör(LeverantörsfakturaId(id)),
            nummer,
            motpart,
            total,
            återstår,
            förfallodag: foretag::datum(&förfallodag)?,
            kopieringschips: maya_domain::reskontra::kopieringschips(&bg, &pg, &iban, &referens),
            kontakt: String::new(),
        });
    }
    // Stabil: lika förfallodagar behåller kundfakturorna först.
    fakturor.sort_by_key(|f| f.förfallodag);
    Ok(fakturor)
}

async fn utkast(
    conn: &mut SqliteConnection,
    företag: FöretagsId,
) -> Result<Vec<Utkastsak>, DatabasFel> {
    type Rad = (i64, Option<String>, String, i64, String, i64);
    let rader: Vec<Rad> = sqlx::query_as(
        "SELECT v.id, v.datum, v.beskrivning,
                coalesce((SELECT sum(debet) FROM verifikatrad WHERE verifikat_id = v.id), 0),
                v.skapad,
                (SELECT count(*) FROM underlagskoppling WHERE verifikat_id = v.id)
         FROM verifikat v
         WHERE v.foretag_id = ? AND v.bokford IS NULL
         ORDER BY v.id",
    )
    .bind(företag.0)
    .fetch_all(&mut *conn)
    .await?;
    let konton: Vec<(i64, i64)> = sqlx::query_as(
        "SELECT r.verifikat_id, r.konto FROM verifikatrad r
         JOIN verifikat v ON v.id = r.verifikat_id
         WHERE v.foretag_id = ? AND v.bokford IS NULL AND r.konto IS NOT NULL",
    )
    .bind(företag.0)
    .fetch_all(&mut *conn)
    .await?;
    rader
        .into_iter()
        .map(|(id, datum, beskrivning, summa, skapad, underlag)| {
            let kassa = regler::rör_kassan(
                konton
                    .iter()
                    .filter(|(v, _)| *v == id)
                    .filter_map(|(_, k)| u16::try_from(*k).ok().and_then(Kontonummer::new)),
            );
            Ok(Utkastsak {
                id: VerifikatId(id),
                datum: datum.as_deref().map(foretag::datum).transpose()?,
                beskrivning,
                summa: Belopp::från_öre(summa),
                skapad: tidpunkt(&skapad)?,
                underlag: usize::try_from(underlag).unwrap_or_default(),
                kassa,
            })
        })
        .collect()
}
