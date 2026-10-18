//! Systemdokumentationen genereras per företag ur data och de inbyggda reglerna (S1–S6).

use maya_core::atkomst::ÅtkomstFel;
use maya_core::config::Config;
use maya_core::systemdokumentation::systemdokumentation;
use maya_core::{Databas, Klocka, kontoplan, sie4, testhjalp, utskrift};
use maya_domain::kontoplan::{Kontonummer, bas, ink2};
use maya_domain::systemdokumentation::{Avsnitt, Block, Systemdokumentation, Tabellrad, Text};

const BFL_KONTROLL: &str = include_str!("../../../docs/bfl-kontroll.md");

fn config(par: &[(&str, &str)]) -> Config {
    Config::from_env(
        [
            ("MAYA_PUBLIC_URL", "https://bokforing.example.se"),
            ("MAYA_BACKUP_DIR", "/backup"),
        ]
        .iter()
        .chain(par)
        .map(|(k, v)| (k.to_string(), v.to_string())),
    )
    .unwrap()
}

fn avsnitt<'a>(dok: &'a Systemdokumentation, id: &str) -> &'a Avsnitt {
    dok.avsnitt
        .iter()
        .find(|a| a.id == id)
        .unwrap_or_else(|| panic!("avsnittet {id} saknas"))
}

/// Avsnittets text, utan stil, en rad per stycke, listpunkt, faktarad och tabellrad.
fn text(a: &Avsnitt) -> String {
    let mut ut = Vec::new();
    for b in &a.block {
        match b {
            Block::Underrubrik(t) => ut.push(t.clone()),
            Block::Stycke(t) => ut.push(t.ren()),
            Block::Lista(punkter) => ut.extend(punkter.iter().map(Text::ren)),
            Block::Fakta(rader) => {
                ut.extend(rader.iter().map(|(r, v)| format!("{r}: {}", v.ren())))
            }
            Block::Tabell(t) => {
                ut.push(t.kolumner.join(" | "));
                ut.extend(t.rader.iter().map(|r| match r {
                    Tabellrad::Rubrik(r) => r.clone(),
                    Tabellrad::Rad(c) => c.iter().map(Text::ren).collect::<Vec<_>>().join(" | "),
                }))
            }
            Block::Länk { text, href } => ut.push(format!("{text} <{href}>")),
        }
    }
    ut.join("\n")
}

#[tokio::test]
async fn avsnitten_finns_i_ordning_med_krav_som_finns_i_bfl_kontrollen() {
    let db = Databas::tillfällig().await;
    let (anna, företag) = testhjalp::företag(&db).await;

    let dok = systemdokumentation(&db, &testhjalp::config(), &anna, företag)
        .await
        .unwrap();

    let ids: Vec<&str> = dok.avsnitt.iter().map(|a| a.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "kontoplan",
            "sammanstallning",
            "samlingsplan",
            "behandlingsregler",
            "verifieringskedja",
            "utskrift",
            "overforingar",
            "arkivplan",
            "version",
        ]
    );
    let krav = |id: &str| avsnitt(&dok, id).krav.clone();
    assert_eq!(krav("kontoplan"), ["S1"]);
    assert_eq!(krav("samlingsplan"), ["S1", "VF4"]);
    assert_eq!(krav("behandlingsregler"), ["S2"]);
    assert_eq!(krav("verifieringskedja"), ["S3"]);
    assert_eq!(krav("utskrift"), ["S4", "A2"]);
    assert_eq!(krav("overforingar"), ["S5", "A4"]);
    assert_eq!(krav("version"), ["S6"]);
    for a in &dok.avsnitt {
        assert!(!a.krav.is_empty(), "{} har inga krav", a.id);
        for id in &a.krav {
            assert!(
                BFL_KONTROLL.contains(&format!("- **{id}** ")),
                "{id} i {} finns inte i docs/bfl-kontroll.md",
                a.id
            );
        }
        assert!(!a.block.is_empty(), "{} är tomt", a.id);
    }
    assert_eq!(dok.företag, "Exempel AB");
    assert_eq!(dok.orgnr, "556036-0793");
}

