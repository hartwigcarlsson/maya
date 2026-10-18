//! Systemdokumentationen per företag (S1–S6, S3 för etappens flöden): kontoplan,
//! sammanställning till årsredovisningen, samlingsplan, behandlingsregler, verifieringskedja,
//! utskrift, överföringar, arkivplan och version. Den genereras vid varje läsning ur företagets
//! data, konfigurationen och de inbyggda reglerna i `maya_domain::systemdokumentation`, så den
//! stämmer alltid med systemet. Webben visar den och `utskrift::systemdokumentation` skriver
//! samma dokument som PDF.
//!
//! Länkarna är sökvägar i webben, så att revisorn når varje del som beskrivs.

use jiff::Timestamp;
use jiff::civil::Date;
use maya_domain::backup::{DAGLIGA, MÅNATLIGA};
use maya_domain::kontoplan::{bas, ink2, klassnamn, om_numret};
use maya_domain::rakenskapsar::periodnamn;
use maya_domain::systemdokumentation::{
    Avsnitt, Block, Systemdokumentation, Tabell, Tabellrad, Text, behandlingsregler, ändringslogg,
};
use maya_domain::underlag::STÖRSTA_STORLEK;
use serde_json::Value;

use crate::aktor::Aktör;
use crate::atkomst::ÅtkomstFel;
use crate::backup::{self, Backupmål, Körning};
use crate::behandlingshistorik::{self, Händelse, Post, Urval};
use crate::config::Config;
use crate::foretag::{self, Företag, FöretagsId};
use crate::{Databas, kontoplan, underlag, verifikat};

/// Det som står när driften inte har angett något i arkivplanens fritext.
const EJ_ANGIVET: &str = "ej angivet";

/// Systemdokumentationen för företaget, som den gäller nu.
pub async fn systemdokumentation(
    db: &Databas,
    config: &Config,
    aktör: &Aktör,
    företag: FöretagsId,
) -> Result<Systemdokumentation, ÅtkomstFel> {
    let f = foretag::företaget(db, aktör, företag).await?;
    let konton = kontoplan::konton(db, aktör, företag).await?;
    let bokförda = verifikat::lista(db, aktör, företag, verifikat::Urval::Bokförda).await?;
    let underlag = underlag::lista(db, aktör, företag, underlag::Urval::Alla).await?;
    let historik = behandlingshistorik::historik(db, aktör, företag, &Urval::default())
        .await?
        .poster;
    let status = backup::status(db).await?;
    let bas_version = konton
        .iter()
        .filter_map(|k| k.bas_version.clone())
        .max()
        .unwrap_or_else(|| bas::VERSION.to_string());
    let bas = format!("/foretag/{företag}");

    let avsnitt = vec![
        kontoplanen(&konton, &bas_version, &bas),
        sammanställningen(),
        samlingsplanen(&f, &bokförda, &bas),
        behandlingsreglerna(&f),
        verifieringskedjan(&bokförda, &underlag, &bas),
        utskriften(&bas),
        överföringarna(config, &status, &historik),
        arkivplanen(&f, config),
        versionen(&bas_version, &bas),
    ];
    Ok(Systemdokumentation {
        företag: f.namn.clone(),
        orgnr: f.orgnr.to_string(),
        version: env!("CARGO_PKG_VERSION").into(),
        ink2: ink2::UTGÅVA.into(),
        bas: bas_version,
        avsnitt,
    })
}

fn avsnitt(id: &str, rubrik: &str, krav: &[&str], block: Vec<Block>) -> Avsnitt {
    Avsnitt {
        id: id.into(),
        rubrik: rubrik.into(),
        krav: krav.iter().map(|k| k.to_string()).collect(),
        block,
    }
}

fn stycke(text: impl Into<Text>) -> Block {
    Block::Stycke(text.into())
}

fn länk(text: &str, href: String) -> Block {
    Block::Länk {
        text: text.into(),
        href,
    }
}

fn fakta(rader: Vec<(&str, String)>) -> Block {
    Block::Fakta(
        rader
            .into_iter()
            .map(|(rubrik, värde)| (rubrik.to_string(), värde.into()))
            .collect(),
    )
}

fn tabell(kolumner: &[&str], rader: Vec<Tabellrad>) -> Block {
    Block::Tabell(Tabell {
        kolumner: kolumner.iter().map(|k| k.to_string()).collect(),
        rader,
    })
}

fn rad<const N: usize>(celler: [String; N]) -> Tabellrad {
    Tabellrad::Rad(celler.into_iter().map(Text::from).collect())
}

