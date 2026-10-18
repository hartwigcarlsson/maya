//! Behandlingshistoriken per företag: varje ändring som påverkar postbehandlingen, med tid och
//! aktör (H1, H2). Append-only i två lager (H3): `core` har ingen kodväg som ändrar eller
//! raderar en post, och schemat avvisar `UPDATE` och `DELETE`.

use jiff::Timestamp;
use jiff::civil::Date;
use maya_domain::belopp::Saldo;
use maya_domain::foretag::{Bokföringsmetod, Momsperiod};
use serde_json::Value;
use sqlx::SqliteConnection;

use crate::aktor::{Aktör, AnvändarId, Handling, Kanal};
use crate::atkomst::{self, ÅtkomstFel};
use crate::foretag::FöretagsId;
use crate::{Databas, DatabasFel};

/// Händelsetyperna i behandlingshistoriken. Lägg till nya här.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Händelse {
    FöretagSkapat,
    FöretagÄndrat,
    KontoSkapat,
    KontoNamnbytt,
    KontoAktiverat,
    KontoInaktiverat,
    BasTillägg,
    RäkenskapsårSkapat,
    RäkenskapsårÄndrat,
    BokföringsmetodBytt,
    MomsperiodBytt,
    VerifikatBokfört,
    VerifikatRättat,
    UtkastRaderat,
    IngåendeBalansÄndrad,
    UnderlagUppladdat,
    UnderlagBorttaget,
    VerifikatKompletterat,
    ÅtkomstGiven,
    ÅtkomstTagen,
    AnvändareAvaktiverad,
    InbjudanSkapad,
    InbjudanAnvänd,
    Sie4Exporterad,
    ÅterställningGjord,
    MigreringKörd,
    MigreringMisslyckad,
    ProgramversionBytt,
}

impl Händelse {
    /// Alla typer som har en kodväg. En ny typ läggs till här också, så att testet som kör
    /// varje typ genom `core` måste få en kodväg för den.
    pub const ALLA: [Händelse; 28] = [
        Händelse::FöretagSkapat,
        Händelse::FöretagÄndrat,
        Händelse::KontoSkapat,
        Händelse::KontoNamnbytt,
        Händelse::KontoAktiverat,
        Händelse::KontoInaktiverat,
        Händelse::BasTillägg,
        Händelse::RäkenskapsårSkapat,
        Händelse::RäkenskapsårÄndrat,
        Händelse::BokföringsmetodBytt,
        Händelse::MomsperiodBytt,
        Händelse::VerifikatBokfört,
        Händelse::VerifikatRättat,
        Händelse::UtkastRaderat,
        Händelse::IngåendeBalansÄndrad,
        Händelse::UnderlagUppladdat,
        Händelse::UnderlagBorttaget,
        Händelse::VerifikatKompletterat,
        Händelse::ÅtkomstGiven,
        Händelse::ÅtkomstTagen,
        Händelse::AnvändareAvaktiverad,
        Händelse::InbjudanSkapad,
        Händelse::InbjudanAnvänd,
        Händelse::Sie4Exporterad,
        Händelse::ÅterställningGjord,
        Händelse::MigreringKörd,
        Händelse::MigreringMisslyckad,
        Händelse::ProgramversionBytt,
    ];

