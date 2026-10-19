//! SIE4-exporten med återläsning, genom `core` mot en tillfällig databas.

use jiff::civil::{Date, date};
use maya_core::aktor::Aktör;
use maya_core::betalning::{self, Faktura};
use maya_core::foretag::FöretagsId;
use maya_core::sie4::{self, Sie4Fel};
use maya_core::{
    Databas, Klocka, behandlingshistorik, kundfaktura, kundforlust, leverantorsfaktura, saldo,
    testhjalp, verifikat,
};
use maya_domain::betalning::NyBetalning;
use maya_domain::ingaende_balans::{NyIngåendeBalans, NySaldorad};
use maya_domain::kreditfaktura::NyKreditering;
use maya_domain::kundforlust::NyKundförlust;
use maya_domain::leverantorsfaktura::{NyLeverantörsfaktura, NyRad as NyLeverantörsrad};
use maya_domain::verifikat::{NyRad, NyttVerifikat};

const ÅR_2026: Date = date(2026, 1, 1);
const ÅR_2027: Date = date(2027, 1, 1);

fn klocka(tid: &str) -> Klocka {
    Klocka::fast(tid.parse().unwrap())
}

fn verifikat(datum: &str, beskrivning: &str, rader: &[(&str, &str, &str, &str)]) -> NyttVerifikat {
    NyttVerifikat {
        datum: datum.into(),
        beskrivning: beskrivning.into(),
        rader: rader
            .iter()
            .map(|&(konto, debet, kredit, text)| NyRad {
                konto: konto.into(),
                debet: debet.into(),
                kredit: kredit.into(),
                text: text.into(),
            })
            .collect(),
    }
}

/// Ett företag med en inskriven ingående balans 2026, två verifikat 2026, ett 2027 (som skapar
/// året) och ett utkast 2027.
async fn bokföring(db: &Databas) -> (Aktör, FöretagsId) {
    let (anna, id) = testhjalp::företag(db).await;
    let ib = NyIngåendeBalans {
        rader: vec![
            NySaldorad {
                konto: "1930".into(),
                debet: "10 000".into(),
                kredit: String::new(),
            },
            NySaldorad {
                konto: "2081".into(),
                debet: String::new(),
                kredit: "10 000".into(),
            },
        ],
    };
    saldo::ändra_ingående_balans(db, &klocka("2026-01-02T08:00:00Z"), &anna, id, ÅR_2026, &ib)
        .await
        .unwrap();
    for (tid, formulär) in [
        (
            "2026-03-15T09:00:00Z",
            verifikat(
                "2026-03-14",
                "Försäljning",
                &[
                    ("1930", "1 250", "", ""),
                    ("3001", "", "1 000", ""),
                    ("2611", "", "250", ""),
                ],
            ),
        ),
        (
            // 23.30 UTC är nästa dag i Stockholm.
            "2027-01-09T23:30:00Z",
            verifikat(
                "2026-12-31",
                "Bankavgift",
                &[("6570", "100", "", ""), ("1930", "", "100", "")],
            ),
        ),
        (
            "2027-02-02T10:00:00Z",
            verifikat(
                "2027-02-01",
                "Försäljning \"Kaffe\" – kontant",
                &[
                    ("1930", "500", "", ""),
                    ("3001", "", "400", "Kaffe à 25 kr"),
                    ("2611", "", "100", ""),
                ],
            ),
        ),
    ] {
        verifikat::bokför(db, &klocka(tid), &anna, id, &formulär)
            .await
            .unwrap();
    }
    verifikat::spara_utkast(
        db,
        &klocka("2027-02-02T11:00:00Z"),
        &anna,
        id,
        None,
        &verifikat("2027-02-02", "Utkast", &[("1930", "999", "", "")]),
    )
    .await
    .unwrap();
    (anna, id)
}

/// Raderna utan `#PROGRAM` (programversionen) och den avslutande `#KSUMMA`, som beror på den.
/// Kontrollsumman prövas av att filen läses tillbaka.
fn utan_version(fil: &[u8]) -> Vec<&[u8]> {
    fil.split_inclusive(|&b| b == b'\n')
        .filter(|rad| !rad.starts_with(b"#PROGRAM ") && !rad.starts_with(b"#KSUMMA "))
        .collect()
}