/// En tidpunkt i Stockholm, till minuten.
fn tid(t: Timestamp) -> String {
    t.to_zoned(maya_domain::tid::stockholm())
        .strftime("%Y-%m-%d %H:%M")
        .to_string()
}

fn kontoplanen(konton: &[kontoplan::Konto], bas_version: &str, bas: &str) -> Avsnitt {
    let aktiva = konton.iter().filter(|k| k.aktiv).count();
    let egna = konton.iter().filter(|k| k.bas_namn.is_none()).count();
    let mut rader = Vec::new();
    let mut klass = None;
    for k in konton {
        if klass != Some(k.nummer.klass()) {
            klass = Some(k.nummer.klass());
            rader.push(Tabellrad::Rubrik(format!(
                "{} {}",
                k.nummer.klass(),
                klassnamn(k.nummer.klass())
            )));
        }
        let ink2 = om_numret(k.nummer)
            .ink2
            .iter()
            .map(|r| format!("{} {}", r.rad(), r.benämning()))
            .collect::<Vec<_>>()
            .join(" / ");
        rader.push(rad([
            k.nummer.to_string(),
            k.namn.clone(),
            if k.aktiv { "Aktiv" } else { "Inaktiv" }.into(),
            ink2,
        ]));
    }
    avsnitt(
        "kontoplan",
        "Kontoplan",
        &["S1"],
        vec![
            stycke(format!(
                "Företagets kontoplan bygger på {bas_version}, som kopierades till företaget när \
                 det lades upp. Den har {} konton, varav {aktiva} aktiva och {egna} egna. Bara \
                 aktiva konton kan bokföras på. Kontoklassen ur numret avgör kontots typ, och \
                 INK2-raden kommer ur sammanställningen nedan.",
                konton.len()
            )),
            länk("Kontoplanen", format!("{bas}/installningar/kontoplan")),
            länk(
                "Kontoplanen som PDF",
                format!("{bas}/installningar/kontoplan/pdf"),
            ),
            tabell(&["Konto", "Namn", "Status", "INK2-rad"], rader),
        ],
    )
}

fn sammanställningen() -> Avsnitt {
    let intervall: Vec<_> = ink2::intervall().collect();
    let rader = ink2::rader()
        .iter()
        .map(|r| {
            let konton = intervall
                .iter()
                .filter_map(|(från, till, koppling)| {
                    let (plus, minus) = (koppling.fält(true).rad(), koppling.fält(false).rad());
                    let spann = if från == till {
                        från.to_string()
                    } else {
                        format!("{från}–{till}")
                    };
                    match (plus == r, minus == r) {
                        (true, true) => Some(spann),
                        (true, false) => Some(format!("{spann} (om netto +)")),
                        (false, true) => Some(format!("{spann} (om netto −)")),
                        (false, false) => None,
                    }
                })
                .collect::<Vec<_>>()
                .join(", ");
            rad([
                r.rad().to_string(),
                r.benämning().to_string(),
                r.rubriker().join(" › "),
                konton,
            ])
        })
        .collect();
    avsnitt(
        "sammanstallning",
        "Sammanställning till årsredovisningen",
        &["S1"],
        vec![
            stycke(format!(
                "Kontona sammanställs till årsredovisningen och inkomstdeklarationen genom \
                 INK2-kopplingstabellen, utgåva {}, som byggs in ur BAS fria kopplingstabell. \
                 Tabellen är densamma för alla företag och går inte att ändra per företag. Egna \
                 konton får sin rad ur intervallen. Saldots tecken avgör om plus- eller \
                 minusfältet används. Balansräkningens rubriker följer blankettens; \
                 resultaträkningens delsummor (rörelseresultat, resultat efter finansiella \
                 poster, resultat före skatt och årets resultat) är inbyggda i rapporten.",
                ink2::UTGÅVA
            )),
            tabell(&["Rad", "Benämning", "Rubriker", "Konton"], rader),
        ],
    )
}

