//! Leverantörsfakturor (fakturametoden): utkast med originalet kopplat, och registreringen som
//! låser fakturan och bokför den i samma transaktion.
//!
//! Ett utkast skapas ur ett okopplat underlag ("Vad är det här?" → Leverantörsfaktura) eller
//! med en uppladdning i fliken Leverantörsfakturor, och hör sedan till fakturan, så att det
//! inte längre är okopplat. Utkastet kan ändras och raderas fritt; en radering loggas med en
//! sammanfattning, och originalet blir okopplat igen. En ny leverantör på utkastet har status
//! `ny`, blir verklig när fakturan registreras och raderas med Maya som aktör när inget utkast
//! längre hänvisar till den.
//!
//! *Registrera och bokför* prövar spärrarna i `maya_domain::leverantorsfaktura` och bokför
//! kostnaden per rad, 2641 och 2440 daterat på fakturadatumet med originalet som underlag (VF5),
//! i samma transaktion som fakturan låses (L1). Verifikatet länkar till fakturan, och fakturan
//! till verifikatet. Under kontantmetoden på fakturadatumet bokförs inget (*Registrera*, #149):
//! skulden är inte bokförd, och betalningen bokför kostnaden och momsen.
//!
//! Omvänd skattskyldighet (#146) sparas i utkastet när användaren väljer den; annars föreslås
//! leverantörens förval. Registreringen sparar det som bokfördes, och bokför den fiktiva momsen
//! enligt `maya_domain::leverantorsfaktura`.
//!
//! **Kreditfaktura från leverantören** (#148) registreras som en faktura, med negativt
//! totalbelopp och en frivillig hänvisning (`krediterar`) till originalet. Verifikatet är
//! registreringens med omvänt tecken, och historiken får "kreditfaktura registrerad". Det som
//! återstår av en leverantörsfaktura räknas i vyn `leverantorsfaktura_i_reskontran`: totalen minus
//! de registrerade kreditfakturorna som hänvisar till den och betalningarna.
//!
//! **Rätta avskrift…** (#148) vänder registreringens verifikat med en rättelse daterad i dag
//! (R1), låser upp fakturan (status `upplast`) och behåller betalningarna; orsaken är valfri.
//! En upplåst faktura skrivs av och sparas som ett utkast, men raderas inte och tar inte emot
//! betalningar. Den registreras om med samma regler och ett nytt verifikat, och totalen får inte
//! bli mindre än det som redan har betalats.

use std::fmt;
use std::str::FromStr;

use jiff::civil::Date;
use maya_domain::belopp::Saldo;
use maya_domain::kontoplan::Kontonummer;
use maya_domain::leverantor::OmvändSkattskyldighet;
use maya_domain::leverantorsfaktura::{
    Förutsättningar, Krediterbar, Leverantörsval, NY, NyLeverantörsfaktura, NyRad, Rad,
    Registrerad, Spärr,
};
use maya_domain::verifikat::{NyRad as NyVerifikatrad, NyttVerifikat};
use serde_json::json;
use sqlx::SqliteConnection;

use crate::aktor::{Aktör, Handling};
use crate::atkomst::{self, Ärende, ÅtkomstFel};
use crate::behandlingshistorik::{self, Händelse, Post};
use crate::betalning::Faktura;
use crate::foretag::{self, FöretagsId, datum};
use crate::leverantor::{self, LeverantörsId};
use crate::underlag::{self, UnderlagFel, UnderlagId, Uppladdat};
use crate::verifikat::{
    self, Bokfört, Länk, Signatur, VerifikatFel, VerifikatId, Verifikationsnummer,
};
use crate::{Databas, DatabasFel, Klocka};

/// En leverantörsfakturas id i installationen. Vem som helst kan skriva ett id; `core` avgör om
/// aktören har åtkomst till fakturans företag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LeverantörsfakturaId(pub(crate) i64);

impl fmt::Display for LeverantörsfakturaId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for LeverantörsfakturaId {
    type Err = std::num::ParseIntError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse().map(LeverantörsfakturaId)
    }
}

/// Var fakturan är i sin livscykel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Utkast,
    Registrerad,
    /// Registreringen är vänd med Rätta avskrift, och fakturan registreras om (#148).
    Upplåst,
}

#[derive(Debug, thiserror::Error)]
pub enum FakturaFel {
    /// Spärrarna i `domain`, med samma koder som formuläret visar.
    #[error("fakturan kan inte registreras")]
    Spärrat(Vec<Spärr>),
    /// Inget utkast med det id:t i företaget: det finns inte, eller är redan registrerat.
    #[error("utkastet finns inte")]
    IngetUtkast,
    /// Underlaget finns inte i företaget, eller hör redan till ett verifikat eller en faktura.
    #[error("underlaget finns inte eller är inte okopplat")]
    InteOkopplat,
    /// Rätta avskrift gäller en registrerad faktura i företaget (#148).
    #[error("fakturan är inte registrerad")]
    InteRegistrerad,
    /// Registreringens verifikat kunde inte rättas (#148), t.ex. i ett räkenskapsår som inte finns.
    #[error(transparent)]
    Rättelse(VerifikatFel),
    #[error(transparent)]
    Underlag(UnderlagFel),
    #[error(transparent)]
    Åtkomst(#[from] ÅtkomstFel),
}

impl From<sqlx::Error> for FakturaFel {
    fn from(e: sqlx::Error) -> Self {
        FakturaFel::Åtkomst(e.into())
    }
}

impl From<DatabasFel> for FakturaFel {
    fn from(e: DatabasFel) -> Self {
        FakturaFel::Åtkomst(e.into())
    }
}

impl From<UnderlagFel> for FakturaFel {
    fn from(e: UnderlagFel) -> Self {
        match e {
            UnderlagFel::Åtkomst(e) => FakturaFel::Åtkomst(e),
            e => FakturaFel::Underlag(e),
        }
    }
}

impl From<VerifikatFel> for FakturaFel {
    fn from(e: VerifikatFel) -> Self {
        match e {
            VerifikatFel::Åtkomst(e) => FakturaFel::Åtkomst(e),
            // Leverantörsfakturans spärrar täcker verifikatets; det här är ett fel i Maya.
            e => FakturaFel::Åtkomst(DatabasFel::Korrupt(format!("registreringen: {e}")).into()),
        }
    }
}

/// En leverantörsfaktura med sitt original och sin historik.
#[derive(Debug, Clone, PartialEq)]
pub struct Leverantörsfaktura {
    pub id: LeverantörsfakturaId,
    pub status: Status,
    /// Originalet.
    pub underlag: UnderlagId,
    pub filnamn: String,
    pub leverantör: Option<LeverantörsId>,
    /// Leverantörens namn, tomt utan leverantör.
    pub leverantörsnamn: String,
    /// Leverantören är ny och läggs i registret när fakturan registreras.
    pub ny_leverantör: bool,
    /// Fakturan som formulär, så som den sparades.
    pub formulär: NyLeverantörsfaktura,
    pub fakturadatum: Option<Date>,
    pub förfallodag: Option<Date>,
    /// I öre; negativt för en kreditfaktura.
    pub total: Option<i64>,
    /// Registreringens verifikat; efter Rätta avskrift den senaste registreringens.
    pub verifikat: Option<(VerifikatId, Verifikationsnummer)>,
    pub skapad: Signatur,
    pub registrerad: Option<Signatur>,
    /// Posterna i behandlingshistoriken som rör fakturan, i ordning.
    pub historik: Vec<Post>,
    /// Originalet som kreditfakturan hänvisar till (#148).
    pub krediterar: Option<Kreditlänk>,
    /// De kreditfakturor som hänvisar till fakturan (#148).
    pub krediteras_av: Vec<Kreditlänk>,
    /// Rätta avskrift, i ordning (#148).
    pub avskrifter: Vec<Avskriftsrättelse>,
}

/// En faktura i en hänvisning mellan en kreditfaktura och dess original.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Kreditlänk {
    pub id: LeverantörsfakturaId,
    pub fakturanummer: String,
    /// I öre.
    pub total: i64,
    pub status: Status,
}