#[tokio::test]
async fn exporten_blir_den_förväntade_filen_och_läses_tillbaka_till_samma_saldon() {
    let db = Databas::tillfällig().await;
    let (anna, id) = bokföring(&db).await;

    let export = sie4::exportera(&db, &klocka("2027-02-03T08:00:00Z"), &anna, id, ÅR_2027)
        .await
        .unwrap();

    assert_eq!(export.filnamn, "SIE4_5560360793_2027.se");
    let förväntad = include_bytes!("../golden/sie4_2027.se");
    maya_sie::läs(förväntad).expect("den förväntade filen är giltig");
    assert_eq!(
        utan_version(&export.innehåll)
            .iter()
            .map(|r| String::from_utf8_lossy(r))
            .collect::<String>(),
        utan_version(förväntad)
            .iter()
            .map(|r| String::from_utf8_lossy(r))
            .collect::<String>(),
    );
    assert_eq!(utan_version(&export.innehåll), utan_version(förväntad));

    let fil = maya_sie::läs(&export.innehåll).unwrap();
    assert_eq!(fil.program, "Maya");
    assert_eq!(fil.version, env!("CARGO_PKG_VERSION"));
    samma_saldon(&db, &anna, id, &fil, &[(0, ÅR_2027), (-1, ÅR_2026)]).await;
}

/// Filens #IB, #UB och #RES per årsnummer är databasens saldon för året.
async fn samma_saldon(
    db: &Databas,
    anna: &Aktör,
    id: FöretagsId,
    fil: &maya_sie::Sie4,
    år: &[(i32, Date)],
) {
    let saldon = |poster: &[maya_sie::Saldopost], årsnr: i32| -> Vec<(u16, i64)> {
        poster
            .iter()
            .filter(|p| p.årsnr == årsnr)
            .map(|p| (p.konto as u16, p.öre))
            .collect()
    };
    for &(årsnr, år) in år {
        let sb = saldo::saldobalans(db, anna, id, år).await.unwrap();
        let ur_db = |f: fn(&saldo::Kontosaldo) -> i64, balans: bool| -> Vec<(u16, i64)> {
            sb.konton
                .iter()
                .filter(|k| k.konto.typ().balanskonto() == balans && f(k) != 0)
                .map(|k| (k.konto.get(), f(k)))
                .collect()
        };
        assert_eq!(
            saldon(&fil.ingående, årsnr),
            ur_db(|k| k.ingående.öre(), true)
        );
        assert_eq!(
            saldon(&fil.utgående, årsnr),
            ur_db(|k| k.utgående.öre(), true)
        );
        assert_eq!(
            saldon(&fil.resultat, årsnr),
            ur_db(|k| k.utgående.öre(), false)
        );
    }
}

#[tokio::test]
async fn första_året_har_inget_föregående_år() {
    let db = Databas::tillfällig().await;
    let (anna, id) = bokföring(&db).await;

    let export = sie4::exportera(&db, &klocka("2027-02-03T08:00:00Z"), &anna, id, ÅR_2026)
        .await
        .unwrap();

    let fil = maya_sie::läs(&export.innehåll).unwrap();
    assert_eq!(fil.räkenskapsår.len(), 1);
    assert_eq!(fil.räkenskapsår[0].årsnr, 0);
    assert!(fil.ingående.iter().all(|p| p.årsnr == 0));
    let nummer: Vec<_> = fil.verifikat.iter().map(|v| v.nummer).collect();
    assert_eq!(nummer, [1, 2]);
    assert_eq!(fil.verifikat[1].regdatum, Some(date(2027, 1, 10)));
}

#[tokio::test]
async fn exporten_loggas_i_behandlingshistoriken() {
    let db = Databas::tillfällig().await;
    let (anna, id) = bokföring(&db).await;

    let export = sie4::exportera(&db, &klocka("2027-02-03T08:00:00Z"), &anna, id, ÅR_2027)
        .await
        .unwrap();

    let poster = behandlingshistorik::läs(&db, &anna, id).await.unwrap();
    let post = poster.last().unwrap();
    assert_eq!(post.typ, "SIE4 exporterad");
    assert_eq!(post.objekt, "2027-01-01 – 2027-12-31");
    assert_eq!(post.aktör, anna);
    assert_eq!(post.tid, "2027-02-03T08:00:00Z".parse().unwrap());
    assert_eq!(post.beskrivning["filnamn"], "SIE4_5560360793_2027.se");
    assert_eq!(post.beskrivning["antal_verifikat"], 1);
    assert_eq!(post.beskrivning["byte"], export.innehåll.len());
    assert_eq!(post.beskrivning["sha256"].as_str().unwrap().len(), 64);
}