fn samlingsplanen(f: &Företag, bokförda: &[verifikat::Verifikatrad], bas: &str) -> Avsnitt {
    let rader = f
        .räkenskapsår
        .iter()
        .map(|år| {
            let mut nummer: Vec<u32> = bokförda
                .iter()
                .filter(|v| v.datum.is_some_and(|d| år.start <= d && d <= år.slut))
                .filter_map(|v| v.nummer.map(|n| n.get()))
                .collect();
            nummer.sort();
            let serie = match (nummer.first(), nummer.last()) {
                (Some(a), Some(b)) if a == b => format!("V{a}"),
                (Some(a), Some(b)) => format!("V{a}–V{b}"),
                _ => "inga bokförda".into(),
            };
            let perioder = match (år.perioder.first(), år.perioder.last()) {
                (Some(första), Some(sista)) => format!(
                    "{} – {} ({})",
                    periodnamn(första.start),
                    periodnamn(sista.start),
                    år.perioder.len()
                ),
                _ => String::new(),
            };
            rad([
                format!("{} – {}", år.start, år.slut),
                "V".into(),
                serie,
                perioder,
            ])
        })
        .collect();
    let sammanställningar = [
        (
            "Grundbok",
            "rapporter/grundbok",
            "verifikaten i registreringsordning per period (L4)",
        ),
        (
            "Huvudbok",
            "rapporter/huvudbok",
            "per konto med ingående balans, transaktioner och utgående balans (L4)",
        ),
        (
            "Resultaträkning",
            "rapporter/resultatrakning",
            "per månad eller ackumulerat",
        ),
        (
            "Balansräkning",
            "rapporter/balansrakning",
            "ingående balans, förändring och utgående balans",
        ),
        ("Kontoplan", "installningar/kontoplan", "se ovan"),
        (
            "Behandlingshistorik",
            "installningar/behandlingshistorik",
            "varje ändring som påverkar bokföringen (H1–H3)",
        ),
    ];
    let mut block = vec![
        stycke(
            "Verifikaten numreras i en serie V per räkenskapsår, som börjar på V1. Numret \
             tilldelas när verifikatet bokförs, som nästa nummer i årets serie och i samma \
             transaktion, så serien är obruten (VF3). Utkast har inget nummer. Rättelser \
             numreras i samma serie. Ett senare räkenskapsår (12 månader) skapas när det \
             första verifikatet dateras i det. Perioderna är kalendermånader inom året.",
        ),
        tabell(&["Räkenskapsår", "Serie", "Bokförda", "Perioder"], rader),
        stycke(
            "Sammanställningarna räknas vid varje läsning ur de bokförda verifikaten, aldrig \
             ur utkast:",
        ),
    ];
    block.push(tabell(
        &["Sammanställning", "Innehåll"],
        sammanställningar
            .iter()
            .map(|(namn, _, innehåll)| rad([namn.to_string(), innehåll.to_string()]))
            .collect(),
    ));
    block.extend(
        sammanställningar
            .iter()
            .map(|(namn, sökväg, _)| länk(namn, format!("{bas}/{sökväg}"))),
    );
    avsnitt(
        "samlingsplan",
        "Samlingsplan och serier",
        &["S1", "VF4"],
        block,
    )
}

fn behandlingsreglerna(f: &Företag) -> Avsnitt {
    let historik = |val: Vec<String>| val.join("; ");
    let mut block = vec![
        Block::Underrubrik("Företagets val".into()),
        fakta(vec![
            (
                "Bokföringsmetod",
                historik(
                    f.bokföringsmetoder
                        .iter()
                        .map(|(från, m)| format!("{} från {från}", m.namn()))
                        .collect(),
                ),
            ),
            (
                "Momsperiod",
                historik(
                    f.momsperioder
                        .iter()
                        .map(|(från, m)| format!("{} från {från}", m.namn()))
                        .collect(),
                ),
            ),
        ]),
        stycke(
            "Bokföringsmetoden och momsperioden styr flöden som kommer i senare versioner \
             (fakturor och moms). Byten loggas i behandlingshistoriken.",
        ),
    ];
    block.extend(behandlingsregler());
    avsnitt("behandlingsregler", "Behandlingsregler", &["S2"], block)
}