    pub fn namn(self) -> &'static str {
        match self {
            Händelse::FöretagSkapat => "företag skapat",
            Händelse::FöretagÄndrat => "företag ändrat",
            Händelse::KontoSkapat => "konto skapat",
            Händelse::KontoNamnbytt => "konto namnbytt",
            Händelse::KontoAktiverat => "konto aktiverat",
            Händelse::KontoInaktiverat => "konto inaktiverat",
            Händelse::BasTillägg => "BAS-tillägg",
            Händelse::RäkenskapsårSkapat => "räkenskapsår skapat",
            Händelse::RäkenskapsårÄndrat => "räkenskapsår ändrat",
            Händelse::BokföringsmetodBytt => "bokföringsmetod bytt",
            Händelse::MomsperiodBytt => "momsperiod bytt",
            Händelse::VerifikatBokfört => "verifikat bokfört",
            Händelse::VerifikatRättat => "verifikat rättat",
            Händelse::UtkastRaderat => "utkast raderat",
            Händelse::IngåendeBalansÄndrad => "ingående balans ändrad",
            Händelse::UnderlagUppladdat => "underlag uppladdat",
            Händelse::UnderlagBorttaget => "underlag borttaget",
            Händelse::VerifikatKompletterat => "verifikat kompletterat",
            Händelse::ÅtkomstGiven => "åtkomst given",
            Händelse::ÅtkomstTagen => "åtkomst tagen",
            Händelse::AnvändareAvaktiverad => "användare avaktiverad",
            Händelse::InbjudanSkapad => "inbjudan skapad",
            Händelse::InbjudanAnvänd => "inbjudan använd",
            Händelse::Sie4Exporterad => "SIE4 exporterad",
            Händelse::ÅterställningGjord => "återställning gjord",
            Händelse::MigreringKörd => "migrering körd",
            Händelse::MigreringMisslyckad => "migrering misslyckad",
            Händelse::ProgramversionBytt => "programversion bytt",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Post {
    pub tid: Timestamp,
    pub aktör: Aktör,
    /// Användarens visningsnamn, när aktören är en användare.
    pub användarnamn: Option<String>,
    /// Händelsens typ, t.ex. `företag skapat`.
    pub typ: String,
    /// Det händelsen rör, t.ex. ett organisationsnummer eller ett kontonummer.
    pub objekt: String,
    /// Strukturerad beskrivning av ändringen som JSON.
    pub beskrivning: serde_json::Value,
}

/// Skriver en post i samma transaktion som ändringen. Användningsfallen går genom
/// [`atkomst::Ärende::logga`].
pub(crate) async fn skriv(
    conn: &mut SqliteConnection,
    företag: FöretagsId,
    tid: Timestamp,
    aktör: &Aktör,
    händelse: Händelse,
    objekt: &str,
    beskrivning: serde_json::Value,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO behandlingshistorik
             (foretag_id, tid, kanal, anvandare_id, typ, objekt, beskrivning)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(företag.0)
    .bind(tid.to_string())
    .bind(aktör.kanal.namn())
    .bind(aktör.användare.map(|a| a.0))
    .bind(händelse.namn())
    .bind(objekt)
    .bind(beskrivning.to_string())
    .execute(conn)
    .await?;
    Ok(())
}

/// Företagets behandlingshistorik i den ordning posterna skrevs.
pub async fn läs(
    db: &Databas,
    aktör: &Aktör,
    företag: FöretagsId,
) -> Result<Vec<Post>, ÅtkomstFel> {
    let mut tx = db.pool().begin().await?;
    atkomst::kräv(&mut tx, aktör, företag, Handling::LäsaFöretag).await?;
    let rader = sqlx::query_as(
        "SELECT h.tid, h.kanal, h.anvandare_id, u.visningsnamn, h.typ, h.objekt, h.beskrivning
         FROM behandlingshistorik h LEFT JOIN anvandare u ON u.id = h.anvandare_id
         WHERE h.foretag_id = ? ORDER BY h.id",
    )
    .bind(företag.0)
    .fetch_all(&mut *tx)
    .await?;
    Ok(poster(rader)?)
}

/// Ett urval ur behandlingshistoriken. `None` betyder alla.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Urval {
    /// Typen som den står i posten, t.ex. `konto namnbytt`.
    pub typ: Option<String>,
    pub aktör: Option<Aktörsval>,
    /// Första dagen, i Stockholm.
    pub från: Option<Date>,
    /// Sista dagen, i Stockholm.
    pub till: Option<Date>,
}

/// En aktör att välja i urvalet: en användare (i vilken kanal som helst), eller en kanal utan
/// användare (Maya självt, CLI:t).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Aktörsval {
    Användare(AnvändarId),
    Kanal(Kanal),
}