/// En upplåsning med Rätta avskrift: registreringens verifikat och rättelsen som vände det.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Avskriftsrättelse {
    pub verifikat: Option<Länk>,
    pub rättelse: Option<Länk>,
    pub orsak: String,
    pub rättad: Signatur,
}

/// En faktura i listan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fakturarad {
    pub id: LeverantörsfakturaId,
    pub status: Status,
    pub leverantörsnamn: String,
    pub ny_leverantör: bool,
    pub fakturanummer: String,
    pub fakturadatum: Option<Date>,
    pub förfallodag: Option<Date>,
    /// I öre; negativt för en kreditfaktura.
    pub total: Option<i64>,
    /// Det som återstår att betala i öre (#147): totalen minus kreditfakturorna och
    /// betalningarna i reskontran (#148); negativt är ett tillgodohavande. För ett utkast
    /// totalen, eller 0 utan total.
    pub återstår: i64,
    /// Det som registrerade kreditfakturor har krediterat, i öre och positivt (#148).
    pub krediterat: i64,
    /// Fakturan är en kreditfaktura som hänvisar till ett original och räknas av mot det.
    pub kreditfaktura: bool,
}

/// Skapar ett utkast med det okopplade underlaget `underlag` som original.
pub async fn skapa_utkast(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    företag: FöretagsId,
    underlag: UnderlagId,
) -> Result<LeverantörsfakturaId, FakturaFel> {
    let mut ärende =
        Ärende::börja(db, klocka, aktör, företag, Handling::HanteraFakturautkast).await?;
    let okopplat: bool = sqlx::query_scalar(
        "SELECT EXISTS (
             SELECT 1 FROM underlag u WHERE u.foretag_id = ? AND u.id = ?
               AND NOT EXISTS (SELECT 1 FROM underlagskoppling k WHERE k.underlag_id = u.id)
               AND NOT EXISTS (SELECT 1 FROM leverantorsfaktura f WHERE f.underlag_id = u.id)
               AND NOT EXISTS (SELECT 1 FROM kundfaktura f WHERE f.underlag_id = u.id))",
    )
    .bind(företag.0)
    .bind(underlag.0)
    .fetch_one(ärende.conn())
    .await?;
    if !okopplat {
        return Err(FakturaFel::InteOkopplat);
    }
    let id = skapa_i(&mut ärende, underlag).await?;
    ärende.slutför().await?;
    Ok(id)
}

/// Laddar upp originalet och skapar utkastet i en transaktion.
pub async fn ladda_upp(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    företag: FöretagsId,
    fil: (&str, &str, &[u8]),
) -> Result<(LeverantörsfakturaId, Uppladdat), FakturaFel> {
    let mut ärende =
        Ärende::börja(db, klocka, aktör, företag, Handling::HanteraFakturautkast).await?;
    atkomst::kräv(ärende.conn(), aktör, företag, Handling::HanteraUnderlag).await?;
    let (filnamn, mime, innehåll) = fil;
    let uppladdat = underlag::ladda_upp_i(&mut ärende, filnamn, mime, innehåll).await?;
    let id = skapa_i(&mut ärende, uppladdat.id).await?;
    ärende.slutför().await?;
    Ok((id, uppladdat))
}

async fn skapa_i(
    ärende: &mut Ärende,
    underlag: UnderlagId,
) -> Result<LeverantörsfakturaId, sqlx::Error> {
    let (företag, aktör, nu) = (ärende.företag, ärende.aktör, ärende.nu);
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO leverantorsfaktura
             (foretag_id, status, underlag_id, skapad, skapad_kanal, skapad_av)
         VALUES (?, 'utkast', ?, ?, ?, ?) RETURNING id",
    )
    .bind(företag.0)
    .bind(underlag.0)
    .bind(nu.to_string())
    .bind(aktör.kanal.namn())
    .bind(aktör.användare.map(|a| a.0))
    .fetch_one(ärende.conn())
    .await?;
    Ok(LeverantörsfakturaId(id))
}

