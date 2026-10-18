//! Det som rör en användare som helhet görs bara med CLI:t (ADR 0005): återställa, avaktivera,
//! återaktivera och byta domän. Varje händelse hamnar i installationsloggen.

use jiff::{SignedDuration, Timestamp};
use maya_core::aktor::{Aktör, Kanal};
use maya_core::inloggning::{self, InloggningsFel, NyPasskey};
use maya_core::installation::{self, InstallationsFel, RelyingParty};
use maya_core::installationslogg;
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

fn passkey(id: &[u8]) -> NyPasskey {
    NyPasskey {
        credential_id: id.to_vec(),
        namn: "MacBook".into(),
        passkey: format!("{{\"cred\":{id:?}}}"),
    }
}

fn rp(url: &str) -> RelyingParty {
    RelyingParty::från_publik_url(&url.parse().unwrap())
}

/// Anna med passkeyn "ett", inloggad. Ger sessionstoken.
async fn anna(db: &Databas) -> String {
    installation::kontrollera_rp(db, &rp("https://bokforing.example.se"))
        .await
        .unwrap();
    let länk = inloggning::skapa_första_användare(db, &nu(), &Aktör::cli(), "anna@example.se")
        .await
        .unwrap();
    inloggning::slutför_registrering(db, &nu(), länk.token(), passkey(b"ett"), None)
        .await
        .unwrap()
        .token()
        .to_string()
}

async fn kan_logga_in(db: &Databas) -> bool {
    inloggning::inloggning_för(db, "anna@example.se")
        .await
        .unwrap()
        .is_some()
}

async fn loggtyper(db: &Databas) -> Vec<String> {
    installationslogg::läs(db)
        .await
        .unwrap()
        .into_iter()
        .map(|p| p.typ)
        .collect()
}

#[tokio::test]
async fn aterstallning_ger_en_engangslank_som_galler_i_24_timmar() {
    let db = Databas::tillfällig().await;
    anna(&db).await;

    let länk = inloggning::återställ_användare(&db, &nu(), &Aktör::cli(), " Anna@Example.se ")
        .await
        .unwrap();

    assert_eq!(
        länk.giltig_till,
        START.parse::<Timestamp>().unwrap() + timmar(24)
    );
    let för_sent = inloggning::registrering(&db, &klocka(timmar(24)), länk.token()).await;
    assert!(matches!(för_sent, Err(InloggningsFel::OgiltigLänk)));

    let registrering = inloggning::registrering(&db, &klocka(timmar(23)), länk.token())
        .await
        .unwrap();
    assert_eq!(registrering.användare.epost, "anna@example.se");
    inloggning::slutför_registrering(&db, &klocka(timmar(23)), länk.token(), passkey(b"ny"), None)
        .await
        .unwrap();
    let igen = inloggning::registrering(&db, &klocka(timmar(23)), länk.token()).await;
    assert!(matches!(igen, Err(InloggningsFel::OgiltigLänk)));

    let post = installationslogg::läs(&db).await.unwrap().pop().unwrap();
    assert_eq!(post.typ, "användare återställd");
    assert_eq!(post.aktör.kanal, Kanal::Cli);
    assert_eq!(post.objekt, "anna@example.se");
}

#[tokio::test]
async fn en_okand_anvandare_kan_inte_aterstallas() {
    let db = Databas::tillfällig().await;
    anna(&db).await;

    let err = inloggning::återställ_användare(&db, &nu(), &Aktör::cli(), "bo@example.se")
        .await
        .unwrap_err();

    assert!(matches!(err, InloggningsFel::OkändAnvändare), "{err}");
}

#[tokio::test]
async fn en_avaktiverad_anvandare_kan_inte_logga_in_och_ateraktivering_ger_tillbaka_inloggningen() {
    let db = Databas::tillfällig().await;
    let session = anna(&db).await;
    let återställning =
        inloggning::återställ_användare(&db, &nu(), &Aktör::cli(), "anna@example.se")
            .await
            .unwrap();

    inloggning::avaktivera_användare(&db, &nu(), &Aktör::cli(), "anna@example.se")
        .await
        .unwrap();

    assert!(!kan_logga_in(&db).await);
    assert!(
        inloggning::session(&db, &nu(), &session)
            .await
            .unwrap()
            .is_none()
    );
    let id = inloggning::registrering(&db, &nu(), återställning.token()).await;
    assert!(matches!(id, Err(InloggningsFel::OgiltigLänk)));
    let err = inloggning::återställ_användare(&db, &nu(), &Aktör::cli(), "anna@example.se")
        .await
        .unwrap_err();
    assert!(matches!(err, InloggningsFel::Avaktiverad), "{err}");

    inloggning::återaktivera_användare(&db, &nu(), &Aktör::cli(), "anna@example.se")
        .await
        .unwrap();

    assert!(kan_logga_in(&db).await);
    // Sessioner och länkar från före avaktiveringen gäller inte igen.
    assert!(
        inloggning::session(&db, &nu(), &session)
            .await
            .unwrap()
            .is_none()
    );
    let id = inloggning::registrering(&db, &nu(), återställning.token()).await;
    assert!(matches!(id, Err(InloggningsFel::OgiltigLänk)));
    assert_eq!(
        loggtyper(&db).await[2..],
        [
            "användare avaktiverad".to_string(),
            "användare återaktiverad".to_string()
        ]
    );
}