/// Nyckeln i en adress: `anvandare-<id>`, eller kanalens namn (`maya`, `cli`, `webb`).
impl std::fmt::Display for Aktörsval {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Aktörsval::Användare(a) => write!(f, "anvandare-{a}"),
            Aktörsval::Kanal(k) => f.write_str(k.namn()),
        }
    }
}

impl std::str::FromStr for Aktörsval {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, ()> {
        match s.strip_prefix("anvandare-") {
            Some(id) => id
                .parse()
                .map(|id| Aktörsval::Användare(AnvändarId(id)))
                .map_err(|_| ()),
            None => Kanal::från_namn(s).map(Aktörsval::Kanal).ok_or(()),
        }
    }
}

/// Ett urval ur behandlingshistoriken och det som går att välja i den.
#[derive(Debug, Clone, PartialEq)]
pub struct Historik {
    /// Posterna i urvalet, i den ordning de skrevs.
    pub poster: Vec<Post>,
    /// Typerna som finns i företagets historik, i bokstavsordning.
    pub typer: Vec<String>,
    /// Aktörerna som finns i företagets historik, med namn, i namnordning.
    pub aktörer: Vec<(Aktörsval, String)>,
}

/// Företagets behandlingshistorik i urvalet.
pub async fn historik(
    db: &Databas,
    aktör: &Aktör,
    företag: FöretagsId,
    urval: &Urval,
) -> Result<Historik, ÅtkomstFel> {
    let alla = läs(db, aktör, företag).await?;
    let mut typer: Vec<String> = alla.iter().map(|p| p.typ.clone()).collect();
    typer.sort();
    typer.dedup();
    let mut aktörer: Vec<(Aktörsval, String)> = Vec::new();
    for post in &alla {
        let val = post.aktörsval();
        if !aktörer.iter().any(|(a, _)| *a == val) {
            aktörer.push((val, post.aktörsnamn()));
        }
    }
    // Användarna i namnordning, sedan Maya och CLI:t.
    aktörer.sort_by_key(|(val, namn)| (matches!(val, Aktörsval::Kanal(_)), namn.to_lowercase()));
    let poster = alla.into_iter().filter(|p| urval.tar_med(p)).collect();
    Ok(Historik {
        poster,
        typer,
        aktörer,
    })
}

impl Urval {
    /// Urvalet som text, t.ex. `typ konto namnbytt · från 2026-10-01`. `aktörer` ger namnet på
    /// den valda aktören (se [`Historik::aktörer`]).
    pub fn beskriv(&self, aktörer: &[(Aktörsval, String)]) -> String {
        let mut delar = Vec::new();
        if let Some(typ) = &self.typ {
            delar.push(format!("typ {typ}"));
        }
        if let Some(val) = self.aktör {
            let namn = aktörer
                .iter()
                .find(|(a, _)| *a == val)
                .map_or_else(|| val.to_string(), |(_, n)| n.clone());
            delar.push(format!("aktör {namn}"));
        }
        if let Some(från) = self.från {
            delar.push(format!("från {från}"));
        }
        if let Some(till) = self.till {
            delar.push(format!("till {till}"));
        }
        if delar.is_empty() {
            "alla poster".into()
        } else {
            delar.join(" · ")
        }
    }

    /// Om posten är med i urvalet.
    pub fn tar_med(&self, post: &Post) -> bool {
        let dag = maya_domain::tid::datum(post.tid);
        self.typ.as_ref().is_none_or(|t| *t == post.typ)
            && self.aktör.is_none_or(|a| a == post.aktörsval())
            && self.från.is_none_or(|f| dag >= f)
            && self.till.is_none_or(|t| dag <= t)
    }
}

impl Post {
    /// Aktören som den kan väljas i ett urval.
    pub fn aktörsval(&self) -> Aktörsval {
        match self.aktör.användare {
            Some(a) => Aktörsval::Användare(a),
            None => Aktörsval::Kanal(self.aktör.kanal),
        }
    }