/// Sparar formuläret i utkastet. Ett utkast får vara ofullständigt; bara det som inte går att
/// lagra spärrar (datum, belopp, kontonummer och momssatser som inte går att tolka, och en ny
/// leverantör som inte klarar registrets regler). En ny leverantör sparas med utkastet, och en
/// ny leverantör som inget utkast längre hänvisar till raderas.
pub async fn spara_utkast(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    företag: FöretagsId,
    id: LeverantörsfakturaId,
    formulär: &NyLeverantörsfaktura,
) -> Result<(), FakturaFel> {
    let mut ärende =
        Ärende::börja(db, klocka, aktör, företag, Handling::HanteraFakturautkast).await?;
    spara_i(&mut ärende, id, formulär).await?;
    leverantor::rensa_nya_i(&mut ärende).await?;
    ärende.slutför().await?;
    Ok(())
}

async fn spara_i(
    ärende: &mut Ärende,
    id: LeverantörsfakturaId,
    formulär: &NyLeverantörsfaktura,
) -> Result<(), FakturaFel> {
    let företag = ärende.företag;
    let (nuvarande, _) = kräv_avskrift(ärende, id).await?;
    let u = formulär.som_utkast().map_err(FakturaFel::Spärrat)?;
    // Hänvisningen sparas när den pekar på en annan faktura i företaget; spärrarna vid
    // registreringen prövar resten.
    let krediterar = match u.krediterar {
        Some(k) => {
            sqlx::query_scalar::<_, i64>(
                "SELECT id FROM leverantorsfaktura WHERE id = ? AND foretag_id = ? AND id <> ?",
            )
            .bind(k)
            .bind(företag.0)
            .bind(id.0)
            .fetch_optional(ärende.conn())
            .await?
        }
        None => None,
    };
    let leverantör = match formulär.leverantör.trim() {
        NY if !formulär.ny_leverantör.namn.trim().is_empty() => {
            let konton = läs_konton(ärende.conn(), företag).await?;
            let l = formulär.ny_leverantör.kontrollera(&konton).map_err(|fel| {
                FakturaFel::Spärrat(
                    fel.into_iter()
                        .map(|f| Spärr {
                            fält: maya_domain::leverantorsfaktura::Fält::NyLeverantör(f.fält),
                            kod: "ny_leverantor",
                            text: f.text,
                            åtgärd: None,
                        })
                        .collect(),
                )
            })?;
            let ny = match nuvarande {
                Some(l) if leverantor::ny_i(ärende.conn(), företag, l).await?.is_some() => {
                    Some(l)
                }
                _ => None,
            };
            Some(leverantor::spara_ny_i(ärende, ny, &l).await?)
        }
        text => match text.parse::<LeverantörsId>() {
            Ok(l) if aktiv_leverantör(ärende.conn(), företag, l).await? => Some(l),
            _ => None,
        },
    };
    sqlx::query(
        "UPDATE leverantorsfaktura SET leverantor_id = ?, fakturanummer = ?, fakturadatum = ?,
             forfallodag = ?, referens = ?, total = ?, originalvaluta = ?,
             omvand_skattskyldighet = ?, krediterar = ?
         WHERE id = ? AND status IN ('utkast', 'upplast')",
    )
    .bind(leverantör.map(|l| l.0))
    .bind(&u.fakturanummer)
    .bind(u.fakturadatum.map(|d| d.to_string()))
    .bind(u.förfallodag.map(|d| d.to_string()))
    .bind(&u.referens)
    .bind(u.total)
    .bind(&u.originalvaluta)
    .bind(u.omvänd_skattskyldighet.map(|o| o.kod()))
    .bind(krediterar)
    .bind(id.0)
    .execute(ärende.conn())
    .await?;
    sqlx::query("DELETE FROM leverantorsfakturarad WHERE faktura_id = ?")
        .bind(id.0)
        .execute(ärende.conn())
        .await?;
    for (i, r) in u.rader.iter().enumerate() {
        sqlx::query(
            "INSERT INTO leverantorsfakturarad (faktura_id, radnr, konto, netto, momssats, moms)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(id.0)
        .bind(i as i64 + 1)
        .bind(r.konto.map(Kontonummer::get))
        .bind(r.netto)
        .bind(i64::from(r.momssats))
        .bind(r.moms)
        .execute(ärende.conn())
        .await?;
    }
    Ok(())
}

/// Resultatet av en registrering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Registrerat {
    /// Registreringens verifikat; `None` när inget bokfördes (kontantmetoden, #149).
    pub verifikat: Option<Bokfört>,
}

