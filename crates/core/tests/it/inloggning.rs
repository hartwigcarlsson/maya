use jiff::{SignedDuration, Timestamp};
use maya_core::aktor::Aktör;
use maya_core::inloggning::{self, InloggningsFel, NyPasskey};
use maya_core::{Databas, Klocka};

const START: &str = "2026-10-18T08:00:00Z";

fn klocka(efter: SignedDuration) -> Klocka {
    Klocka::fast(START.parse::<Timestamp>().unwrap() + efter)
}

fn nu() -> Klocka {
    klocka(SignedDuration::ZERO)
}

fn timmar(h: i64) -> SignedDuration {
    SignedDuration::from_hours(h)
}

fn dagar(d: i64) -> SignedDuration {
    timmar(24 * d)
}

fn passkey(id: &[u8]) -> NyPasskey {
    NyPasskey {
        credential_id: id.to_vec(),
        namn: "MacBook".into(),
        passkey: format!("{{\"cred\":{id:?}}}"),
    }
}

#[tokio::test]
async fn innan_nagon_anvandare_finns_sager_core_det() {
    let db = Databas::tillfällig().await;

    assert!(!inloggning::finns_användare(&db).await.unwrap());
}

#[tokio::test]
async fn cli_skapar_forsta_anvandaren_med_en_registreringslank_som_galler_i_24_timmar() {
    let db = Databas::tillfällig().await;

    let länk = inloggning::skapa_första_användare(&db, &nu(), &Aktör::cli(), "anna@example.se")
        .await
        .unwrap();

    assert!(inloggning::finns_användare(&db).await.unwrap());
    assert_eq!(
        länk.giltig_till,
        START.parse::<Timestamp>().unwrap() + timmar(24)
    );
    let registrering = inloggning::registrering(&db, &klocka(timmar(23)), länk.token())
        .await
        .unwrap();
    assert_eq!(registrering.användare.epost, "anna@example.se");
    assert!(registrering.befintliga_passkeys.is_empty());

    let för_sent = inloggning::registrering(&db, &klocka(timmar(25)), länk.token()).await;
    assert!(matches!(för_sent, Err(InloggningsFel::OgiltigLänk)));
}

#[tokio::test]
async fn bara_cli_far_skapa_anvandare() {
    let db = Databas::tillfällig().await;

    for aktör in [Aktör::maya()] {
        let err = inloggning::skapa_första_användare(&db, &nu(), &aktör, "anna@example.se")
            .await
            .unwrap_err();
        assert!(matches!(err, InloggningsFel::KanalenFårInte), "{err}");
    }
    assert!(!inloggning::finns_användare(&db).await.unwrap());
}

#[tokio::test]
async fn den_forsta_anvandaren_kan_bara_skapas_en_gang() {
    let db = Databas::tillfällig().await;
    inloggning::skapa_första_användare(&db, &nu(), &Aktör::cli(), "anna@example.se")
        .await
        .unwrap();

    let err = inloggning::skapa_första_användare(&db, &nu(), &Aktör::cli(), "bo@example.se")
        .await
        .unwrap_err();

    assert!(matches!(err, InloggningsFel::AnvändareFinns), "{err}");
}

#[tokio::test]
async fn en_ogiltig_e_postadress_vagras() {
    let db = Databas::tillfällig().await;

    let err = inloggning::skapa_första_användare(&db, &nu(), &Aktör::cli(), "anna")
        .await
        .unwrap_err();

    assert!(matches!(err, InloggningsFel::OgiltigEpost(_)), "{err}");
}

#[tokio::test]
async fn registreringslanken_ger_en_passkey_och_en_session_och_kan_bara_anvandas_en_gang() {
    let db = Databas::tillfällig().await;
    let länk = inloggning::skapa_första_användare(&db, &nu(), &Aktör::cli(), "Anna@Example.se")
        .await
        .unwrap();

    let session =
        inloggning::slutför_registrering(&db, &nu(), länk.token(), passkey(b"ett"), None)
            .await
            .unwrap();

    let inloggad = inloggning::session(&db, &nu(), session.token())
        .await
        .unwrap()
        .expect("inloggad");
    assert_eq!(inloggad.användare.epost, "anna@example.se");
    assert_eq!(inloggad.aktör, Aktör::webb(inloggad.användare.id));
    assert_eq!(inloggad.passkey_använd, nu().nu());

    let igen =
        inloggning::slutför_registrering(&db, &nu(), länk.token(), passkey(b"tva"), None).await;
    assert!(matches!(igen, Err(InloggningsFel::OgiltigLänk)));
    let igen = inloggning::registrering(&db, &nu(), länk.token()).await;
    assert!(matches!(igen, Err(InloggningsFel::OgiltigLänk)));
}

