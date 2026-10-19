//! Företagets fakturauppgifter: betalsätt, OCR, logotyp, svarsadress och standardmeddelande,
//! genom `core` mot en tillfällig databas.

use maya_core::atkomst::ÅtkomstFel;
use maya_core::fakturauppgifter::{self, FakturauppgiftsFel};
use maya_core::{Databas, Klocka, behandlingshistorik, foretag, testhjalp};
use maya_domain::fakturauppgifter::{Fakturauppgifter, Fält};
use serde_json::json;

const NU: &str = "2026-10-20T08:00:00Z";
const PNG: &[u8] = b"\x89PNG\r\n\x1a\nen liten logotyp";
const SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1 1"/>"#;

fn klocka() -> Klocka {
    Klocka::fast(NU.parse().unwrap())
}

fn ifyllda() -> Fakturauppgifter {
    Fakturauppgifter {
        bankgiro: "50501055".into(),
        plusgiro: "".into(),
        iban: "SE45 5000 0000 0583 9825 7466".into(),
        bic: "essesess".into(),
        ocr: false,
        svarsadress: "Faktura@Exempel.se".into(),
        standardmeddelande: "Hej! Här kommer fakturan.".into(),
    }
}

#[tokio::test]
async fn ett_nytt_företag_har_inga_fakturauppgifter_och_ocr_av() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;

    let f = fakturauppgifter::fakturauppgifterna(&db, &anna, id)
        .await
        .unwrap();

    assert_eq!(f.uppgifter, Fakturauppgifter::default());
    assert!(!f.uppgifter.har_betalsätt());
    assert_eq!(f.logotyp, None);
}

#[tokio::test]
async fn ändrade_fakturauppgifter_sparas_och_loggas_med_det_som_ändrades() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;

    fakturauppgifter::ändra(&db, &klocka(), &anna, id, &ifyllda())
        .await
        .unwrap();

    let f = fakturauppgifter::fakturauppgifterna(&db, &anna, id)
        .await
        .unwrap()
        .uppgifter;
    assert_eq!(f.bankgiro, "5050-1055");
    assert_eq!(f.iban, "SE45 5000 0000 0583 9825 7466");
    assert_eq!(f.bic, "ESSESESS");
    assert_eq!(f.svarsadress, "faktura@exempel.se");
    assert_eq!(f.standardmeddelande, "Hej! Här kommer fakturan.");
    assert!(f.har_betalsätt());

    let historik = behandlingshistorik::läs(&db, &anna, id).await.unwrap();
    let post = historik.last().unwrap();
    assert_eq!(post.typ, "fakturauppgifter ändrade");
    assert_eq!(post.objekt, "556036-0793");
    assert_eq!(post.aktör, anna);
    assert_eq!(post.tid.to_string(), NU);
    assert_eq!(
        post.beskrivning,
        json!({
            "bankgiro": { "från": "", "till": "5050-1055" },
            "iban": { "från": "", "till": "SE45 5000 0000 0583 9825 7466" },
            "bic": { "från": "", "till": "ESSESESS" },
            "svarsadress": { "från": "", "till": "faktura@exempel.se" },
            "standardmeddelande": { "från": "", "till": "Hej! Här kommer fakturan." },
        })
    );
    assert!(
        post.text().contains("bankgiro: ”” → ”5050-1055”"),
        "{}",
        post.text()
    );
    // OCR-inställningen ändrades inte.
    assert!(!historik.iter().any(|p| p.typ == "ocr-inställning ändrad"));
}