/// Registrerar och bokför utkastet med innehållet i formuläret, i en transaktion: prövar
/// spärrarna, gör en ny leverantör verklig, bokför registreringens verifikat daterat på
/// fakturadatumet med originalet som underlag, låser fakturan och loggar registreringen.
pub async fn registrera(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    företag: FöretagsId,
    id: LeverantörsfakturaId,
    formulär: &NyLeverantörsfaktura,
) -> Result<Registrerat, FakturaFel> {
    let mut ärende = Ärende::börja(db, klocka, aktör, företag, Handling::RegistreraFaktura).await?;
    let (_, status) = kräv_avskrift(&mut ärende, id).await?;
    let förutsättningar = läs_förutsättningar(ärende.conn(), företag, Some(id)).await?;
    let r = formulär
        .kontrollera(&förutsättningar)
        .map_err(FakturaFel::Spärrat)?;
    // Det som registreras sparas först i utkastet, med de spärrar som gäller för det.
    spara_i(&mut ärende, id, formulär).await?;
    let leverantör = LeverantörsId(
        sqlx::query_scalar::<_, Option<i64>>(
            "SELECT leverantor_id FROM leverantorsfaktura WHERE id = ?",
        )
        .bind(id.0)
        .fetch_one(ärende.conn())
        .await?
        .ok_or(FakturaFel::IngetUtkast)?,
    );
    let leverantörsnamn = match &r.ny_leverantör {
        Some(ny) => {
            leverantor::gör_verklig_i(&mut ärende, leverantör, ny).await?;
            ny.namn.clone()
        }
        None => {
            sqlx::query_scalar("SELECT namn FROM leverantor WHERE id = ?")
                .bind(leverantör.0)
                .fetch_one(ärende.conn())
                .await?
        }
    };

    // Raderna med det momsbelopp som bokförs, så att fakturan visar det som registrerades.
    for (i, rad) in r.rader.iter().enumerate() {
        sqlx::query("UPDATE leverantorsfakturarad SET konto = ?, moms = ? WHERE faktura_id = ? AND radnr = ?")
            .bind(rad.konto.get())
            .bind(rad.moms)
            .bind(id.0)
            .bind(i as i64 + 1)
            .execute(ärende.conn())
            .await?;
    }

    let underlag: UnderlagId = UnderlagId(
        sqlx::query_scalar("SELECT underlag_id FROM leverantorsfaktura WHERE id = ?")
            .bind(id.0)
            .fetch_one(ärende.conn())
            .await?,
    );
    let verifikatformulär = NyttVerifikat {
        datum: r.fakturadatum.to_string(),
        beskrivning: format!(
            "{} {}, {leverantörsnamn}",
            if r.total < 0 {
                "Kreditfaktura"
            } else {
                "Leverantörsfaktura"
            },
            r.fakturanummer
        ),
        rader: r
            .kontering()
            .iter()
            .map(|rad| NyVerifikatrad {
                konto: rad.konto.get().to_string(),
                debet: rad.debet.eller_tomt(),
                kredit: rad.kredit.eller_tomt(),
                text: String::new(),
            })
            .collect(),
    };
    // Under fakturametoden på fakturadatumet bokför registreringen skulden; en kreditfaktura med
    // hänvisning följer originalets skuld, och en utan hänvisning bokförs alltid (#149).
    let original = r
        .krediterar
        .as_deref()
        .and_then(|k| k.parse().ok())
        .map(|k| Faktura::Leverantör(LeverantörsfakturaId(k)));
    let (kontering, skulden_bokförs) = match original {
        Some(o) => (
            crate::fordran::kreditens_kontering(
                ärende.conn(),
                företag,
                o,
                r.fakturadatum,
                r.total,
                r.kontering(),
            )
            .await?,
            false,
        ),
        None if r.total < 0
            || crate::fordran::fakturametoden_på(ärende.conn(), företag, r.fakturadatum)
                .await? =>
        {
            (Some(r.kontering()), true)
        }
        None => (None, false),
    };
    let bokfört = match kontering {
        Some(rader) => Some(
            verifikat::bokför_i_ärende(
                &mut ärende,
                &NyttVerifikat {
                    rader: rader
                        .iter()
                        .map(|rad| NyVerifikatrad {
                            konto: rad.konto.get().to_string(),
                            debet: rad.debet.eller_tomt(),
                            kredit: rad.kredit.eller_tomt(),
                            text: String::new(),
                        })
                        .collect(),
                    ..verifikatformulär
                },
                &[underlag],
            )
            .await?,
        ),
        None => None,
    };
    let (aktör_, nu) = (ärende.aktör, ärende.nu);
    sqlx::query(
        "UPDATE leverantorsfaktura SET status = 'registrerad', verifikat_id = ?, registrerad = ?,
             registrerad_kanal = ?, registrerad_av = ?, omvand_skattskyldighet = ?
         WHERE id = ? AND status IN ('utkast', 'upplast')",
    )
    .bind(bokfört.map(|b| b.id.0))
    .bind(nu.to_string())
    .bind(aktör_.kanal.namn())
    .bind(aktör_.användare.map(|a| a.0))
    .bind(r.omvänd_skattskyldighet.kod())
    .bind(id.0)
    .execute(ärende.conn())
    .await?;
    if let (Some(b), true) = (bokfört, skulden_bokförs) {
        crate::fordran::bokför_i(ärende.conn(), företag, Faktura::Leverantör(id), b.id).await?;
    }
    let filnamn: (String, String) =
        sqlx::query_as("SELECT filnamn, sha256 FROM underlag WHERE id = ?")
            .bind(underlag.0)
            .fetch_one(ärende.conn())
            .await?;
    // #148: hänvisningen till originalet, och om fakturan registrerades om efter Rätta avskrift.
    let krediterar = match &r.krediterar {
        Some(k) => {
            let nummer: String =
                sqlx::query_scalar("SELECT fakturanummer FROM leverantorsfaktura WHERE id = ?")
                    .bind(k.parse::<i64>().unwrap_or_default())
                    .fetch_one(ärende.conn())
                    .await?;
            json!({ "id": k.parse::<i64>().unwrap_or_default(), "fakturanummer": nummer })
        }
        None => serde_json::Value::Null,
    };
    let händelse = if r.total < 0 {
        Händelse::KreditfakturaRegistrerad
    } else {
        Händelse::LeverantörsfakturaRegistrerad
    };
    ärende
        .logga(
            händelse,
            &format!("{} {leverantörsnamn}", r.fakturanummer),
            json!({
                "leverantörsfaktura": id.0,
                "leverantör": leverantör.0,
                "leverantörsnamn": leverantörsnamn,
                "fakturanummer": r.fakturanummer,
                "fakturadatum": r.fakturadatum.to_string(),
                "förfallodag": r.förfallodag.to_string(),
                "referens": r.referens,
                "originalvaluta": r.originalvaluta,
                "omvänd_skattskyldighet": r.omvänd_skattskyldighet.kod(),
                "summa_öre": r.total,
                "krediterar": krediterar,
                "omregistrerad": status == Status::Upplåst,
                "verifikat": bokfört.map(|b| b.id.0),
                "nummer": bokfört.map(|b| b.nummer.to_string()),
                "underlag": { "underlag": underlag.0, "filnamn": filnamn.0, "sha256": filnamn.1 },
            }),
        )
        .await?;
    ärende.slutför().await?;
    Ok(Registrerat {
        verifikat: bokfört
    })
}

