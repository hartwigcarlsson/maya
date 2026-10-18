//! Grundbok och huvudbok (L3, L4), genom `core` mot en tillfällig databas.

use jiff::civil::{Date, date};
use maya_core::aktor::Aktör;
use maya_core::atkomst::ÅtkomstFel;
use maya_core::bokforing::{self, Periodurval, RapportFel};
use maya_core::foretag::FöretagsId;
use maya_core::verifikat::{self, Bokfört};
use maya_core::{Databas, Klocka, testhjalp};
use maya_domain::verifikat::{NyRad, NyttVerifikat};

const ÅR_2026: Date = date(2026, 1, 1);

fn klocka(tid: &str) -> Klocka {
    Klocka::fast(tid.parse().unwrap())
}

/// Ett verifikat med raderna `(konto, debet, kredit)` i kronor som text.
fn formulär(datum: &str, beskrivning: &str, rader: &[(u16, &str, &str)]) -> NyttVerifikat {
    NyttVerifikat {
        datum: datum.into(),
        beskrivning: beskrivning.into(),
        rader: rader
            .iter()
            .map(|&(konto, debet, kredit)| NyRad {
                konto: konto.to_string(),
                debet: debet.into(),
                kredit: kredit.into(),
                text: String::new(),
            })
            .collect(),
    }
}

/// Bokför vid tidpunkten `bokförd`.
async fn bokför(
    db: &Databas,
    aktör: &Aktör,
    företag: FöretagsId,
    bokförd: &str,
    v: NyttVerifikat,
) -> Bokfört {
    verifikat::bokför(db, &klocka(bokförd), aktör, företag, &v)
        .await
        .unwrap()
}

fn hela_året() -> Periodurval {
    Periodurval {
        år: ÅR_2026,
        från: None,
        till: None,
    }
}

#[tokio::test]
async fn grundboken_listar_bokförda_verifikat_i_registreringsordning_per_period() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    // V1 är daterat efter V2 men bokfördes först: grundboken följer bokföringen, inte datumet.
    bokför(
        &db,
        &anna,
        id,
        "2026-03-25T09:00:00Z",
        formulär(
            "2026-03-20",
            "Försäljning",
            &[(1930, "500", ""), (3001, "", "500")],
        ),
    )
    .await;
    bokför(
        &db,
        &anna,
        id,
        "2026-03-25T10:00:00Z",
        formulär(
            "2026-03-05",
            "Hyra",
            &[(5010, "200", ""), (1930, "", "200")],
        ),
    )
    .await;
    bokför(
        &db,
        &anna,
        id,
        "2026-05-02T08:00:00Z",
        formulär(
            "2026-04-30",
            "Telefon",
            &[(6210, "50", ""), (1930, "", "50")],
        ),
    )
    .await;
    // Ett utkast syns aldrig.
    verifikat::spara_utkast(
        &db,
        &klocka("2026-05-03T08:00:00Z"),
        &anna,
        id,
        None,
        &formulär("2026-04-01", "Utkast", &[(6210, "1", ""), (1930, "", "1")]),
    )
    .await
    .unwrap();

    let g = bokforing::grundbok(&db, &anna, id, &hela_året())
        .await
        .unwrap();

    let perioder: Vec<(String, Vec<String>)> = g
        .perioder
        .iter()
        .map(|p| {
            (
                p.period.start.to_string(),
                p.verifikat
                    .iter()
                    .map(|v| format!("{} {} {} {}", v.nummer, v.datum, v.beskrivning, v.summa))
                    .collect::<Vec<_>>(),
            )
        })
        .filter(|(_, v)| !v.is_empty())
        .collect();
    assert_eq!(
        perioder,
        vec![
            (
                "2026-03-01".to_string(),
                vec![
                    "V1 2026-03-20 Försäljning 500,00".to_string(),
                    "V2 2026-03-05 Hyra 200,00".to_string(),
                ]
            ),
            (
                "2026-04-01".to_string(),
                vec!["V3 2026-04-30 Telefon 50,00".to_string()]
            ),
        ]
    );
    assert_eq!(g.perioder.len(), 12, "alla årets perioder, också tomma");
    let v1 = &g.perioder[2].verifikat[0];
    assert_eq!(v1.bokförd.tid, "2026-03-25T09:00:00Z".parse().unwrap());
    assert_eq!(v1.bokförd.namn, "anna@example.se");
    let rader: Vec<_> = v1
        .rader
        .iter()
        .map(|r| {
            (
                r.konto.get(),
                r.kontonamn.as_str(),
                r.debet.to_string(),
                r.kredit.to_string(),
            )
        })
        .collect();
    assert_eq!(
        rader,
        vec![
            (1930, "Företagskonto", "500,00".into(), "0,00".into()),
            (
                3001,
                "Försäljning inom Sverige, 25 % moms",
                "0,00".into(),
                "500,00".into()
            ),
        ]
    );
}

#[tokio::test]
async fn grundboken_visar_bara_urvalets_perioder() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    bokför(
        &db,
        &anna,
        id,
        "2026-02-01T09:00:00Z",
        formulär("2026-01-31", "Januari", &[(1930, "1", ""), (3001, "", "1")]),
    )
    .await;
    bokför(
        &db,
        &anna,
        id,
        "2026-03-01T09:00:00Z",
        formulär(
            "2026-02-15",
            "Februari",
            &[(1930, "2", ""), (3001, "", "2")],
        ),
    )
    .await;
    bokför(
        &db,
        &anna,
        id,
        "2026-04-01T09:00:00Z",
        formulär("2026-03-15", "Mars", &[(1930, "3", ""), (3001, "", "3")]),
    )
    .await;

    let urval = Periodurval {
        år: ÅR_2026,
        från: Some(date(2026, 2, 1)),
        till: Some(date(2026, 3, 31)),
    };
    let g = bokforing::grundbok(&db, &anna, id, &urval).await.unwrap();

    let texter: Vec<_> = g
        .perioder
        .iter()
        .flat_map(|p| p.verifikat.iter().map(|v| v.beskrivning.as_str()))
        .collect();
    assert_eq!(texter, ["Februari", "Mars"]);
    assert_eq!(g.perioder.len(), 2);

    let baklänges = Periodurval {
        år: ÅR_2026,
        från: Some(date(2026, 3, 1)),
        till: Some(date(2026, 2, 1)),
    };
    assert!(matches!(
        bokforing::grundbok(&db, &anna, id, &baklänges).await,
        Err(RapportFel::OkändPeriod)
    ));
    let utanför = Periodurval {
        till: Some(date(2027, 1, 1)),
        ..urval
    };
    assert!(matches!(
        bokforing::grundbok(&db, &anna, id, &utanför).await,
        Err(RapportFel::OkändPeriod)
    ));
    let okänt_år = Periodurval {
        år: date(2025, 1, 1),
        ..hela_året()
    };
    assert!(matches!(
        bokforing::grundbok(&db, &anna, id, &okänt_år).await,
        Err(RapportFel::OkäntRäkenskapsår)
    ));
}

#[tokio::test]
async fn den_som_saknar_åtkomst_får_ingen_grundbok() {
    let db = Databas::tillfällig().await;
    let (_, id) = testhjalp::företag(&db).await;
    let bertil = testhjalp::användare(&db, "bertil@example.se").await;

    assert!(matches!(
        bokforing::grundbok(&db, &bertil, id, &hela_året()).await,
        Err(RapportFel::Åtkomst(ÅtkomstFel::IngenÅtkomst))
    ));
}