#[tokio::test]
async fn ocr_inställningen_loggas_som_en_egen_typ() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    let på = Fakturauppgifter {
        ocr: true,
        ..Fakturauppgifter::default()
    };

    fakturauppgifter::ändra(&db, &klocka(), &anna, id, &på)
        .await
        .unwrap();
    fakturauppgifter::ändra(&db, &klocka(), &anna, id, &Fakturauppgifter::default())
        .await
        .unwrap();

    let historik = behandlingshistorik::läs(&db, &anna, id).await.unwrap();
    let ocr: Vec<_> = historik
        .iter()
        .filter(|p| p.typ == "ocr-inställning ändrad")
        .collect();
    assert_eq!(ocr.len(), 2);
    assert_eq!(ocr[0].beskrivning, json!({ "från": false, "till": true }));
    assert_eq!(ocr[0].text(), "OCR-referens på");
    assert_eq!(ocr[1].text(), "OCR-referens av");
    assert!(!historik.iter().any(|p| p.typ == "fakturauppgifter ändrade"));
    assert!(
        !fakturauppgifter::fakturauppgifterna(&db, &anna, id)
            .await
            .unwrap()
            .uppgifter
            .ocr
    );
}

#[tokio::test]
async fn utan_ändring_loggas_inget() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    fakturauppgifter::ändra(&db, &klocka(), &anna, id, &ifyllda())
        .await
        .unwrap();
    let före = behandlingshistorik::läs(&db, &anna, id)
        .await
        .unwrap()
        .len();

    // Samma uppgifter, skrivna på ett annat sätt.
    let samma = Fakturauppgifter {
        bankgiro: "5050-1055".into(),
        bic: "ESSESESS".into(),
        ..ifyllda()
    };
    fakturauppgifter::ändra(&db, &klocka(), &anna, id, &samma)
        .await
        .unwrap();

    assert_eq!(
        behandlingshistorik::läs(&db, &anna, id)
            .await
            .unwrap()
            .len(),
        före
    );
}

#[tokio::test]
async fn felaktiga_betalsätt_nekas_på_servern_med_samma_koder_som_formuläret() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    let fel = Fakturauppgifter {
        bankgiro: "5050-1056".into(),
        plusgiro: "12".into(),
        iban: "SE4550000000058398257466".into(),
        ..Fakturauppgifter::default()
    };

    let svar = fakturauppgifter::ändra(&db, &klocka(), &anna, id, &fel).await;

    let Err(FakturauppgiftsFel::Ogiltigt(koder)) = svar else {
        panic!("{svar:?}");
    };
    let koder: Vec<_> = koder.iter().map(|f| (f.fält, f.kod)).collect();
    assert_eq!(
        koder,
        [
            (Fält::Bankgiro, "bankgiro_kontrollsiffra"),
            (Fält::Plusgiro, "plusgiro_kontrollsiffra"),
            (Fält::Bic, "bic_saknas"),
        ]
    );
    assert_eq!(
        fakturauppgifter::fakturauppgifterna(&db, &anna, id)
            .await
            .unwrap()
            .uppgifter,
        Fakturauppgifter::default()
    );
}

#[tokio::test]
async fn logotypen_lagras_kan_bytas_och_tas_bort_och_varje_ändring_loggas() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;

    fakturauppgifter::byt_logotyp(&db, &klocka(), &anna, id, "logga.png", PNG)
        .await
        .unwrap();
    let f = fakturauppgifter::fakturauppgifterna(&db, &anna, id)
        .await
        .unwrap();
    let logotyp = f.logotyp.unwrap();
    assert_eq!(logotyp.filnamn, "logga.png");
    assert_eq!(logotyp.mime, "image/png");
    assert_eq!(logotyp.storlek, PNG.len() as u64);
    let fil = fakturauppgifter::logotypen(&db, &anna, id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fil.innehåll, PNG);
    assert_eq!(fil.mime, "image/png");

    fakturauppgifter::byt_logotyp(&db, &klocka(), &anna, id, "../logga.svg", SVG)
        .await
        .unwrap();
    let fil = fakturauppgifter::logotypen(&db, &anna, id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fil.innehåll, SVG);
    assert_eq!(fil.mime, "image/svg+xml");
    assert_eq!(fil.filnamn, "logga.svg");

    fakturauppgifter::ta_bort_logotyp(&db, &klocka(), &anna, id)
        .await
        .unwrap();
    assert_eq!(
        fakturauppgifter::logotypen(&db, &anna, id).await.unwrap(),
        None
    );
    // Att ta bort en logotyp som inte finns ändrar inget.
    fakturauppgifter::ta_bort_logotyp(&db, &klocka(), &anna, id)
        .await
        .unwrap();

    let historik = behandlingshistorik::läs(&db, &anna, id).await.unwrap();
    let poster: Vec<_> = historik
        .iter()
        .filter(|p| p.typ == "fakturauppgifter ändrade")
        .collect();
    assert_eq!(poster.len(), 3);
    let png_sha = &poster[0].beskrivning["logotyp"]["till"]["sha256"];
    assert_eq!(png_sha.as_str().unwrap().len(), 64);
    assert_eq!(poster[0].beskrivning["logotyp"]["från"], json!(null));
    assert_eq!(poster[0].text(), "logotyp: ingen → ”logga.png”");
    assert_eq!(&poster[1].beskrivning["logotyp"]["från"]["sha256"], png_sha);
    assert_eq!(poster[1].text(), "logotyp: ”logga.png” → ”logga.svg”");
    assert_eq!(poster[2].beskrivning["logotyp"]["till"], json!(null));
    assert_eq!(poster[2].text(), "logotyp: ”logga.svg” → ingen");
}