/// Raderar ett utkast och loggar raderingen med en sammanfattning. Originalet blir okopplat
/// igen, och en ny leverantör som bara hörde till utkastet raderas med Maya som aktör.
pub async fn radera_utkast(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    företag: FöretagsId,
    id: LeverantörsfakturaId,
) -> Result<(), FakturaFel> {
    let mut ärende =
        Ärende::börja(db, klocka, aktör, företag, Handling::HanteraFakturautkast).await?;
    kräv_utkast(&mut ärende, id).await?;
    let f = läs(ärende.conn(), företag, id)
        .await?
        .ok_or(FakturaFel::IngetUtkast)?;
    let antal = f.formulär.rader.len();
    sqlx::query("DELETE FROM leverantorsfakturarad WHERE faktura_id = ?")
        .bind(id.0)
        .execute(ärende.conn())
        .await?;
    sqlx::query("DELETE FROM leverantorsfaktura WHERE id = ? AND status = 'utkast'")
        .bind(id.0)
        .execute(ärende.conn())
        .await?;
    ärende
        .logga(
            Händelse::FakturautkastRaderat,
            if f.formulär.fakturanummer.is_empty() {
                "leverantörsfaktura utan nummer"
            } else {
                &f.formulär.fakturanummer
            },
            json!({
                "leverantörsfaktura": id.0,
                "sort": "leverantörsfaktura",
                "leverantörsnamn": f.leverantörsnamn,
                "fakturanummer": f.formulär.fakturanummer,
                "fakturadatum": f.fakturadatum.map(|d| d.to_string()),
                "summa_öre": f.total,
                "antal_rader": antal,
                "underlag": { "underlag": f.underlag.0, "filnamn": f.filnamn },
            }),
        )
        .await?;
    leverantor::rensa_nya_i(&mut ärende).await?;
    ärende.slutför().await?;
    Ok(())
}

/// Det som formuläret prövar spärrarna mot, för utkastet `utkast` (dess egen faktura räknas
/// inte som dubblett).
pub async fn förutsättningar(
    db: &Databas,
    aktör: &Aktör,
    företag: FöretagsId,
    utkast: Option<LeverantörsfakturaId>,
) -> Result<Förutsättningar, ÅtkomstFel> {
    let mut tx = db.pool().begin().await?;
    atkomst::kräv(&mut tx, aktör, företag, Handling::LäsaFöretag).await?;
    Ok(läs_förutsättningar(&mut tx, företag, utkast).await?)
}

async fn läs_förutsättningar(
    conn: &mut SqliteConnection,
    företag: FöretagsId,
    utkast: Option<LeverantörsfakturaId>,
) -> Result<Förutsättningar, DatabasFel> {
    let v = verifikat::läs_förutsättningar(&mut *conn, företag).await?;
    let f = foretag::läs_företag(&mut *conn, företag).await?;
    let leverantörer: Vec<(i64, String, Option<i64>, String)> = sqlx::query_as(
        "SELECT id, namn, standardkonto, omvand_skattskyldighet FROM leverantor
         WHERE foretag_id = ? AND status = 'aktiv' ORDER BY namn COLLATE NOCASE, id",
    )
    .bind(företag.0)
    .fetch_all(&mut *conn)
    .await?;
    let registrerade: Vec<(i64, i64, String, String, i64)> = sqlx::query_as(
        "SELECT id, leverantor_id, fakturanummer, fakturadatum, total FROM leverantorsfaktura
         WHERE foretag_id = ? AND status <> 'utkast' AND id IS NOT ?
           AND leverantor_id IS NOT NULL AND fakturadatum IS NOT NULL AND total IS NOT NULL",
    )
    .bind(företag.0)
    .bind(utkast.map(|u| u.0))
    .fetch_all(&mut *conn)
    .await?;
    // #148: fakturorna som en kreditfaktura kan hänvisa till, med raderna som registrerades.
    let originalen: Vec<(i64, i64, String, i64, Option<String>)> = sqlx::query_as(
        "SELECT id, leverantor_id, fakturanummer, total, omvand_skattskyldighet
         FROM leverantorsfaktura
         WHERE foretag_id = ? AND status = 'registrerad' AND krediterar IS NULL AND total > 0
           AND id IS NOT ?
         ORDER BY fakturadatum DESC, id DESC",
    )
    .bind(företag.0)
    .bind(utkast.map(|u| u.0))
    .fetch_all(&mut *conn)
    .await?;
    let mut krediterbara = Vec::new();
    for (id, leverantör, nummer, total, omvänd) in originalen {
        let rader: Vec<(i64, i64, i64, i64)> = sqlx::query_as(
            "SELECT konto, netto, momssats, moms FROM leverantorsfakturarad
             WHERE faktura_id = ? ORDER BY radnr",
        )
        .bind(id)
        .fetch_all(&mut *conn)
        .await?;
        krediterbara.push(Krediterbar {
            id: id.to_string(),
            leverantör: leverantör.to_string(),
            fakturanummer: nummer,
            total,
            omvänd_skattskyldighet: omvänd
                .as_deref()
                .and_then(OmvändSkattskyldighet::från_kod)
                .unwrap_or_default(),
            rader: rader
                .into_iter()
                .map(|(konto, netto, sats, moms)| {
                    Ok(Rad {
                        konto: u16::try_from(konto)
                            .ok()
                            .and_then(Kontonummer::new)
                            .ok_or_else(|| {
                                DatabasFel::Korrupt("leverantorsfakturarad.konto".into())
                            })?,
                        netto,
                        momssats: u8::try_from(sats).unwrap_or(25),
                        moms,
                    })
                })
                .collect::<Result<_, DatabasFel>>()?,
        });
    }
    // #148: det som redan har betalats av en upplåst faktura.
    let betalt: i64 = match utkast {
        Some(u) => sqlx::query_scalar(
            "SELECT coalesce((SELECT betalt FROM leverantorsfaktura_i_reskontran WHERE id = ?), 0)",
        )
        .bind(u.0)
        .fetch_one(&mut *conn)
        .await?,
        None => 0,
    };
    Ok(Förutsättningar {
        konton: v.konton,
        räkenskapsår: v.räkenskapsår,
        momsperioder: f.momsperioder,
        leverantörer: leverantörer
            .into_iter()
            .map(|(id, namn, konto, omvänd)| {
                Ok(Leverantörsval {
                    id: id.to_string(),
                    namn,
                    standardkonto: konto
                        .and_then(|k| u16::try_from(k).ok())
                        .and_then(Kontonummer::new),
                    omvänd_skattskyldighet: OmvändSkattskyldighet::från_kod(&omvänd).ok_or_else(
                        || DatabasFel::Korrupt("leverantor.omvand_skattskyldighet".into()),
                    )?,
                })
            })
            .collect::<Result<_, DatabasFel>>()?,
        registrerade: registrerade
            .into_iter()
            .map(|(id, leverantör, nummer, d, total)| {
                Ok(Registrerad {
                    id: id.to_string(),
                    leverantör: leverantör.to_string(),
                    fakturanummer: nummer,
                    fakturadatum: datum(&d)?,
                    total,
                })
            })
            .collect::<Result<_, DatabasFel>>()?,
        krediterbara,
        betalt,
    })
}

