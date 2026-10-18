//! Grundbok och huvudbok (L3, L4), genom `core` mot en tillfällig databas.

use jiff::civil::{Date, date};
use maya_core::aktor::Aktör;
use maya_core::atkomst::ÅtkomstFel;
use maya_core::bokforing::Huvudbok;
use maya_core::bokforing::{self, Periodurval, RapportFel};
use maya_core::foretag::FöretagsId;
use maya_core::saldo::{self, Status};
use maya_core::verifikat::{self, Bokfört};
use maya_core::{Databas, Klocka, testhjalp};
use maya_domain::ingaende_balans::{NyIngåendeBalans, NySaldorad};
use maya_domain::kontourval::Kontourval;
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
    assert!(matches!(
        bokforing::huvudbok(&db, &bertil, id, &hela_året(), &Kontourval::alla()).await,
        Err(RapportFel::Åtkomst(ÅtkomstFel::IngenÅtkomst))
    ));
}

/// Ingående balans 1 000 kr på banken, en försäljning (V1), en hyra som är daterad före
/// försäljningen men bokförd efter den (V2), en telefonräkning i april (V3) och ett utkast.
async fn exempel(db: &Databas) -> (Aktör, FöretagsId) {
    let (anna, id) = testhjalp::företag(db).await;
    let ib = NyIngåendeBalans {
        rader: vec![
            NySaldorad {
                konto: "1930".into(),
                debet: "1 000".into(),
                kredit: String::new(),
            },
            NySaldorad {
                konto: "2081".into(),
                debet: String::new(),
                kredit: "1 000".into(),
            },
        ],
    };
    saldo::ändra_ingående_balans(db, &klocka("2026-01-02T08:00:00Z"), &anna, id, ÅR_2026, &ib)
        .await
        .unwrap();
    let mut v1 = formulär(
        "2026-03-20",
        "Försäljning; kund \"A\"",
        &[(1930, "500", ""), (3001, "", "500")],
    );
    v1.rader[0].text = "Swish".into();
    bokför(db, &anna, id, "2026-03-25T09:00:00Z", v1).await;
    bokför(
        db,
        &anna,
        id,
        "2026-03-25T10:00:00.5Z",
        formulär(
            "2026-03-05",
            "Hyra",
            &[(5010, "200", ""), (1930, "", "200")],
        ),
    )
    .await;
    bokför(
        db,
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
    verifikat::spara_utkast(
        db,
        &klocka("2026-05-03T08:00:00Z"),
        &anna,
        id,
        None,
        &formulär("2026-04-01", "Utkast", &[(1930, "9", ""), (3001, "", "9")]),
    )
    .await
    .unwrap();
    (anna, id)
}

/// Huvudboken som text: konto, ingående balans, transaktionerna med löpande saldo och
/// utgående balans.
fn som_text(h: &Huvudbok) -> Vec<String> {
    let mut rader = Vec::new();
    for k in &h.konton {
        rader.push(format!("{} {} IB {}", k.konto, k.namn, k.ingående));
        for t in &k.transaktioner {
            rader.push(format!(
                "  {} {} {} D{} K{} = {}",
                t.nummer, t.datum, t.text, t.debet, t.kredit, t.saldo
            ));
        }
        rader.push(format!("  UB {}", k.utgående));
    }
    rader
}

#[tokio::test]
async fn huvudboken_ger_ingående_balans_löpande_saldo_och_utgående_balans_per_konto() {
    let db = Databas::tillfällig().await;
    let (anna, id) = exempel(&db).await;

    let h = bokforing::huvudbok(
        &db,
        &anna,
        id,
        &hela_året(),
        &Kontourval::tolka("1930, 3000-3999").unwrap(),
    )
    .await
    .unwrap();

    assert_eq!(h.status, Status::Inskriven);
    assert_eq!(
        som_text(&h),
        [
            "1930 Företagskonto IB 1 000,00",
            "  V1 2026-03-20 Swish D500,00 K0,00 = 1 500,00",
            "  V2 2026-03-05 Hyra D0,00 K200,00 = 1 300,00",
            "  V3 2026-04-30 Telefon D0,00 K50,00 = 1 250,00",
            "  UB 1 250,00",
            "3001 Försäljning inom Sverige, 25 % moms IB 0,00",
            "  V1 2026-03-20 Försäljning; kund \"A\" D0,00 K500,00 = -500,00",
            "  UB -500,00",
        ]
    );
}

#[tokio::test]
async fn huvudboken_från_en_period_mitt_i_året_börjar_på_saldot_före_den() {
    let db = Databas::tillfällig().await;
    let (anna, id) = exempel(&db).await;
    let april = Periodurval {
        år: ÅR_2026,
        från: Some(date(2026, 4, 1)),
        till: Some(date(2026, 4, 30)),
    };

    let h = bokforing::huvudbok(&db, &anna, id, &april, &Kontourval::alla())
        .await
        .unwrap();

    assert_eq!(
        som_text(&h),
        [
            "1930 Företagskonto IB 1 300,00",
            "  V3 2026-04-30 Telefon D0,00 K50,00 = 1 250,00",
            "  UB 1 250,00",
            "2081 Aktiekapital IB -1 000,00",
            "  UB -1 000,00",
            "3001 Försäljning inom Sverige, 25 % moms IB -500,00",
            "  UB -500,00",
            "5010 Lokalhyra IB 200,00",
            "  UB 200,00",
            "6210 Telekommunikation IB 0,00",
            "  V3 2026-04-30 Telefon D50,00 K0,00 = 50,00",
            "  UB 50,00",
        ]
    );
}

#[tokio::test]
async fn huvudboken_för_ett_år_med_preliminär_ingående_balans() {
    let db = Databas::tillfällig().await;
    let (anna, id) = exempel(&db).await;
    // Ett verifikat i 2027 skapar året; 2026 är inte stängt.
    bokför(
        &db,
        &anna,
        id,
        "2027-01-10T08:00:00Z",
        formulär("2027-01-10", "Ränta", &[(1930, "10", ""), (8310, "", "10")]),
    )
    .await;
    let år_2027 = Periodurval {
        år: date(2027, 1, 1),
        från: None,
        till: None,
    };

    let h = bokforing::huvudbok(
        &db,
        &anna,
        id,
        &år_2027,
        &Kontourval::tolka("1930, 2099, 3001").unwrap(),
    )
    .await
    .unwrap();

    assert_eq!(h.status, Status::Preliminär);
    assert_eq!(
        som_text(&h),
        [
            "1930 Företagskonto IB 1 250,00",
            "  V1 2027-01-10 Ränta D10,00 K0,00 = 1 260,00",
            "  UB 1 260,00",
            // Årets resultat 2026 (vinst 250 kr) ligger på 2099 tills året stängs.
            "2099 Årets resultat IB -250,00",
            "  UB -250,00",
        ]
    );
}
