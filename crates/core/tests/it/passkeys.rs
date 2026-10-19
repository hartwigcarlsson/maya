//! En inloggad användare hanterar sina passkeys och sessioner. Hantering av passkeys kräver en
//! färsk passkey: en passkey använd i sessionen de senaste 5 minuterna.

use jiff::{SignedDuration, Timestamp};
use maya_core::aktor::Aktör;
use maya_core::inloggning::{self, Inloggad, InloggningsFel, NyPasskey};
use maya_core::{Databas, Klocka};

const START: &str = "2026-10-18T08:00:00Z";

fn klocka(efter: SignedDuration) -> Klocka {
    Klocka::fast(START.parse::<Timestamp>().unwrap() + efter)
}

fn nu() -> Klocka {
    klocka(SignedDuration::ZERO)
}

fn minuter(m: i64) -> SignedDuration {
    SignedDuration::from_mins(m)
}

fn passkey(id: &[u8], namn: &str) -> NyPasskey {
    NyPasskey {
        credential_id: id.to_vec(),
        namn: namn.into(),
        passkey: format!("{{\"cred\":{id:?}}}"),
    }
}

/// Anna har registrerat passkeyn "MacBook" och är inloggad sedan START. Ger sessionstoken.
async fn anna(db: &Databas) -> String {
    let länk = inloggning::skapa_första_användare(db, &nu(), &Aktör::cli(), "anna@example.se")
        .await
        .unwrap();
    inloggning::slutför_registrering(db, &nu(), länk.token(), passkey(b"ett", "MacBook"), None)
        .await
        .unwrap()
        .token()
        .to_string()
}

async fn inloggad(db: &Databas, klocka: &Klocka, token: &str) -> Inloggad {
    inloggning::session(db, klocka, token)
        .await
        .unwrap()
        .expect("sessionen gäller")
}

async fn namn(db: &Databas, inloggad: &Inloggad) -> Vec<String> {
    inloggning::passkeys(db, inloggad)
        .await
        .unwrap()
        .into_iter()
        .map(|p| p.namn)
        .collect()
}

#[tokio::test]
async fn anvandaren_kan_lagga_till_namnge_och_ta_bort_passkeys() {
    let db = Databas::tillfällig().await;
    let token = anna(&db).await;
    let anna = inloggad(&db, &nu(), &token).await;

    inloggning::lägg_till_passkey(&db, &nu(), &anna, passkey(b"tva", "iPhone"))
        .await
        .unwrap();
    assert_eq!(namn(&db, &anna).await, ["MacBook", "iPhone"]);

    let iphone = inloggning::passkeys(&db, &anna).await.unwrap()[1]
        .id
        .clone();
    inloggning::byt_namn_på_passkey(&db, &nu(), &anna, &iphone, "  Annas iPhone ")
        .await
        .unwrap();
    assert_eq!(namn(&db, &anna).await, ["MacBook", "Annas iPhone"]);

    let macbook = inloggning::passkeys(&db, &anna).await.unwrap()[0]
        .id
        .clone();
    inloggning::ta_bort_passkey(&db, &nu(), &anna, &macbook)
        .await
        .unwrap();
    assert_eq!(namn(&db, &anna).await, ["Annas iPhone"]);
}

#[tokio::test]
async fn den_sista_passkeyn_gar_inte_att_ta_bort() {
    let db = Databas::tillfällig().await;
    let token = anna(&db).await;
    let anna = inloggad(&db, &nu(), &token).await;
    let macbook = inloggning::passkeys(&db, &anna).await.unwrap()[0]
        .id
        .clone();

    let err = inloggning::ta_bort_passkey(&db, &nu(), &anna, &macbook)
        .await
        .unwrap_err();

    assert!(matches!(err, InloggningsFel::SistaPasskeyn), "{err}");
    assert_eq!(namn(&db, &anna).await, ["MacBook"]);
}