fn verifieringskedjan(
    bokförda: &[verifikat::Verifikatrad],
    underlag: &[underlag::Underlag],
    bas: &str,
) -> Avsnitt {
    let kompletteringar = underlag
        .iter()
        .flat_map(|u| &u.kopplingar)
        .filter(|k| k.komplettering)
        .count();
    avsnitt(
        "verifieringskedja",
        "Verifieringskedja och informationsflöden",
        &["S3"],
        vec![
            stycke(
                "I den här versionen registreras allt för hand i webben; det finns ingen import, \
                 e-faktura eller bankkoppling. Kedjan från affärshändelse till rapport:",
            ),
            Block::Lista(vec![
                Text::markdown(&format!(
                    "**Underlag**: originalfilen laddas upp och sparas oförändrad i databasen, \
                     högst {} MB, med SHA-256 räknad när den togs emot, filnamn, typ, storlek, \
                     vem och när (VF5). Samma hash i företaget ger en dubblettvarning. Ett \
                     underlag som har hört till ett bokfört verifikat kan inte tas bort.",
                    STÖRSTA_STORLEK / (1024 * 1024)
                )),
                Text::markdown(
                    "**Verifikat**: underlagen kopplas till ett utkast, som bokförs med datum, \
                     beskrivning, rader, nummer, vem och när (VF2, H1). Ett bokfört verifikat och \
                     dess kopplingar ändras aldrig (L1). Ett underlag som kopplas efter \
                     bokföringen är en komplettering, med vem och när (VF6). Ett fel rättas med \
                     ett nytt verifikat som länkas till originalet (R1, R2, VF7).",
                ),
                Text::markdown(
                    "**Grundbok och huvudbok**: räknas ur de bokförda verifikaten i \
                     registreringsordning, per period respektive per konto (L4).",
                ),
                Text::markdown(
                    "**Rapporter och SIE4**: resultat- och balansräkningen räknas ur samma \
                     verifikat och den ingående balansen. SIE4-exporten läses tillbaka och \
                     stäms av innan filen lämnas ut, och exporten loggas med filens SHA-256.",
                ),
            ]),
            stycke(
                "Verifikatets PDF förtecknar underlagen med filnamn och SHA-256 och visar \
                 kompletteringar och historik, så att varje rad i rapporterna kan följas tillbaka \
                 till verifikatet och underlaget, och tvärtom.",
            ),
            fakta(vec![
                ("Bokförda verifikat", bokförda.len().to_string()),
                ("Underlag", underlag.len().to_string()),
                ("Kompletteringar", kompletteringar.to_string()),
            ]),
            länk("Underlag", format!("{bas}/underlag")),
            länk("Verifikat", format!("{bas}/verifikat")),
        ],
    )
}

fn utskriften(bas: &str) -> Avsnitt {
    let delar = [
        (
            "Verifikat",
            "Verifikat → öppna verifikatet → Skriv ut eller Exportera PDF",
            "verifikat",
        ),
        (
            "Underlag",
            "Underlag eller verifikatet → öppna filen; originalet laddas ned oförändrat",
            "underlag",
        ),
        (
            "Grundbok",
            "Rapporter → Grundbok → PDF eller CSV för urvalet",
            "rapporter/grundbok",
        ),
        (
            "Huvudbok",
            "Rapporter → Huvudbok → PDF eller CSV för urvalet",
            "rapporter/huvudbok",
        ),
        (
            "Resultaträkning",
            "Rapporter → Resultaträkning → PDF eller CSV",
            "rapporter/resultatrakning",
        ),
        (
            "Balansräkning",
            "Rapporter → Balansräkning → PDF eller CSV",
            "rapporter/balansrakning",
        ),
        ("SIE4", "Rapporter → SIE4 för räkenskapsåret", "rapporter"),
        (
            "Kontoplan",
            "Inställningar → Kontoplan → Skriv ut",
            "installningar/kontoplan",
        ),
        (
            "Behandlingshistorik",
            "Inställningar → Behandlingshistorik → PDF eller CSV för urvalet",
            "installningar/behandlingshistorik",
        ),
        (
            "Systemdokumentation",
            "Inställningar → Systemdokumentation → Skriv ut",
            "installningar/systemdokumentation",
        ),
    ];
    let mut block = vec![
        stycke(
            "All räkenskapsinformation kan visas på skärmen och skrivas ut omedelbart. PDF:erna \
             är PDF/A-2b ur inbyggda mallar, och Skriv ut ger samma PDF som exporten. Sidfoten \
             har tidpunkt, användare, Maya-version, INK2-tabellens utgåva och sidnummer. CSV är \
             UTF-8 med BOM, semikolon och decimalkomma. PDF:en skrivs om vid varje utskrift; en \
             PDF som ska arkiveras sparas som fil.",
        ),
        tabell(
            &["Del", "Så tas den fram"],
            delar
                .iter()
                .map(|(del, hur, _)| rad([del.to_string(), hur.to_string()]))
                .collect(),
        ),
    ];
    block.extend(
        delar
            .iter()
            .map(|(del, _, sökväg)| länk(del, format!("{bas}/{sökväg}"))),
    );
    avsnitt("utskrift", "Utskrift", &["S4", "A2"], block)
}