#[tokio::test]
async fn kontoplanen_är_företagets_som_i_utskriften() {
    let db = Databas::tillfällig().await;
    let (anna, företag) = testhjalp::företag(&db).await;
    let kassa = Kontonummer::new(1910).unwrap();
    kontoplan::byt_namn(&db, &Klocka::system(), &anna, företag, kassa, "Handkassan")
        .await
        .unwrap();

    let dok = systemdokumentation(&db, &testhjalp::config(), &anna, företag)
        .await
        .unwrap();

    let t = text(avsnitt(&dok, "kontoplan"));
    assert!(
        t.contains("1910 | Handkassan | Aktiv | 2.26 Kassa, bank och redovisningsmedel"),
        "{t}"
    );
    assert!(t.contains("1 Tillgångar"), "{t}");
    assert!(
        t.contains(&format!("{} konton", bas::konton().len())),
        "{t}"
    );
    assert!(t.contains("/installningar/kontoplan/pdf"), "{t}");
}

#[tokio::test]
async fn sammanställningen_visar_ink2_tabellens_intervall_och_rubriker() {
    let db = Databas::tillfällig().await;
    let (anna, företag) = testhjalp::företag(&db).await;

    let dok = systemdokumentation(&db, &testhjalp::config(), &anna, företag)
        .await
        .unwrap();

    let t = text(avsnitt(&dok, "sammanstallning"));
    assert!(t.contains(ink2::UTGÅVA), "{t}");
    assert!(
        t.contains(
            "2.26 | Kassa, bank och redovisningsmedel | Balansräkning › Tillgångar › \
             Omsättningstillgångar › Kassa och bank | 1900–1999"
        ),
        "{t}"
    );
    // 899x står på 3.26 vid vinst och 3.27 vid förlust.
    assert!(t.contains("8990–8999 (om netto +)"), "{t}");
    assert!(t.contains("8990–8999 (om netto −)"), "{t}");
}

#[tokio::test]
async fn samlingsplanen_visar_serien_v_per_räkenskapsår_med_perioder() {
    let db = Databas::tillfällig().await;
    let (anna, företag) = testhjalp::företag(&db).await;
    let rader = [(1930, 10_000, 0), (3001, 0, 10_000)];
    testhjalp::bokför(&db, &anna, företag, "2026-03-14", &rader).await;
    testhjalp::bokför(&db, &anna, företag, "2026-04-02", &rader).await;
    testhjalp::bokför(&db, &anna, företag, "2027-01-10", &rader).await;

    let dok = systemdokumentation(&db, &testhjalp::config(), &anna, företag)
        .await
        .unwrap();

    let t = text(avsnitt(&dok, "samlingsplan"));
    assert!(
        t.contains("2026-01-01 – 2026-12-31 | V | V1–V2 | januari 2026 – december 2026 (12)"),
        "{t}"
    );
    assert!(
        t.contains("2027-01-01 – 2027-12-31 | V | V1 | januari 2027 – december 2027 (12)"),
        "{t}"
    );
    for sammanställning in ["Grundbok", "Huvudbok", "Resultaträkning", "Balansräkning"] {
        assert!(t.contains(sammanställning), "{sammanställning}: {t}");
    }
}

#[tokio::test]
async fn behandlingsreglerna_har_automatkonteringarna_och_företagets_metod() {
    let db = Databas::tillfällig().await;
    let (anna, företag) = testhjalp::företag(&db).await;

    let dok = systemdokumentation(&db, &testhjalp::config(), &anna, företag)
        .await
        .unwrap();

    let a = avsnitt(&dok, "behandlingsregler");
    assert!(a.block.iter().any(
        |b| matches!(b, Block::Tabell(t) if t.kolumner == ["Flöde", "Debet", "Kredit", "Villkor"])
    ));
    let t = text(a);
    assert!(
        t.contains("Bokföringsmetod: Fakturametoden från 2026-01-01"),
        "{t}"
    );
    assert!(t.contains("Momsperiod: Kvartal från 2026-01-01"), "{t}");
    assert!(t.contains("aldrig registreras"), "{t}");
}