#[tokio::test]
async fn inloggning_borjar_med_e_post_och_ger_anvandarens_passkeys() {
    let db = Databas::tillfällig().await;
    let länk = inloggning::skapa_första_användare(&db, &nu(), &Aktör::cli(), "anna@example.se")
        .await
        .unwrap();
    inloggning::slutför_registrering(&db, &nu(), länk.token(), passkey(b"ett"), None)
        .await
        .unwrap();

    let grund = inloggning::inloggning_för(&db, " ANNA@example.se ")
        .await
        .unwrap()
        .expect("användaren finns");
    assert_eq!(grund.passkeys, vec![passkey(b"ett").passkey]);
    assert!(
        inloggning::inloggning_för(&db, "okand@example.se")
            .await
            .unwrap()
            .is_none()
    );

    let session = inloggning::logga_in(
        &db,
        &klocka(timmar(1)),
        grund.användare.id,
        b"ett",
        Some("{\"uppdaterad\":true}".into()),
        None,
    )
    .await
    .unwrap();
    let inloggad = inloggning::session(&db, &klocka(timmar(1)), session.token())
        .await
        .unwrap()
        .expect("inloggad");
    assert_eq!(inloggad.användare.id, grund.användare.id);
    assert_eq!(inloggad.passkey_använd, klocka(timmar(1)).nu());

    let grund = inloggning::inloggning_för(&db, "anna@example.se")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(grund.passkeys, vec!["{\"uppdaterad\":true}".to_string()]);
}

#[tokio::test]
async fn inloggning_med_nagon_annans_passkey_vagras() {
    let db = Databas::tillfällig().await;
    let länk = inloggning::skapa_första_användare(&db, &nu(), &Aktör::cli(), "anna@example.se")
        .await
        .unwrap();
    inloggning::slutför_registrering(&db, &nu(), länk.token(), passkey(b"ett"), None)
        .await
        .unwrap();
    let grund = inloggning::inloggning_för(&db, "anna@example.se")
        .await
        .unwrap()
        .unwrap();

    let err = inloggning::logga_in(&db, &nu(), grund.användare.id, b"okand", None, None)
        .await
        .unwrap_err();

    assert!(matches!(err, InloggningsFel::OkändPasskey), "{err}");
}

#[tokio::test]
async fn sessionen_loper_ut_efter_7_dagar_utan_aktivitet() {
    let db = Databas::tillfällig().await;
    let session = inloggad_session(&db).await;

    assert!(
        inloggning::session(&db, &klocka(dagar(7) - timmar(1)), &session)
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        inloggning::session(&db, &klocka(dagar(14) - timmar(2)), &session)
            .await
            .unwrap()
            .is_some(),
        "aktivitet förlänger sessionen"
    );
    assert!(
        inloggning::session(&db, &klocka(dagar(21)), &session)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        inloggning::session(&db, &klocka(dagar(15)), &session)
            .await
            .unwrap()
            .is_none(),
        "en utgången session kommer inte tillbaka"
    );
}

#[tokio::test]
async fn sessionen_loper_ut_senast_efter_30_dagar_aven_med_aktivitet() {
    let db = Databas::tillfällig().await;
    let session = inloggad_session(&db).await;

    for dag in [6, 12, 18, 24, 29] {
        assert!(
            inloggning::session(&db, &klocka(dagar(dag)), &session)
                .await
                .unwrap()
                .is_some(),
            "dag {dag}"
        );
    }
    assert!(
        inloggning::session(&db, &klocka(dagar(30)), &session)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn utloggning_avslutar_sessionen() {
    let db = Databas::tillfällig().await;
    let session = inloggad_session(&db).await;

    inloggning::logga_ut(&db, &session).await.unwrap();

    assert!(
        inloggning::session(&db, &nu(), &session)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn en_okand_sessionstoken_ger_ingen_inloggning() {
    let db = Databas::tillfällig().await;

    assert!(
        inloggning::session(&db, &nu(), "inte-en-token")
            .await
            .unwrap()
            .is_none()
    );
}

async fn inloggad_session(db: &Databas) -> String {
    let länk = inloggning::skapa_första_användare(db, &nu(), &Aktör::cli(), "anna@example.se")
        .await
        .unwrap();
    inloggning::slutför_registrering(db, &nu(), länk.token(), passkey(b"ett"), None)
        .await
        .unwrap()
        .token()
        .to_string()
}