fn körning(k: &Option<Körning>, lyckad: &str) -> String {
    match k {
        None => "ingen än".into(),
        Some(k) => {
            let utfall = if k.lyckades { lyckad } else { "misslyckades" };
            let backup = k
                .backup
                .as_deref()
                .map(|b| format!(" ({b})"))
                .unwrap_or_default();
            format!("{}, {utfall}{backup}", tid(k.tid))
        }
    }
}

fn överföringarna(config: &Config, status: &backup::Backupstatus, historik: &[Post]) -> Avsnitt {
    let av_typ = |h: Händelse| historik.iter().filter(move |p| p.typ == h.namn());
    let fält = |p: &Post, nyckel: &str| match &p.beskrivning[nyckel] {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        v => v.to_string(),
    };
    let mut block = vec![
        stycke(
            "Varje överföring verifieras innan originalet kasseras eller filen lämnas ut (A4). \
             Kontrollsummorna är antal verifikat per serie och räkenskapsår, debet och kredit \
             per konto och år, en SHA-256 över alla bokförda rader med deras underlagskopplingar \
             och varje underlags SHA-256.",
        ),
        Block::Lista(vec![
            Text::markdown(
                "**Migreringar**: vid start efter en uppgradering tas en backup, och \
                 kontrollsummorna räknas före och efter migreringarna. Vid avvikelse läggs \
                 databasen tillbaka och Maya startar inte. Varje migrering och byte av \
                 programversion loggas i behandlingshistoriken.",
            ),
            Text::markdown(
                "**Backup**: varje natt kopieras databasen med underlagen till backupmålet, med \
                 ett manifest med kontrollsummor och databasfilens SHA-256. Kopian läses \
                 tillbaka ur målet och verifieras: filens hash, databasens integritet, \
                 underlagens hashar och kontrollsummorna mot manifestet. En återställning med \
                 maya restore verifieras på samma sätt före och efter.",
            ),
            Text::markdown(
                "**SIE4-export**: filen läses tillbaka med Mayas SIE-läsare, och antal \
                 verifikat, debet och kredit per konto och IB, UB och RES stäms av mot \
                 databasen. Vid avvikelse lämnas ingen fil ut. Exporten loggas med filens \
                 SHA-256.",
            ),
        ]),
        fakta(vec![
            ("Backupmål", Backupmål::från(&config.backup).beskrivning()),
            (
                "Senaste backup",
                körning(&status.senaste_backup, "lyckades"),
            ),
            (
                "Senaste verifiering",
                körning(&status.senaste_verifiering, "verifierad"),
            ),
        ]),
        Block::Underrubrik("SIE4-exporter".into()),
    ];
    let exporter: Vec<Tabellrad> = av_typ(Händelse::Sie4Exporterad)
        .map(|p| {
            rad([
                tid(p.tid),
                p.objekt.clone(),
                fält(p, "filnamn"),
                fält(p, "antal_verifikat"),
                grupperad(&fält(p, "sha256")),
            ])
        })
        .collect();
    block.push(if exporter.is_empty() {
        stycke("Ingen SIE4-fil har exporterats.")
    } else {
        tabell(
            &["Tid", "Räkenskapsår", "Fil", "Verifikat", "SHA-256"],
            exporter,
        )
    });
    block.push(Block::Underrubrik(
        "Migreringar och programversioner".into(),
    ));
    let starter: Vec<Tabellrad> = historik
        .iter()
        .filter(|p| {
            [
                Händelse::MigreringKörd.namn(),
                Händelse::MigreringMisslyckad.namn(),
                Händelse::ProgramversionBytt.namn(),
                Händelse::ÅterställningGjord.namn(),
            ]
            .contains(&p.typ.as_str())
        })
        .map(|p| rad([tid(p.tid), p.typ.clone(), p.text()]))
        .collect();
    block.push(if starter.is_empty() {
        stycke("Inga migreringar har körts sedan företaget lades upp.")
    } else {
        tabell(&["Tid", "Typ", "Ändring"], starter)
    });
    avsnitt("overforingar", "Överföringar", &["S5", "A4"], block)
}