/// Företagets leverantörsfakturor: utkasten först, sedan de registrerade efter förfallodag.
pub async fn lista(
    db: &Databas,
    aktör: &Aktör,
    företag: FöretagsId,
) -> Result<Vec<Fakturarad>, ÅtkomstFel> {
    let mut tx = db.pool().begin().await?;
    atkomst::kräv(&mut tx, aktör, företag, Handling::LäsaFöretag).await?;
    #[derive(sqlx::FromRow)]
    struct Listrad {
        id: i64,
        status: String,
        namn: Option<String>,
        lstatus: Option<String>,
        fakturanummer: String,
        fakturadatum: Option<String>,
        forfallodag: Option<String>,
        total: Option<i64>,
        krediterar: Option<i64>,
        krediterat: Option<i64>,
        aterstar: Option<i64>,
    }
    // Det som återstår räknas i vyn `leverantorsfaktura_i_reskontran` (#148), samma som för
    // betalningarna; ett utkast har sin total kvar.
    let rader: Vec<Listrad> = sqlx::query_as(
        "SELECT f.id, f.status, l.namn, l.status AS lstatus, f.fakturanummer, f.fakturadatum,
                f.forfallodag, f.total, f.krediterar, r.krediterat, r.aterstar
         FROM leverantorsfaktura f LEFT JOIN leverantor l ON l.id = f.leverantor_id
         LEFT JOIN leverantorsfaktura_i_reskontran r ON r.id = f.id
         WHERE f.foretag_id = ?
         ORDER BY f.status <> 'utkast', f.forfallodag, f.id",
    )
    .bind(företag.0)
    .fetch_all(&mut *tx)
    .await?;
    Ok(rader
        .into_iter()
        .map(|r| {
            Ok(Fakturarad {
                id: LeverantörsfakturaId(r.id),
                status: status_(&r.status)?,
                leverantörsnamn: r.namn.unwrap_or_default(),
                ny_leverantör: r.lstatus.as_deref() == Some("ny"),
                fakturanummer: r.fakturanummer,
                fakturadatum: r.fakturadatum.as_deref().map(datum).transpose()?,
                förfallodag: r.forfallodag.as_deref().map(datum).transpose()?,
                total: r.total,
                återstår: r.aterstar.unwrap_or(r.total.unwrap_or_default()),
                krediterat: r.krediterat.unwrap_or_default(),
                kreditfaktura: r.krediterar.is_some() && r.status != "utkast",
            })
        })
        .collect::<Result<_, DatabasFel>>()?)
}

/// En leverantörsfaktura i företaget, eller `None` om den inte finns där.
pub async fn fakturan(
    db: &Databas,
    aktör: &Aktör,
    företag: FöretagsId,
    id: LeverantörsfakturaId,
) -> Result<Option<Leverantörsfaktura>, ÅtkomstFel> {
    let mut tx = db.pool().begin().await?;
    atkomst::kräv(&mut tx, aktör, företag, Handling::LäsaFöretag).await?;
    Ok(läs(&mut tx, företag, id).await?)
}

fn status_(kod: &str) -> Result<Status, DatabasFel> {
    match kod {
        "utkast" => Ok(Status::Utkast),
        "registrerad" => Ok(Status::Registrerad),
        "upplast" => Ok(Status::Upplåst),
        _ => Err(DatabasFel::Korrupt("leverantorsfaktura.status".into())),
    }
}

#[derive(sqlx::FromRow)]
struct FakturaRad {
    id: i64,
    status: String,
    underlag_id: i64,
    filnamn: String,
    leverantor_id: Option<i64>,
    leverantorsnamn: Option<String>,
    leverantorsstatus: Option<String>,
    fakturanummer: String,
    fakturadatum: Option<String>,
    forfallodag: Option<String>,
    referens: String,
    total: Option<i64>,
    originalvaluta: String,
    omvand_skattskyldighet: Option<String>,
    krediterar: Option<i64>,
    verifikat_id: Option<i64>,
    verifikatnummer: Option<i64>,
    skapad: String,
    skapad_kanal: String,
    skapad_av: Option<i64>,
    skapad_namn: Option<String>,
    registrerad: Option<String>,
    registrerad_kanal: Option<String>,
    registrerad_av: Option<i64>,
    registrerad_namn: Option<String>,
}

