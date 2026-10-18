//! Kontrollsummorna som backupen, migreringarna och återställningen jämför (A4, A5).

use jiff::civil::date;
use maya_core::kontrollsumma::{Kontosumma, Kontrollsummor, Underlagshash, Verifikatantal};
use maya_core::{Databas, Klocka, testhjalp, underlag, verifikat};
use maya_domain::verifikat::{NyRad, NyttVerifikat};

const ORGNR: &str = "5560360793";
// SHA-256 av "kvitto 1", räknad med Pythons hashlib.
const KVITTO_HASH: &str = "c828a89c94342aca02f01ddc914aa85f99b513b57762b426677df7460e62b161";

fn utkast() -> NyttVerifikat {
    NyttVerifikat {
        datum: "2026-05-01".into(),
        beskrivning: "Utkast".into(),
        rader: vec![NyRad {
            konto: "1930".into(),
            debet: "999".into(),
            kredit: String::new(),
            text: String::new(),
        }],
    }
}

#[tokio::test]
async fn kontrollsummorna_räknar_bokförda_verifikat_per_serie_och_år_och_konto() {
    let db = Databas::tillfällig().await;
    let (anna, företag) = testhjalp::företag(&db).await;
    testhjalp::bokför(
        &db,
        &anna,
        företag,
        "2026-03-14",
        &[(1930, 12_500, 0), (3001, 0, 12_500)],
    )
    .await;
    testhjalp::bokför(
        &db,
        &anna,
        företag,
        "2026-04-01",
        &[(1930, 0, 2_000), (1510, 2_000, 0)],
    )
    .await;
    // Nästa räkenskapsår skapas av bokföringen.
    testhjalp::bokför(
        &db,
        &anna,
        företag,
        "2027-01-10",
        &[(1930, 100, 0), (3001, 0, 100)],
    )
    .await;
    // Ett utkast räknas inte.
    verifikat::spara_utkast(&db, &Klocka::system(), &anna, företag, None, &utkast())
        .await
        .unwrap();

    let summor = Kontrollsummor::räkna(&db).await.unwrap();

    let år26 = date(2026, 1, 1);
    let år27 = date(2027, 1, 1);
    assert_eq!(
        summor.verifikat,
        vec![
            Verifikatantal {
                orgnr: ORGNR.into(),
                räkenskapsår: år26,
                serie: "V".into(),
                antal: 2
            },
            Verifikatantal {
                orgnr: ORGNR.into(),
                räkenskapsår: år27,
                serie: "V".into(),
                antal: 1
            },
        ]
    );
    let konto = |år, konto, debet, kredit| Kontosumma {
        orgnr: ORGNR.into(),
        räkenskapsår: år,
        konto,
        debet,
        kredit,
    };
    assert_eq!(
        summor.konton,
        vec![
            konto(år26, 1510, 2_000, 0),
            konto(år26, 1930, 12_500, 2_000),
            konto(år26, 3001, 0, 12_500),
            konto(år27, 1930, 100, 0),
            konto(år27, 3001, 0, 100),
        ]
    );
    assert_eq!(summor.bokförda_rader.len(), 64, "SHA-256 i hex");
}

#[tokio::test]
async fn hashen_över_bokförda_rader_ändras_bara_av_bokföring() {
    let db = Databas::tillfällig().await;
    let (anna, företag) = testhjalp::företag(&db).await;
    testhjalp::bokför(
        &db,
        &anna,
        företag,
        "2026-03-14",
        &[(1930, 12_500, 0), (3001, 0, 12_500)],
    )
    .await;
    let före = Kontrollsummor::räkna(&db).await.unwrap();

    verifikat::spara_utkast(&db, &Klocka::system(), &anna, företag, None, &utkast())
        .await
        .unwrap();
    let efter_utkast = Kontrollsummor::räkna(&db).await.unwrap();
    assert_eq!(efter_utkast, före);
    assert!(före.avvikelser(&efter_utkast).is_empty());

    testhjalp::bokför(
        &db,
        &anna,
        företag,
        "2026-03-15",
        &[(1930, 1, 0), (3001, 0, 1)],
    )
    .await;
    let efter = Kontrollsummor::räkna(&db).await.unwrap();
    assert_ne!(efter.bokförda_rader, före.bokförda_rader);
    let avvikelser = före.avvikelser(&efter);
    assert!(
        avvikelser
            .iter()
            .any(|a| a == "antal verifikat i serie V för 5560360793 räkenskapsåret som börjar 2026-01-01: 1 före, 2 efter"),
        "{avvikelser:?}"
    );
    assert!(
        avvikelser
            .iter()
            .any(|a| a == "hashen över bokförda rader skiljer sig"),
        "{avvikelser:?}"
    );
}

#[tokio::test]
async fn underlagens_hashar_räknas_ur_innehållet() {
    let db = Databas::tillfällig().await;
    let (anna, företag) = testhjalp::företag(&db).await;
    let uppladdat = underlag::ladda_upp(
        &db,
        &Klocka::system(),
        &anna,
        företag,
        "kvitto.txt",
        "text/plain",
        b"kvitto 1",
    )
    .await
    .unwrap();

    let summor = Kontrollsummor::räkna(&db).await.unwrap();
    assert_eq!(
        summor.underlag,
        vec![Underlagshash {
            orgnr: ORGNR.into(),
            id: uppladdat.id.to_string().parse().unwrap(),
            sha256: KVITTO_HASH.into(),
        }]
    );
}

#[tokio::test]
async fn kontrollsummorna_går_att_spara_som_json_och_läsa_tillbaka() {
    let db = Databas::tillfällig().await;
    let (anna, företag) = testhjalp::företag(&db).await;
    testhjalp::bokför(
        &db,
        &anna,
        företag,
        "2026-03-14",
        &[(1930, 12_500, 0), (3001, 0, 12_500)],
    )
    .await;
    let summor = Kontrollsummor::räkna(&db).await.unwrap();
    let json = serde_json::to_string(&summor).unwrap();
    assert_eq!(
        serde_json::from_str::<Kontrollsummor>(&json).unwrap(),
        summor
    );
}