#[tokio::test]
async fn en_avvikelse_vid_återläsningen_ger_ett_fel_och_ingen_fil() {
    let db = Databas::tillfällig().await;
    let (anna, id) = bokföring(&db).await;
    let före = behandlingshistorik::läs(&db, &anna, id)
        .await
        .unwrap()
        .len();
    let nu = klocka("2027-02-03T08:00:00Z");

    // En skrivare som tappar en rad: verifikatet balanserar inte längre mot databasen.
    let fel = testhjalp::exportera_sie4_förvanskad(&db, &nu, &anna, id, ÅR_2026, |fil| {
        fil.verifikat[0].transaktioner.pop();
    })
    .await
    .unwrap_err();
    let Sie4Fel::Avvikelse(avvikelser) = fel else {
        panic!("{fel:?}")
    };
    assert!(
        avvikelser.iter().any(|a| a.contains("2611")),
        "{avvikelser:?}"
    );

    // En skrivare som tappar ett verifikat och ett saldo.
    let fel = testhjalp::exportera_sie4_förvanskad(&db, &nu, &anna, id, ÅR_2027, |fil| {
        fil.verifikat.clear();
        fil.utgående.pop();
    })
    .await
    .unwrap_err();
    let Sie4Fel::Avvikelse(avvikelser) = fel else {
        panic!("{fel:?}")
    };
    assert!(
        avvikelser.iter().any(|a| a.contains("verifikat")),
        "{avvikelser:?}"
    );
    assert!(
        avvikelser.iter().any(|a| a.contains("#UB")),
        "{avvikelser:?}"
    );

    let efter = behandlingshistorik::läs(&db, &anna, id)
        .await
        .unwrap()
        .len();
    assert_eq!(efter, före, "en misslyckad export loggas inte");
}

#[tokio::test]
async fn avstämningen_fångar_ett_fel_i_läsningen_som_filen_byggs_av() {
    let db = Databas::tillfällig().await;
    let (anna, id) = bokföring(&db).await;
    let nu = klocka("2027-02-03T08:00:00Z");

    // Läsningen tappar årets sista verifikat; filen byggs av samma läsning och stämmer med den.
    let fel = testhjalp::exportera_sie4_med_felaktig_läsning(&db, &nu, &anna, id, ÅR_2026)
        .await
        .unwrap_err();

    let Sie4Fel::Avvikelse(avvikelser) = fel else {
        panic!("{fel:?}")
    };
    assert!(
        avvikelser.iter().any(|a| a.contains("verifikat")),
        "{avvikelser:?}"
    );
    assert!(
        avvikelser.iter().any(|a| a.contains("konto 6570")),
        "{avvikelser:?}"
    );
}

#[tokio::test]
async fn bara_den_med_åtkomst_till_företaget_kan_exportera() {
    let db = Databas::tillfällig().await;
    let (_, id) = bokföring(&db).await;
    let bertil = testhjalp::användare(&db, "bertil@example.se").await;
    let nu = klocka("2027-02-03T08:00:00Z");

    let fel = sie4::exportera(&db, &nu, &bertil, id, ÅR_2027)
        .await
        .unwrap_err();
    assert!(matches!(fel, Sie4Fel::Åtkomst(_)), "{fel:?}");
    let fel = sie4::exportera(&db, &nu, &Aktör::cli(), id, ÅR_2027)
        .await
        .unwrap_err();
    assert!(matches!(fel, Sie4Fel::Åtkomst(_)), "{fel:?}");
}

#[tokio::test]
async fn ett_okänt_räkenskapsår_ger_ett_fel() {
    let db = Databas::tillfällig().await;
    let (anna, id) = bokföring(&db).await;

    let fel = sie4::exportera(
        &db,
        &klocka("2027-02-03T08:00:00Z"),
        &anna,
        id,
        date(2025, 1, 1),
    )
    .await
    .unwrap_err();
    assert!(matches!(fel, Sie4Fel::OkäntRäkenskapsår), "{fel:?}");
}

// #155 Ett år med fakturor

