//! Att göra: nyckeltalen, utkasten och de okopplade underlagen, genom `core` mot en tillfällig
//! databas.

use jiff::civil::date;
use maya_core::att_gora::{self, Sak};
use maya_core::foretag;
use maya_core::underlag;
use maya_core::verifikat;
use maya_core::{Databas, Klocka, saldo, testhjalp};
use maya_domain::att_gora::Frist;
use maya_domain::foretag::NyttFöretag;
use maya_domain::ingaende_balans::{NyIngåendeBalans, NySaldorad};
use maya_domain::verifikat::{NyRad, NyttVerifikat};

fn klocka(nu: &str) -> Klocka {
    Klocka::fast(nu.parse().unwrap())
}

/// Tisdag 14 oktober 2026, förmiddag i Stockholm.
fn i_dag() -> Klocka {
    klocka("2026-10-14T08:00:00Z")
}

fn rad(konto: &str, debet: &str, kredit: &str) -> NyRad {
    NyRad {
        konto: konto.into(),
        debet: debet.into(),
        kredit: kredit.into(),
        text: String::new(),
    }
}

fn formulär(datum: &str, beskrivning: &str, rader: Vec<NyRad>) -> NyttVerifikat {
    NyttVerifikat {
        datum: datum.into(),
        beskrivning: beskrivning.into(),
        rader,
    }
}

#[tokio::test]
async fn nyckeltalen_räknas_bara_ur_bokförda_verifikat_till_och_med_i_dag() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    saldo::ändra_ingående_balans(
        &db,
        &i_dag(),
        &anna,
        id,
        date(2026, 1, 1),
        &NyIngåendeBalans {
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
        },
    )
    .await
    .unwrap();
    testhjalp::bokför(
        &db,
        &anna,
        id,
        "2026-03-14",
        &[(1930, 1_250_000, 0), (3001, 0, 1_250_000)],
    )
    .await;
    testhjalp::bokför(
        &db,
        &anna,
        id,
        "2026-05-02",
        &[(5410, 200_000, 0), (1930, 0, 200_000)],
    )
    .await;
    // Årets resultat på 8999 räknas inte in i resultatet.
    testhjalp::bokför(
        &db,
        &anna,
        id,
        "2026-06-30",
        &[(8999, 100_000, 0), (2099, 0, 100_000)],
    )
    .await;
    // Daterat efter i dag: inte med "per i dag" eller "hittills".
    testhjalp::bokför(
        &db,
        &anna,
        id,
        "2026-11-20",
        &[(1930, 50_000, 0), (3001, 0, 50_000)],
    )
    .await;
    // Ett utkast räknas aldrig.
    verifikat::spara_utkast(
        &db,
        &i_dag(),
        &anna,
        id,
        None,
        &formulär(
            "2026-10-01",
            "Utkast",
            vec![rad("1930", "999", ""), rad("3001", "", "999")],
        ),
    )
    .await
    .unwrap();

    let a = att_gora::att_göra(&db, &i_dag(), &testhjalp::config(), &anna, id)
        .await
        .unwrap();

    assert_eq!(a.idag, date(2026, 10, 14));
    assert_eq!(a.nyckeltal.företagskontot.öre(), 2_050_000);
    // Vinst är positiv.
    assert_eq!(a.nyckeltal.resultat_i_år.öre(), 1_050_000);
    assert_eq!(
        a.nyckeltal.år.map(|å| (å.start(), å.slut())),
        Some((date(2026, 1, 1), date(2026, 12, 31)))
    );

    // Ett nytt år: företagskontot bär med sig fjolårets saldo, resultatet börjar om.
    testhjalp::bokför(
        &db,
        &anna,
        id,
        "2027-01-10",
        &[(1930, 30_000, 0), (3001, 0, 30_000)],
    )
    .await;
    let a = att_gora::att_göra(
        &db,
        &klocka("2027-02-01T08:00:00Z"),
        &testhjalp::config(),
        &anna,
        id,
    )
    .await
    .unwrap();
    assert_eq!(a.nyckeltal.företagskontot.öre(), 2_130_000);
    assert_eq!(a.nyckeltal.resultat_i_år.öre(), 30_000);
}