#[tokio::test]
async fn arkivplanen_tar_med_driftens_fritext_ordagrant() {
    let db = Databas::tillfällig().await;
    let (anna, företag) = testhjalp::företag(&db).await;
    let config = config(&[
        (
            "MAYA_ARKIVPLAN_OFFSITE",
            "USB-disk till bankfacket varje månad",
        ),
        ("MAYA_ARKIVPLAN_NYCKEL", "Ingen kryptering"),
        ("MAYA_ARKIVPLAN_ANSVARIG", "Anna Andersson"),
        ("MAYA_DRIFT_EPOST", "drift@example.se"),
    ]);

    let dok = systemdokumentation(&db, &config, &anna, företag)
        .await
        .unwrap();

    let a = avsnitt(&dok, "arkivplan");
    let t = text(a);
    for väntat in [
        "Offsite: USB-disk till bankfacket varje månad",
        "Krypteringsnyckel: Ingen kryptering",
        "Ansvarig för driften: Anna Andersson, larm till drift@example.se",
        "katalogen /backup",
        "03:00",
        "7 dagliga och 12 månatliga",
        // Sju år efter kalenderåret då räkenskapsåret slutade (A1).
        "2026-01-01 – 2026-12-31 | 2033-12-31",
    ] {
        assert!(t.contains(väntat), "saknar {väntat:?}:\n{t}");
    }
    assert!(!t.contains("Plats och land"), "{t}");
}

#[tokio::test]
async fn arkivplanen_förutsätter_att_databasen_och_backuperna_förvaras_i_sverige() {
    let db = Databas::tillfällig().await;
    let (anna, företag) = testhjalp::företag(&db).await;

    let dok = systemdokumentation(&db, &config(&[]), &anna, företag)
        .await
        .unwrap();

    let a = avsnitt(&dok, "arkivplan");
    assert!(a.krav.iter().any(|k| k == "A6"), "{:?}", a.krav);
    let t = text(a);
    for väntat in [
        "Maya förutsätter att databasen och alla backuper förvaras i Sverige (BFL 7:2)",
        "Den som driftar servern ansvarar för det",
        "Förvaring i ett annat land stöds inte i Maya 0.1",
    ] {
        assert!(t.contains(väntat), "saknar {väntat:?}:\n{t}");
    }
}

#[tokio::test]
async fn arkivplanen_lägger_backupmålets_åtskillnad_på_den_som_driftar_servern() {
    let db = Databas::tillfällig().await;
    let (anna, företag) = testhjalp::företag(&db).await;

    let dok = systemdokumentation(&db, &config(&[]), &anna, företag)
        .await
        .unwrap();

    let t = text(avsnitt(&dok, "arkivplan"));
    for väntat in [
        "Den som driftar servern väljer ett backupmål som är åtskilt från databasen",
        "Maya kräver att ett backupmål är angivet men kontrollerar inte var det ligger",
    ] {
        assert!(t.contains(väntat), "saknar {väntat:?}:\n{t}");
    }
}