#[tokio::test]
async fn hantering_av_passkeys_kraver_en_passkey_anvand_de_senaste_5_minuterna() {
    let db = Databas::tillfällig().await;
    let token = anna(&db).await;
    inloggning::lägg_till_passkey(
        &db,
        &nu(),
        &inloggad(&db, &nu(), &token).await,
        passkey(b"tva", "iPhone"),
    )
    .await
    .unwrap();

    let sen = klocka(minuter(5));
    let anna = inloggad(&db, &sen, &token).await;
    assert!(!anna.har_färsk_passkey(&sen));
    let iphone = inloggning::passkeys(&db, &anna).await.unwrap()[1]
        .id
        .clone();
    let försök = [
        inloggning::ny_passkey(&db, &sen, &anna).await.map(|_| ()),
        inloggning::lägg_till_passkey(&db, &sen, &anna, passkey(b"tre", "iPad")).await,
        inloggning::byt_namn_på_passkey(&db, &sen, &anna, &iphone, "Nytt").await,
        inloggning::ta_bort_passkey(&db, &sen, &anna, &iphone).await,
    ];
    for resultat in försök {
        assert!(
            matches!(resultat, Err(InloggningsFel::KräverFärskPasskey)),
            "{resultat:?}"
        );
    }
    assert_eq!(namn(&db, &anna).await, ["MacBook", "iPhone"]);

    // Användaren bekräftar med en passkey i sessionen: passkeyn är färsk i 5 minuter till.
    inloggning::bekräfta_passkey(&db, &sen, &anna, b"tva", None)
        .await
        .unwrap();
    let senare = klocka(minuter(9));
    let anna = inloggad(&db, &senare, &token).await;
    assert!(anna.har_färsk_passkey(&senare));
    inloggning::byt_namn_på_passkey(&db, &senare, &anna, &iphone, "Nytt")
        .await
        .unwrap();
    assert_eq!(namn(&db, &anna).await, ["MacBook", "Nytt"]);
}

#[tokio::test]
async fn bekraftelse_med_nagon_annans_passkey_vagras() {
    let db = Databas::tillfällig().await;
    let token = anna(&db).await;
    let sen = klocka(minuter(10));
    let anna = inloggad(&db, &sen, &token).await;

    let err = inloggning::bekräfta_passkey(&db, &sen, &anna, b"okand", None)
        .await
        .unwrap_err();

    assert!(matches!(err, InloggningsFel::OkändPasskey), "{err}");
    assert!(!inloggad(&db, &sen, &token).await.har_färsk_passkey(&sen));
}

#[tokio::test]
async fn sessionslistan_visar_sessionerna_och_en_avslutad_session_ar_utloggad_direkt() {
    let db = Databas::tillfällig().await;
    let token = anna(&db).await;
    let anna = inloggad(&db, &nu(), &token).await;
    let annan = inloggning::logga_in(
        &db,
        &klocka(minuter(1)),
        anna.användare.id,
        b"ett",
        None,
        Some("Firefox på Linux"),
    )
    .await
    .unwrap();

    let sessioner = inloggning::sessioner(&db, &nu(), &anna).await.unwrap();
    assert_eq!(sessioner.len(), 2);
    assert_eq!(sessioner[0].enhet.as_deref(), Some("Firefox på Linux"));
    assert!(!sessioner[0].denna);
    assert!(sessioner[1].denna);
    assert_eq!(sessioner[1].id, anna.session_id());

    inloggning::avsluta_session(&db, &anna, &sessioner[0].id)
        .await
        .unwrap();

    assert!(
        inloggning::session(&db, &klocka(minuter(2)), annan.token())
            .await
            .unwrap()
            .is_none()
    );
    let kvar = inloggning::sessioner(&db, &nu(), &anna).await.unwrap();
    assert_eq!(kvar.len(), 1);
    assert!(kvar[0].denna);
}

#[tokio::test]
async fn sessionslistan_visar_inte_utgångna_sessioner() {
    let db = Databas::tillfällig().await;
    let token = anna(&db).await;
    let anna = inloggad(&db, &nu(), &token).await;
    // En session som inte har använts på åtta dagar.
    inloggning::logga_in(
        &db,
        &klocka(minuter(-8 * 24 * 60)),
        anna.användare.id,
        b"ett",
        None,
        Some("Gammal dator"),
    )
    .await
    .unwrap();

    let sessioner = inloggning::sessioner(&db, &nu(), &anna).await.unwrap();

    assert_eq!(sessioner.len(), 1, "{sessioner:?}");
    assert!(sessioner[0].denna);
    // Efter 30 dagar har också den aktiva sessionen gått ut.
    let senare = klocka(minuter(31 * 24 * 60));
    assert!(
        inloggning::sessioner(&db, &senare, &anna)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn en_annan_anvandares_session_gar_inte_att_avsluta() {
    let db = Databas::tillfällig().await;
    let token = anna(&db).await;
    let anna = inloggad(&db, &nu(), &token).await;

    let err = inloggning::avsluta_session(&db, &anna, &"00".repeat(32))
        .await
        .unwrap_err();

    assert!(matches!(err, InloggningsFel::OkändSession), "{err}");
}