    /// Beloppet som posten gäller, för verifikat och utkast.
    pub fn belopp(&self) -> Option<Saldo> {
        self.beskrivning["summa_öre"].as_i64().map(Saldo::från_öre)
    }

    /// Beskrivningen av ändringen som text. En typ som saknar egen text (en som tillkommit
    /// efter den här versionen) visas med beskrivningens fält, så att inget döljs.
    pub fn text(&self) -> String {
        let b = &self.beskrivning;
        let s = |v: &Value| match v {
            Value::String(s) => s.clone(),
            Value::Null => String::new(),
            v => v.to_string(),
        };
        let f = |namn: &str| s(&b[namn]);
        let metod = |kod: &Value| {
            Bokföringsmetod::från_kod(&s(kod)).map_or_else(|| s(kod), |m| m.namn().to_lowercase())
        };
        let moms = |kod: &Value| {
            Momsperiod::från_kod(&s(kod)).map_or_else(|| s(kod), |m| m.namn().to_lowercase())
        };
        let kronor = |öre: &Value| Saldo::från_öre(öre.as_i64().unwrap_or_default()).to_string();
        match self.typ.as_str() {
            "företag skapat" => format!(
                "{} upplagt: räkenskapsår {} – {}, {}, momsperiod {}, kontoplan {}",
                f("namn"),
                s(&b["första_räkenskapsår"]["start"]),
                s(&b["första_räkenskapsår"]["slut"]),
                metod(&b["bokföringsmetod"]),
                moms(&b["momsperiod"]),
                f("kontoplan"),
            ),
            "företag ändrat" => b
                .as_object()
                .map(|fält| {
                    fält
                        .iter()
                        .map(|(namn, v)| {
                            format!("{namn}: ”{}” → ”{}”", s(&v["från"]), s(&v["till"]))
                        })
                        .collect::<Vec<_>>()
                        .join("; ")
                })
                .unwrap_or_default(),
            "räkenskapsår skapat" => {
                let mut skäl = f("skäl");
                if let Some(först) = skäl.chars().next() {
                    skäl.replace_range(..först.len_utf8(), &först.to_uppercase().to_string());
                }
                skäl
            }
            "räkenskapsår ändrat" => format!(
                "Slutet ändrat från {} till {}",
                s(&b["slut"]["från"]),
                s(&b["slut"]["till"])
            ),
            "bokföringsmetod bytt" => format!(
                "Från {} till {} fr.o.m. {}",
                metod(&b["från"]),
                metod(&b["till"]),
                f("gäller_från")
            ),
            "momsperiod bytt" => format!(
                "Från {} till {} fr.o.m. {}",
                moms(&b["från"]),
                moms(&b["till"]),
                f("gäller_från")
            ),
            "ingående balans ändrad" => b["ändringar"]
                .as_array()
                .map(|ä| {
                    ä.iter()
                        .map(|r| {
                            format!(
                                "{}: {} → {}",
                                s(&r["konto"]),
                                kronor(&r["före_öre"]),
                                kronor(&r["efter_öre"])
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("; ")
                })
                .unwrap_or_default(),
            "konto skapat" => format!("Eget konto ”{}”", f("namn")),
            "konto namnbytt" => {
                format!("Namn ändrat från ”{}” till ”{}”", f("från"), f("till"))
            }
            "konto aktiverat" => "Aktiverat".into(),
            "konto inaktiverat" => "Inaktiverat".into(),
            "BAS-tillägg" => {
                let konton: Vec<String> = b["konton"]
                    .as_array()
                    .map(|k| k.iter().map(s).collect())
                    .unwrap_or_default();
                let antal = if konton.len() == 1 {
                    "1 konto tillagt som inaktivt".to_string()
                } else {
                    format!("{} konton tillagda som inaktiva", konton.len())
                };
                format!("{antal}: {}", konton.join(", "))
            }
            "verifikat bokfört" => format!(
                "{} ”{}”, {} rader",
                f("datum"),
                f("beskrivning"),
                b["rader"].as_array().map_or(0, Vec::len)
            ),
            "utkast raderat" => format!(
                "Utkast daterat {}, {} rader",
                f("datum"),
                s(&b["antal_rader"])
            ),
            "underlag uppladdat" => format!(
                "”{}” uppladdat, {}, {} byte",
                f("filnamn"),
                f("mime"),
                s(&b["storlek"])
            ),
            "underlag borttaget" => format!(
                "”{}” borttaget, {}, {} byte",
                f("filnamn"),
                f("mime"),
                s(&b["storlek"])
            ),
            "verifikat rättat" => {
                let mut text = format!(
                    "Rättat av {} daterad {}",
                    s(&b["rättelse"]["nummer"]),
                    f("datum")
                );
                if !f("orsak").is_empty() {
                    text.push_str(&format!(", orsak ”{}”", f("orsak")));
                }
                if !b["kopia"].is_null() {
                    text.push_str(", registreras om som utkast");
                }
                text
            }
            "verifikat kompletterat" => {
                format!("Kompletterat med underlaget ”{}”", f("filnamn"))
            }
            "åtkomst given" => format!("Åtkomst till företaget för {}", f("epost")),
            "åtkomst tagen" => format!("Åtkomsten till företaget borttagen för {}", f("epost")),
            "användare avaktiverad" => format!(
                "{} avaktiverad: kan inte logga in, åtkomsten står kvar",
                f("epost")
            ),
            "inbjudan skapad" => {
                let giltig = f("giltig_till");
                let giltig = giltig.parse::<Timestamp>().map_or(giltig, |t| {
                    t.to_zoned(maya_domain::tid::stockholm())
                        .strftime("%Y-%m-%d %H:%M")
                        .to_string()
                });
                format!("Inbjudan till {}, giltig till {giltig}", f("epost"))
            }
            "inbjudan använd" => format!("{} registrerade sig med inbjudan", f("epost")),
            "SIE4 exporterad" => {
                format!("{}, {} verifikat", f("filnamn"), s(&b["antal_verifikat"]))
            }
            "återställning gjord" => {
                let backup_tid = f("backup_tid");
                let backup_tid = backup_tid.parse::<Timestamp>().map_or(backup_tid, |t| {
                    t.to_zoned(maya_domain::tid::stockholm())
                        .strftime("%Y-%m-%d %H:%M")
                        .to_string()
                });
                format!(
                    "Hela installationen återställd från backupen {backup_tid}. Det som \
                     registrerades efter den måste registreras om"
                )
            }
            "migrering körd" | "migrering misslyckad" => {
                let migreringar: Vec<String> = b["migreringar"]
                    .as_array()
                    .map(|m| {
                        m.iter()
                            .map(|m| format!("{} {}", s(&m["version"]), s(&m["namn"])))
                            .collect()
                    })
                    .unwrap_or_default();
                let antal = match migreringar.len() {
                    1 => "1 migrering".to_string(),
                    n => format!("{n} migreringar"),
                };
                let backup = match &b["backup"] {
                    Value::Null => String::new(),
                    v => format!(", backup {}", s(v)),
                };
                let utfall = if self.typ == "migrering körd" {
                    format!("kontrollsummorna {}", f("kontrollsummor"))
                } else {
                    format!(
                        "misslyckades och rullades tillbaka, databasen är som före: {}",
                        f("fel")
                    )
                };
                format!(
                    "{antal} av Maya {} ({}): {utfall}{backup}",
                    f("programversion"),
                    migreringar.join(", "),
                )
            }
            "programversion bytt" => {
                let från = match &b["från"] {
                    Value::Null => "okänd version".to_string(),
                    v => s(v),
                };
                format!("Från {från} till {}", f("till"))
            }
            _ => fält_som_text(b),
        }
    }

    /// Aktören som den visas: `Maya`, användarens namn, eller kanalen (`cli`).
    pub fn aktörsnamn(&self) -> String {
        match (self.aktör.kanal, &self.användarnamn) {
            (Kanal::Maya, _) => "Maya".into(),
            (_, Some(namn)) => namn.clone(),
            (kanal, None) => kanal.namn().into(),
        }
    }
}

/// En beskrivning utan egen text: `nyckel: värde` för varje fält, nästlade med punkt.
fn fält_som_text(v: &Value) -> String {
    fn platta(prefix: &str, v: &Value, ut: &mut Vec<String>) {
        match v {
            Value::Object(fält) => {
                for (k, v) in fält {
                    let namn = if prefix.is_empty() {
                        k.clone()
                    } else {
                        format!("{prefix}.{k}")
                    };
                    platta(&namn, v, ut);
                }
            }
            Value::String(s) => ut.push(format!("{prefix}: {s}")),
            v => ut.push(format!("{prefix}: {v}")),
        }
    }
    let mut ut = Vec::new();
    platta("", v, &mut ut);
    ut.join("; ")
}

/// Posterna som rör ett konto: när det kom till (företaget skapat med en kopia av BAS, eller ett
/// BAS-tillägg) och varje ändring av det.
pub(crate) async fn läs_konto(
    conn: &mut SqliteConnection,
    företag: FöretagsId,
    nummer: u16,
) -> Result<Vec<Post>, DatabasFel> {
    let rader = sqlx::query_as(
        "SELECT h.tid, h.kanal, h.anvandare_id, u.visningsnamn, h.typ, h.objekt, h.beskrivning
         FROM behandlingshistorik h
         LEFT JOIN anvandare u ON u.id = h.anvandare_id
         JOIN konto k ON k.foretag_id = h.foretag_id AND k.nummer = ?2
         WHERE h.foretag_id = ?1
           AND ((h.typ LIKE 'konto %' AND h.objekt = CAST(?2 AS TEXT))
             OR (h.typ = 'BAS-tillägg'
                 AND ?2 IN (SELECT value FROM json_each(h.beskrivning, '$.konton')))
             OR (h.typ = 'företag skapat' AND k.ursprung = 'bas' AND k.bas_tillagg = 0))
         ORDER BY h.id",
    )
    .bind(företag.0)
    .bind(nummer)
    .fetch_all(conn)
    .await?;
    poster(rader)
}

/// Posterna som rör ett verifikat, i ordning: de vars beskrivning har `"verifikat": id`.
pub(crate) async fn läs_om_verifikat(
    conn: &mut SqliteConnection,
    företag: FöretagsId,
    verifikat: i64,
) -> Result<Vec<Post>, DatabasFel> {
    let rader = sqlx::query_as(
        "SELECT h.tid, h.kanal, h.anvandare_id, u.visningsnamn, h.typ, h.objekt, h.beskrivning
         FROM behandlingshistorik h LEFT JOIN anvandare u ON u.id = h.anvandare_id
         WHERE h.foretag_id = ? AND h.beskrivning ->> '$.verifikat' = ? ORDER BY h.id",
    )
    .bind(företag.0)
    .bind(verifikat)
    .fetch_all(conn)
    .await?;
    poster(rader)
}

type Rad = (
    String,
    String,
    Option<i64>,
    Option<String>,
    String,
    String,
    String,
);

fn poster(rader: Vec<Rad>) -> Result<Vec<Post>, DatabasFel> {
    let fel = |vad: &str| DatabasFel::Korrupt(format!("behandlingshistoriken: {vad}"));
    rader
        .into_iter()
        .map(
            |(tid, kanal, användare, användarnamn, typ, objekt, beskrivning)| {
                Ok(Post {
                    tid: tid.parse().map_err(|_| fel("tid"))?,
                    aktör: Aktör {
                        kanal: Kanal::från_namn(&kanal).ok_or_else(|| fel("kanal"))?,
                        användare: användare.map(AnvändarId),
                    },
                    användarnamn,
                    typ,
                    objekt,
                    beskrivning: serde_json::from_str(&beskrivning)
                        .map_err(|_| fel("beskrivning"))?,
                })
            },
        )
        .collect()
}