async fn läs(
    conn: &mut SqliteConnection,
    företag: FöretagsId,
    id: LeverantörsfakturaId,
) -> Result<Option<Leverantörsfaktura>, DatabasFel> {
    let Some(f): Option<FakturaRad> = sqlx::query_as(
        "SELECT f.id, f.status, f.underlag_id, u.filnamn, f.leverantor_id,
                l.namn AS leverantorsnamn, l.status AS leverantorsstatus, f.fakturanummer,
                f.fakturadatum, f.forfallodag, f.referens, f.total, f.originalvaluta,
                f.omvand_skattskyldighet, f.krediterar,
                f.verifikat_id, v.nummer AS verifikatnummer,
                f.skapad, f.skapad_kanal, f.skapad_av, s.visningsnamn AS skapad_namn,
                f.registrerad, f.registrerad_kanal, f.registrerad_av,
                r.visningsnamn AS registrerad_namn
         FROM leverantorsfaktura f
         JOIN underlag u ON u.id = f.underlag_id
         LEFT JOIN leverantor l ON l.id = f.leverantor_id
         LEFT JOIN verifikat v ON v.id = f.verifikat_id
         LEFT JOIN anvandare s ON s.id = f.skapad_av
         LEFT JOIN anvandare r ON r.id = f.registrerad_av
         WHERE f.foretag_id = ? AND f.id = ?",
    )
    .bind(företag.0)
    .bind(id.0)
    .fetch_optional(&mut *conn)
    .await?
    else {
        return Ok(None);
    };
    type Radrad = (Option<i64>, Option<i64>, i64, Option<i64>);
    let rader: Vec<Radrad> = sqlx::query_as(
        "SELECT konto, netto, momssats, moms FROM leverantorsfakturarad
         WHERE faktura_id = ? ORDER BY radnr",
    )
    .bind(id.0)
    .fetch_all(&mut *conn)
    .await?;
    let ny = f.leverantorsstatus.as_deref() == Some("ny");
    let ny_leverantör = match (ny, f.leverantor_id) {
        (true, Some(l)) => leverantor::ny_i(&mut *conn, företag, LeverantörsId(l))
            .await?
            .map(|l| l.som_formulär())
            .unwrap_or_default(),
        _ => Default::default(),
    };
    let belopp = |öre: Option<i64>| {
        öre.map(|ö| Saldo::från_öre(ö).to_string())
            .unwrap_or_default()
    };
    let formulär = NyLeverantörsfaktura {
        leverantör: match (ny, f.leverantor_id) {
            (true, Some(_)) => NY.to_string(),
            (false, Some(l)) => l.to_string(),
            (_, None) => String::new(),
        },
        ny_leverantör,
        fakturanummer: f.fakturanummer.clone(),
        fakturadatum: f.fakturadatum.clone().unwrap_or_default(),
        förfallodag: f.forfallodag.clone().unwrap_or_default(),
        referens: f.referens,
        totalbelopp: belopp(f.total),
        omvänd_skattskyldighet: match (f.omvand_skattskyldighet, f.status.as_str()) {
            (Some(kod), _) => kod,
            // Registrerad före #146: ingen omvänd skattskyldighet.
            (None, "utkast") => String::new(),
            (None, _) => OmvändSkattskyldighet::Ingen.kod().into(),
        },
        originalvaluta: f.originalvaluta,
        krediterar: f.krediterar.map(|k| k.to_string()).unwrap_or_default(),
        rader: rader
            .into_iter()
            .map(|(konto, netto, momssats, moms)| NyRad {
                konto: konto.map(|k| k.to_string()).unwrap_or_default(),
                netto: belopp(netto),
                momssats: momssats.to_string(),
                moms: belopp(moms),
            })
            .collect(),
    };
    let historik =
        behandlingshistorik::läs_om(&mut *conn, företag, "leverantörsfaktura", f.id).await?;
    let registrerad = match (f.registrerad, f.registrerad_kanal) {
        (Some(tid), Some(kanal)) => Some(verifikat::signatur(
            &tid,
            &kanal,
            f.registrerad_av,
            f.registrerad_namn,
        )?),
        _ => None,
    };
    // #148: hänvisningarna åt båda hållen och upplåsningarna.
    let krediterar = match f.krediterar {
        Some(k) => kreditlänkar(
            &mut *conn,
            "SELECT id, fakturanummer, total, status FROM leverantorsfaktura WHERE id = ?",
            k,
        )
        .await?
        .into_iter()
        .next(),
        None => None,
    };
    let krediteras_av = kreditlänkar(
        &mut *conn,
        "SELECT id, fakturanummer, total, status FROM leverantorsfaktura
         WHERE krediterar = ? AND status <> 'utkast' ORDER BY fakturadatum, id",
        f.id,
    )
    .await?;
    let avskrifter = avskrifter(&mut *conn, f.id).await?;
    Ok(Some(Leverantörsfaktura {
        id: LeverantörsfakturaId(f.id),
        status: status_(&f.status)?,
        underlag: UnderlagId(f.underlag_id),
        filnamn: f.filnamn,
        leverantör: f.leverantor_id.map(LeverantörsId),
        leverantörsnamn: f.leverantorsnamn.unwrap_or_default(),
        ny_leverantör: ny,
        fakturadatum: f.fakturadatum.as_deref().map(datum).transpose()?,
        förfallodag: f.forfallodag.as_deref().map(datum).transpose()?,
        total: f.total,
        formulär,
        verifikat: match (f.verifikat_id, f.verifikatnummer) {
            (Some(v), Some(n)) => Some((VerifikatId(v), verifikat::verifikationsnummer(n)?)),
            _ => None,
        },
        skapad: verifikat::signatur(&f.skapad, &f.skapad_kanal, f.skapad_av, f.skapad_namn)?,
        registrerad,
        historik,
        krediterar,
        krediteras_av,
        avskrifter,
    }))
}

/// Fakturorna i en hänvisning: id, nummer, total och status ur frågan `sql` med `id`.
async fn kreditlänkar(
    conn: &mut SqliteConnection,
    sql: &'static str,
    id: i64,
) -> Result<Vec<Kreditlänk>, DatabasFel> {
    let rader: Vec<(i64, String, Option<i64>, String)> =
        sqlx::query_as(sql).bind(id).fetch_all(&mut *conn).await?;
    rader
        .into_iter()
        .map(|(id, fakturanummer, total, status)| {
            Ok(Kreditlänk {
                id: LeverantörsfakturaId(id),
                fakturanummer,
                total: total.unwrap_or_default(),
                status: status_(&status)?,
            })
        })
        .collect()
}

async fn avskrifter(
    conn: &mut SqliteConnection,
    faktura: i64,
) -> Result<Vec<Avskriftsrättelse>, DatabasFel> {
    type Avskriftsrad = (
        Option<i64>,
        Option<i64>,
        String,
        String,
        String,
        Option<i64>,
        Option<String>,
    );
    let rader: Vec<Avskriftsrad> = sqlx::query_as(
        "SELECT a.verifikat_id, a.rattelse_id, a.orsak, a.rattad, a.rattad_kanal, a.rattad_av,
                u.visningsnamn
         FROM leverantorsfaktura_avskrift a LEFT JOIN anvandare u ON u.id = a.rattad_av
         WHERE a.faktura_id = ? ORDER BY a.id",
    )
    .bind(faktura)
    .fetch_all(&mut *conn)
    .await?;
    let mut ut = Vec::new();
    for (v, r, orsak, tid, kanal, av, namn) in rader {
        ut.push(Avskriftsrättelse {
            verifikat: verifikat::länk(&mut *conn, v).await?,
            rättelse: verifikat::länk(&mut *conn, r).await?,
            orsak,
            rättad: verifikat::signatur(&tid, &kanal, av, namn)?,
        });
    }
    Ok(ut)
}