/// En hash i grupper om åtta tecken, så att den kan radbrytas i en tabell.
fn grupperad(hex: &str) -> String {
    hex.as_bytes()
        .chunks(8)
        .map(|c| String::from_utf8_lossy(c).into_owned())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Sista dagen som räkenskapsinformationen för året som slutar `slut` ska bevaras: 31 december
/// det sjunde året efter kalenderåret då räkenskapsåret slutade (A1).
fn bevaras_till(slut: Date) -> Date {
    Date::new(slut.year() + 7, 12, 31).expect("ett giltigt datum")
}

fn arkivplanen(f: &Företag, config: &Config) -> Avsnitt {
    let fritext = |v: &Option<String>| v.clone().unwrap_or_else(|| EJ_ANGIVET.into());
    let ansvarig = match (&config.arkivplan.ansvarig, &config.drift_epost) {
        (a, Some(epost)) => format!("{}, larm till {epost}", fritext(a)),
        (a, None) => format!("{}, inga larm (MAYA_DRIFT_EPOST saknas)", fritext(a)),
    };
    let databas = config.data_dir.join("maya.db");
    let mut block = vec![
        stycke(
            "Arkivplanen genereras ur konfigurationen och företagets data. Hela installationens \
             räkenskapsinformation ligger i en databas: verifikaten, underlagen som \
             originalfiler, kontoplanen, behandlingshistoriken och den här dokumentationens \
             regler. Ingen kryptering görs i Maya.",
        ),
        fakta(vec![
            (
                "Databasen och underlagen",
                format!("{} (SQLite, underlagen i samma fil)", databas.display()),
            ),
            ("Backupmål", Backupmål::från(&config.backup).beskrivning()),
            (
                "Schema",
                format!(
                    "varje natt {} (Europe/Stockholm), med verifiering",
                    config.backup_time.strftime("%H:%M")
                ),
            ),
            (
                "Sparade kopior",
                format!("{DAGLIGA} dagliga och {MÅNATLIGA} månatliga sparas; äldre gallras"),
            ),
            (
                "Presentation",
                "i webben, som PDF/A-2b och CSV och som SIE4, se Utskrift".into(),
            ),
        ]),
        Block::Underrubrik("Driftens fritext".into()),
        stycke("Det Maya inte kan veta, ordagrant ur konfigurationen:"),
        fakta(vec![
            ("Plats och land", fritext(&config.arkivplan.plats)),
            ("Offsite", fritext(&config.arkivplan.offsite)),
            ("Krypteringsnyckel", fritext(&config.arkivplan.nyckel)),
            ("Ansvarig för driften", ansvarig),
        ]),
    ];
    if config.arkivplan.plats.is_none() {
        block.push(Block::Varning(
            crate::drift::Driftvarning::ArkivplanUtanLand.text().into(),
        ));
    }
    block.extend([
        Block::Underrubrik("Sparandetid".into()),
        stycke(
            "Räkenskapsinformationen bevaras till och med 31 december det sjunde året efter \
             kalenderåret då räkenskapsåret slutade (BFL 7:2). Maya erbjuder ingen radering av \
             räkenskapsinformation; ett företag med bokförda verifikat, underlag eller en inskriven \
             ingående balans kan inte raderas.",
        ),
        tabell(
            &["Räkenskapsår", "Bevaras till och med"],
            f.räkenskapsår
                .iter()
                .map(|år| {
                    rad([
                        format!("{} – {}", år.start, år.slut),
                        bevaras_till(år.slut).to_string(),
                    ])
                })
                .collect(),
        ),
    ]);
    avsnitt(
        "arkivplan",
        "Arkivplan",
        &["S1", "A1", "A5", "A6", "A7"],
        block,
    )
}

fn versionen(bas_version: &str, bas: &str) -> Avsnitt {
    let mut block = vec![
        fakta(vec![
            ("Maya", env!("CARGO_PKG_VERSION").into()),
            ("INK2-tabell", ink2::UTGÅVA.into()),
            ("BAS", bas_version.into()),
        ]),
        stycke(
            "Dokumentationen genereras av den här versionen av Maya och gäller för den. \
             Ändringar i behandlingsreglerna per release står nedan. Ändringar per företag, som \
             kontoplan, bokföringsmetod, momsperiod och programversion, står i \
             behandlingshistoriken.",
        ),
        länk(
            "Behandlingshistorik",
            format!("{bas}/installningar/behandlingshistorik"),
        ),
        Block::Underrubrik("Ändringslogg för behandlingsreglerna".into()),
    ];
    block.push(tabell(
        &["Release", "Ändring"],
        ändringslogg()
            .iter()
            .rev()
            .flat_map(|r| {
                r.ändringar
                    .iter()
                    .map(|ä| rad([r.version.to_string(), ä.to_string()]))
            })
            .collect(),
    ));
    avsnitt("version", "Version och ändringslogg", &["S6"], block)
}