#[tokio::test]
async fn utkasten_och_de_okopplade_underlagen_är_saker_att_göra() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    let k = i_dag();
    testhjalp::backup(&db, &k).await;
    let kassa = verifikat::spara_utkast(
        &db,
        &k,
        &anna,
        id,
        None,
        &formulär(
            "2026-10-12",
            "Kontant försäljning",
            vec![rad("1910", "500", ""), rad("3001", "", "500")],
        ),
    )
    .await
    .unwrap();
    let kopplat = underlag::ladda_upp(
        &db,
        &k,
        &anna,
        id,
        "kvitto.pdf",
        "",
        b"%PDF-1.7\nett\n%%EOF\n",
    )
    .await
    .unwrap();
    let gammalt = verifikat::spara_utkast_med_underlag(
        &db,
        &k,
        &anna,
        id,
        None,
        &formulär(
            "2026-07-14",
            "Juli",
            vec![rad("5410", "100", ""), rad("1930", "", "100")],
        ),
        &[kopplat.id],
    )
    .await
    .unwrap();
    let okopplat = underlag::ladda_upp(
        &db,
        &k,
        &anna,
        id,
        "faktura.pdf",
        "",
        b"%PDF-1.7\ntva\n%%EOF\n",
    )
    .await
    .unwrap();

    let a = att_gora::att_göra(&db, &k, &testhjalp::config(), &anna, id)
        .await
        .unwrap();

    let mut utkast = Vec::new();
    let mut underlagen = Vec::new();
    for sak in &a.saker {
        match sak {
            Sak::Utkast(u) => utkast.push(u),
            Sak::Underlag(u) => underlagen.push(u.id),
            Sak::Driftvarning(v) => panic!("ingen driftvarning väntad: {v:?}"),
        }
    }
    assert_eq!(underlagen, vec![okopplat.id]);
    let kassans = utkast.iter().find(|u| u.id == kassa).unwrap();
    assert_eq!(
        kassans.frist(a.idag),
        Frist::Kontantfrist {
            senast: date(2026, 10, 13),
            försenad: true
        }
    );
    assert_eq!(kassans.underlag, 0);
    let julis = utkast.iter().find(|u| u.id == gammalt).unwrap();
    assert_eq!(julis.frist(a.idag), Frist::Femtiodagar { dagar: 75 });
    assert_eq!(julis.underlag, 1);
    assert_eq!(julis.beskrivning, "Juli");
    assert_eq!(a.saker.len(), 3);

    // Per företag, för toppmenyn och företagsväljaren.
    let annat = foretag::lägg_upp(
        &db,
        &k,
        &anna,
        &NyttFöretag {
            orgnr: "559900-0006".into(),
            namn: "Annat AB".into(),
            första_år_start: "2026-01-01".into(),
            första_år_slut: "2026-12-31".into(),
            bokföringsmetod: "faktura".into(),
            momsperiod: "kvartal".into(),
        },
    )
    .await
    .unwrap();
    let antal = att_gora::antal(&db, &k, &testhjalp::config(), &anna)
        .await
        .unwrap();
    assert_eq!(antal.get(&id), Some(&3));
    assert_eq!(antal.get(&annat), Some(&0));

    // Den som saknar åtkomst ser inget.
    let bertil = testhjalp::användare(&db, "bertil@example.se").await;
    assert!(
        att_gora::att_göra(&db, &k, &testhjalp::config(), &bertil, id)
            .await
            .is_err()
    );
    assert!(
        att_gora::antal(&db, &k, &testhjalp::config(), &bertil)
            .await
            .unwrap()
            .is_empty()
    );
}