/// Rätta avskrift (#148).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AvskriftRättad {
    /// Rättelsen av registreringens verifikat; `None` när skulden inte var bokförd.
    pub rättelse: Option<Bokfört>,
}

/// Rätta avskrift… (#148): vänder registreringens verifikat med en rättelse daterad i dag (R1),
/// låser upp fakturan och loggar det, i en transaktion. Betalningarna ligger kvar; fakturan
/// skrivs av och registreras om med samma regler. Orsaken är valfri.
pub async fn rätta_avskrift(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    företag: FöretagsId,
    id: LeverantörsfakturaId,
    orsak: &str,
) -> Result<AvskriftRättad, FakturaFel> {
    let mut ärende = Ärende::börja(db, klocka, aktör, företag, Handling::RättaFlöde).await?;
    type Fakturan = (Option<i64>, String, String, Option<String>, Option<i64>);
    let rad: Option<Fakturan> = sqlx::query_as(
        "SELECT f.verifikat_id, f.registrerad, f.fakturanummer, l.namn, f.total
         FROM leverantorsfaktura f LEFT JOIN leverantor l ON l.id = f.leverantor_id
         WHERE f.id = ? AND f.foretag_id = ? AND f.status = 'registrerad'",
    )
    .bind(id.0)
    .bind(företag.0)
    .fetch_optional(ärende.conn())
    .await?;
    let (verifikat_id, registrering, fakturanummer, leverantörsnamn, total) =
        rad.ok_or(FakturaFel::InteRegistrerad)?;
    let idag = maya_domain::tid::datum(ärende.nu);
    let rättelse = match verifikat_id {
        Some(v) => Some(
            verifikat::rätta_i(&mut ärende, VerifikatId(v), idag, orsak)
                .await
                .map_err(|e| match e {
                    VerifikatFel::Åtkomst(e) => FakturaFel::Åtkomst(e),
                    e => FakturaFel::Rättelse(e),
                })?,
        ),
        None => None,
    };
    let (nu, kanal, av) = (
        ärende.nu.to_string(),
        ärende.aktör.kanal.namn(),
        ärende.aktör.användare.map(|a| a.0),
    );
    sqlx::query(
        "INSERT INTO leverantorsfaktura_avskrift
             (foretag_id, faktura_id, registrering, verifikat_id, rattelse_id, orsak, rattad,
              rattad_kanal, rattad_av)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(företag.0)
    .bind(id.0)
    .bind(&registrering)
    .bind(verifikat_id)
    .bind(rättelse.map(|r| r.id.0))
    .bind(orsak.trim())
    .bind(nu)
    .bind(kanal)
    .bind(av)
    .execute(ärende.conn())
    .await?;
    sqlx::query(
        "UPDATE leverantorsfaktura SET status = 'upplast' WHERE id = ? AND status = 'registrerad'",
    )
    .bind(id.0)
    .execute(ärende.conn())
    .await?;
    let verifikatnummer = verifikat::länk(ärende.conn(), verifikat_id)
        .await?
        .and_then(|l| l.nummer)
        .map(|n| n.to_string());
    let leverantörsnamn = leverantörsnamn.unwrap_or_default();
    ärende
        .logga(
            Händelse::AvskriftRättad,
            &format!("{fakturanummer} {leverantörsnamn}"),
            json!({
                "leverantörsfaktura": id.0,
                "fakturanummer": fakturanummer,
                "leverantörsnamn": leverantörsnamn,
                "summa_öre": total,
                "orsak": orsak.trim(),
                "verifikat": verifikat_id.map(|v| json!({ "id": v, "nummer": verifikatnummer })),
                "rättelse": rättelse.map(|r| json!({ "id": r.id.0, "nummer": r.nummer.to_string() })),
                "datum": idag.to_string(),
            }),
        )
        .await?;
    ärende.slutför().await?;
    Ok(AvskriftRättad { rättelse })
}

/// Att `id` är ett utkast eller en upplåst faktura i ärendets företag, som skrivs av; ger dess
/// leverantör och status.
async fn kräv_avskrift(
    ärende: &mut Ärende,
    id: LeverantörsfakturaId,
) -> Result<(Option<LeverantörsId>, Status), FakturaFel> {
    let företag = ärende.företag;
    let rad: Option<(Option<i64>, String)> = sqlx::query_as(
        "SELECT leverantor_id, status FROM leverantorsfaktura
         WHERE id = ? AND foretag_id = ? AND status IN ('utkast', 'upplast')",
    )
    .bind(id.0)
    .bind(företag.0)
    .fetch_optional(ärende.conn())
    .await?;
    match rad {
        Some((l, status)) => Ok((l.map(LeverantörsId), status_(&status)?)),
        None => Err(FakturaFel::IngetUtkast),
    }
}

/// Att `id` är ett utkast i ärendets företag; ger dess leverantör.
async fn kräv_utkast(
    ärende: &mut Ärende,
    id: LeverantörsfakturaId,
) -> Result<Option<LeverantörsId>, FakturaFel> {
    let företag = ärende.företag;
    let rad: Option<Option<i64>> = sqlx::query_scalar(
        "SELECT leverantor_id FROM leverantorsfaktura
         WHERE id = ? AND foretag_id = ? AND status = 'utkast'",
    )
    .bind(id.0)
    .bind(företag.0)
    .fetch_optional(ärende.conn())
    .await?;
    match rad {
        Some(l) => Ok(l.map(LeverantörsId)),
        None => Err(FakturaFel::IngetUtkast),
    }
}

async fn aktiv_leverantör(
    conn: &mut SqliteConnection,
    företag: FöretagsId,
    id: LeverantörsId,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM leverantor WHERE foretag_id = ? AND id = ?
                        AND status = 'aktiv')",
    )
    .bind(företag.0)
    .bind(id.0)
    .fetch_one(conn)
    .await
}

async fn läs_konton(
    conn: &mut SqliteConnection,
    företag: FöretagsId,
) -> Result<Vec<(Kontonummer, bool)>, DatabasFel> {
    Ok(verifikat::läs_förutsättningar(conn, företag).await?.konton)
}