/// Ett företag med fakturametoden och ett år med fakturor: tre kundfakturor på 11 875 kr, där
/// den första delbetalas och skrivs av som kundförlust, den andra krediteras och den tredje är
/// obetald, och en leverantörsfaktura på 1 250 kr som betalas med öresutjämning.
async fn fakturaår(db: &Databas) -> (Aktör, FöretagsId) {
    let (anna, id) = testhjalp::fakturaföretag(db).await;
    let mut kundfakturor = Vec::new();
    for (namn, datum, tid) in [
        ("Kund AB", "2026-03-10", "2026-03-10T09:00:00Z"),
        ("Andra Kunden AB", "2026-05-05", "2026-05-05T09:00:00Z"),
        ("Tredje Kunden AB", "2026-06-01", "2026-06-01T09:00:00Z"),
    ] {
        let kund = testhjalp::kund(db, &anna, id, namn).await;
        let utkast = testhjalp::kundfakturautkast(db, &anna, id, kund, datum).await;
        let f = kundfaktura::utfärda(db, &klocka(tid), &anna, id, utkast)
            .await
            .unwrap();
        kundfakturor.push(f.id);
    }
    betalning::registrera(
        db,
        &klocka("2026-04-01T10:00:00Z"),
        &anna,
        id,
        Faktura::Kund(kundfakturor[0]),
        &NyBetalning {
            betaldag: "2026-04-01".into(),
            belopp: "5 000".into(),
            konto: "1930".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    kundforlust::bokför(
        db,
        &klocka("2026-09-30T10:00:00Z"),
        &anna,
        id,
        kundfakturor[0],
        &NyKundförlust {
            skäl: "konkurs".into(),
            annat: String::new(),
            datum: "2026-09-30".into(),
        },
    )
    .await
    .unwrap();
    let kredit = klocka("2026-05-20T10:00:00Z");
    let utkast = kundfaktura::kreditera(
        db,
        &kredit,
        &anna,
        id,
        kundfakturor[1],
        &NyKreditering {
            hela: true,
            antal: Vec::new(),
        },
    )
    .await
    .unwrap();
    kundfaktura::utfärda(db, &kredit, &anna, id, utkast)
        .await
        .unwrap();

    let leverantör = testhjalp::leverantör(db, &anna, id, "Kontorsvaror AB").await;
    let registrering = klocka("2026-03-15T10:00:00Z");
    let (f, _) = leverantorsfaktura::ladda_upp(
        db,
        &registrering,
        &anna,
        id,
        ("8812.pdf", "application/pdf", b"%PDF-1.4 faktura 8812"),
    )
    .await
    .unwrap();
    leverantorsfaktura::registrera(
        db,
        &registrering,
        &anna,
        id,
        f,
        &NyLeverantörsfaktura {
            leverantör: leverantör.to_string(),
            fakturanummer: "8812".into(),
            fakturadatum: "2026-03-14".into(),
            förfallodag: "2026-04-13".into(),
            totalbelopp: "1 250".into(),
            rader: vec![NyLeverantörsrad {
                konto: "5410".into(),
                netto: "1 000".into(),
                momssats: "25".into(),
                moms: String::new(),
            }],
            ..Default::default()
        },
    )
    .await
    .unwrap();
    betalning::registrera(
        db,
        &klocka("2026-04-13T10:00:00Z"),
        &anna,
        id,
        Faktura::Leverantör(f),
        &NyBetalning {
            betaldag: "2026-04-13".into(),
            belopp: "1 245".into(),
            konto: "1930".into(),
            öresutjämning: true,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    (anna, id)
}

#[tokio::test]
async fn ett_år_med_fakturor_blir_den_förväntade_filen_och_läses_tillbaka_till_samma_saldon() {
    let db = Databas::tillfällig().await;
    let (anna, id) = fakturaår(&db).await;

    let export = sie4::exportera(&db, &klocka("2026-10-20T08:00:00Z"), &anna, id, ÅR_2026)
        .await
        .unwrap();

    assert_eq!(export.filnamn, "SIE4_5560360793_2026.se");
    let förväntad = include_bytes!("../golden/sie4_fakturor_2026.se");
    maya_sie::läs(förväntad).expect("den förväntade filen är giltig");
    assert_eq!(
        utan_version(&export.innehåll)
            .iter()
            .map(|r| String::from_utf8_lossy(r))
            .collect::<String>(),
        utan_version(förväntad)
            .iter()
            .map(|r| String::from_utf8_lossy(r))
            .collect::<String>(),
    );
    assert_eq!(utan_version(&export.innehåll), utan_version(förväntad));

    let fil = maya_sie::läs(&export.innehåll).unwrap();
    // Utfärdandena, betalningarna, kundförlusten, krediten och registreringen.
    assert_eq!(fil.verifikat.len(), 8);
    samma_saldon(&db, &anna, id, &fil, &[(0, ÅR_2026)]).await;
    // Den tredje kundfakturan är obetald, och leverantörsskulden är betald.
    let ub = |konto: u32| {
        fil.utgående
            .iter()
            .find(|p| p.årsnr == 0 && p.konto == konto)
            .map(|p| p.öre)
    };
    assert_eq!(ub(1510), Some(1_187_500));
    assert_eq!(ub(2440), None);
}