#[tokio::test]
async fn en_logotyp_i_ett_annat_format_än_png_jpg_eller_svg_nekas() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;

    for (namn, fil) in [
        ("logga.gif", &b"GIF89a..."[..]),
        ("logga.png", b"%PDF-1.7 som png"),
        ("tom.png", b""),
    ] {
        let svar = fakturauppgifter::byt_logotyp(&db, &klocka(), &anna, id, namn, fil).await;
        let Err(FakturauppgiftsFel::Ogiltigt(fel)) = svar else {
            panic!("{namn}: {svar:?}");
        };
        assert_eq!(fel[0].fält, Fält::Logotyp);
    }

    assert_eq!(
        fakturauppgifter::logotypen(&db, &anna, id).await.unwrap(),
        None
    );
    let historik = behandlingshistorik::läs(&db, &anna, id).await.unwrap();
    assert!(!historik.iter().any(|p| p.typ == "fakturauppgifter ändrade"));
}

#[tokio::test]
async fn den_som_saknar_åtkomst_får_inte_läsa_eller_ändra_fakturauppgifterna() {
    let db = Databas::tillfällig().await;
    let (_, id) = testhjalp::företag(&db).await;
    let bo = testhjalp::användare(&db, "bo@example.se").await;

    assert!(matches!(
        fakturauppgifter::fakturauppgifterna(&db, &bo, id).await,
        Err(ÅtkomstFel::IngenÅtkomst)
    ));
    assert!(matches!(
        fakturauppgifter::logotypen(&db, &bo, id).await,
        Err(ÅtkomstFel::IngenÅtkomst)
    ));
    assert!(matches!(
        fakturauppgifter::ändra(&db, &klocka(), &bo, id, &ifyllda()).await,
        Err(FakturauppgiftsFel::Åtkomst(ÅtkomstFel::IngenÅtkomst))
    ));
    assert!(matches!(
        fakturauppgifter::byt_logotyp(&db, &klocka(), &bo, id, "logga.png", PNG).await,
        Err(FakturauppgiftsFel::Åtkomst(ÅtkomstFel::IngenÅtkomst))
    ));
    assert!(matches!(
        fakturauppgifter::ta_bort_logotyp(&db, &klocka(), &bo, id).await,
        Err(FakturauppgiftsFel::Åtkomst(ÅtkomstFel::IngenÅtkomst))
    ));
}

#[tokio::test]
async fn ett_företag_med_fakturauppgifter_och_logotyp_kan_raderas() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    fakturauppgifter::ändra(&db, &klocka(), &anna, id, &ifyllda())
        .await
        .unwrap();
    fakturauppgifter::byt_logotyp(&db, &klocka(), &anna, id, "logga.png", PNG)
        .await
        .unwrap();

    // Fakturauppgifterna pekar på företaget; en rad som blev kvar fäller raderingen vid COMMIT.
    foretag::radera(&db, &klocka(), &anna, id).await.unwrap();

    assert!(foretag::mina_företag(&db, &anna).await.unwrap().is_empty());
}