#[tokio::test]
async fn versionen_ink2_utgåvan_bas_och_ändringsloggen_visas() {
    let db = Databas::tillfällig().await;
    let (anna, företag) = testhjalp::företag(&db).await;

    let dok = systemdokumentation(&db, &testhjalp::config(), &anna, företag)
        .await
        .unwrap();

    assert_eq!(dok.version, env!("CARGO_PKG_VERSION"));
    assert_eq!(dok.ink2, ink2::UTGÅVA);
    assert_eq!(dok.bas, bas::VERSION);
    let t = text(avsnitt(&dok, "version"));
    assert!(
        t.contains(&format!("Maya: {}", env!("CARGO_PKG_VERSION"))),
        "{t}"
    );
    assert!(t.contains(&format!("INK2-tabell: {}", ink2::UTGÅVA)), "{t}");
    assert!(t.contains("BAS: BAS 2026"), "{t}");
    assert!(t.contains("0.1"), "{t}");
    assert!(
        t.contains("Första versionen av behandlingsreglerna."),
        "{t}"
    );
    assert!(
        t.contains(&format!(
            "/foretag/{företag}/installningar/behandlingshistorik"
        )),
        "{t}"
    );
}

#[tokio::test]
async fn överföringarna_visar_backupverifieringen_och_sie4_exporterna() {
    let db = Databas::tillfällig().await;
    let (anna, företag) = testhjalp::företag(&db).await;
    let klocka = Klocka::system();
    testhjalp::bokför(
        &db,
        &anna,
        företag,
        "2026-03-14",
        &[(1930, 100, 0), (3001, 0, 100)],
    )
    .await;
    testhjalp::backup(&db, &klocka).await;
    sie4::exportera(&db, &klocka, &anna, företag, "2026-01-01".parse().unwrap())
        .await
        .unwrap();

    let dok = systemdokumentation(&db, &testhjalp::config(), &anna, företag)
        .await
        .unwrap();

    let t = text(avsnitt(&dok, "overforingar"));
    assert!(t.contains("Senaste verifiering:"), "{t}");
    assert!(t.contains("verifierad"), "{t}");
    assert!(t.contains("SIE4_5560360793_2026.se"), "{t}");
    assert!(t.contains("kontrollsummor"), "{t}");
}

#[tokio::test]
async fn verifieringskedjan_beskriver_flödet_med_hashar_och_kompletteringar() {
    let db = Databas::tillfällig().await;
    let (anna, företag) = testhjalp::företag(&db).await;

    let dok = systemdokumentation(&db, &testhjalp::config(), &anna, företag)
        .await
        .unwrap();

    let t = text(avsnitt(&dok, "verifieringskedja"));
    for väntat in [
        "Underlag",
        "SHA-256",
        "komplettering",
        "Grundbok och huvudbok",
        "SIE4",
    ] {
        assert!(t.contains(väntat), "saknar {väntat}: {t}");
    }
}

#[tokio::test]
async fn den_som_saknar_åtkomst_får_ingen_systemdokumentation() {
    let db = Databas::tillfällig().await;
    let (_, företag) = testhjalp::företag(&db).await;
    let bertil = testhjalp::användare(&db, "bertil@example.se").await;

    let fel = systemdokumentation(&db, &testhjalp::config(), &bertil, företag)
        .await
        .unwrap_err();

    assert!(matches!(fel, ÅtkomstFel::IngenÅtkomst), "{fel:?}");
}

#[tokio::test]
async fn systemdokumentationen_skrivs_som_pdf_a_2b() {
    let db = Databas::tillfällig().await;
    let (anna, företag) = testhjalp::företag(&db).await;

    let pdf =
        utskrift::systemdokumentation(&db, &Klocka::system(), &testhjalp::config(), &anna, företag)
            .await
            .unwrap();

    assert!(
        pdf.filnamn.starts_with("systemdokumentation_5560360793_"),
        "{}",
        pdf.filnamn
    );
    maya_pdf::testhjalp::är_pdfa_2b(&pdf.innehåll);
    let alla = maya_pdf::testhjalp::sidor(&pdf.innehåll).concat();
    for väntat in [
        "Systemdokumentation",
        "Exempel AB",
        "Automatkonteringar",
        "Rättelse av ett bokfört verifikat",
        "Arkivplan",
        "Version och ändringslogg",
        "Första versionen av behandlingsreglerna.",
    ] {
        assert!(
            maya_pdf::testhjalp::innehåller(&alla, väntat),
            "saknar {väntat}"
        );
    }
}