#[tokio::test]
async fn avaktivering_och_ateraktivering_gors_en_gang() {
    let db = Databas::tillfällig().await;
    anna(&db).await;

    let err = inloggning::återaktivera_användare(&db, &nu(), &Aktör::cli(), "anna@example.se")
        .await
        .unwrap_err();
    assert!(matches!(err, InloggningsFel::InteAvaktiverad), "{err}");

    inloggning::avaktivera_användare(&db, &nu(), &Aktör::cli(), "anna@example.se")
        .await
        .unwrap();
    let err = inloggning::avaktivera_användare(&db, &nu(), &Aktör::cli(), "anna@example.se")
        .await
        .unwrap_err();
    assert!(matches!(err, InloggningsFel::Avaktiverad), "{err}");
}

#[tokio::test]
async fn bara_cli_far_hantera_anvandare() {
    let db = Databas::tillfällig().await;
    let session = anna(&db).await;
    let webb = inloggning::session(&db, &nu(), &session)
        .await
        .unwrap()
        .unwrap()
        .aktör;

    for aktör in [webb, Aktör::maya()] {
        let fel = [
            inloggning::återställ_användare(&db, &nu(), &aktör, "anna@example.se")
                .await
                .map(|_| ()),
            inloggning::avaktivera_användare(&db, &nu(), &aktör, "anna@example.se").await,
            inloggning::återaktivera_användare(&db, &nu(), &aktör, "anna@example.se").await,
        ];
        for fel in fel {
            assert!(
                matches!(fel, Err(InloggningsFel::KanalenFårInte)),
                "{fel:?}"
            );
        }
        let fel = installation::byt_domän(&db, &nu(), &aktör, &rp("https://ny.example.se")).await;
        assert!(
            matches!(fel, Err(InstallationsFel::KanalenFårInte)),
            "{fel:?}"
        );
    }
    assert!(kan_logga_in(&db).await);
}

#[tokio::test]
async fn domanbyte_uppdaterar_rp_id_och_origin_och_tommer_alla_passkeys() {
    let db = Databas::tillfällig().await;
    let session = anna(&db).await;
    let ny = rp("https://ny.example.se:8443");

    installation::byt_domän(&db, &nu(), &Aktör::cli(), &ny)
        .await
        .unwrap();

    installation::kontrollera_rp(&db, &ny).await.unwrap();
    let gammal = installation::kontrollera_rp(&db, &rp("https://bokforing.example.se")).await;
    assert!(matches!(gammal, Err(InstallationsFel::AnnatRpId { .. })));
    let grund = inloggning::inloggning_för(&db, "anna@example.se")
        .await
        .unwrap()
        .unwrap();
    assert!(grund.passkeys.is_empty());
    assert!(
        inloggning::session(&db, &nu(), &session)
            .await
            .unwrap()
            .is_none()
    );

    let post = installationslogg::läs(&db).await.unwrap().pop().unwrap();
    assert_eq!(post.typ, "domän bytt");
    assert_eq!(post.aktör, Aktör::cli());
    assert_eq!(post.objekt, "ny.example.se");
    assert_eq!(post.beskrivning["från_rp_id"], "bokforing.example.se");
    assert_eq!(
        post.beskrivning["till_origin"],
        "https://ny.example.se:8443"
    );
    assert_eq!(post.beskrivning["borttagna_passkeys"], 1);
}

#[tokio::test]
async fn domanbyte_till_samma_doman_vagras() {
    let db = Databas::tillfällig().await;
    anna(&db).await;

    let err = installation::byt_domän(
        &db,
        &nu(),
        &Aktör::cli(),
        &rp("https://bokforing.example.se"),
    )
    .await
    .unwrap_err();

    assert!(matches!(err, InstallationsFel::SammaDomän(_)), "{err}");
    assert!(kan_logga_in(&db).await);
}
