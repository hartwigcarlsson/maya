//! Åtkomst och inbjudningar genom `core` mot en tillfällig databas: bjuda in med engångslänk,
//! ge och ta åtkomst, färsk passkey, vilka användare man ser och behandlingshistoriken.

use jiff::{SignedDuration, Timestamp};
use maya_core::aktor::{Aktör, AnvändarId};
use maya_core::atkomst::{self, DelningsFel, Inbjudan};
use maya_core::foretag::{self, FöretagsId};
use maya_core::inloggning::{self, Inloggad, InloggningsFel, NyPasskey};
use maya_core::{Databas, Klocka, behandlingshistorik, testhjalp};
use maya_domain::foretag::NyttFöretag;

const START: &str = "2026-10-18T08:00:00Z";

fn klocka(efter: SignedDuration) -> Klocka {
    Klocka::fast(START.parse::<Timestamp>().unwrap() + efter)
}

fn nu() -> Klocka {
    klocka(SignedDuration::ZERO)
}

fn passkey(id: &[u8]) -> NyPasskey {
    NyPasskey {
        credential_id: id.to_vec(),
        namn: "Telefon".into(),
        passkey: "{}".into(),
    }
}

fn id(aktör: &Aktör) -> AnvändarId {
    aktör.användare.unwrap()
}

/// Anna med företaget Exempel AB, inloggad med en passkey använd vid START.
async fn anna(db: &Databas) -> (Inloggad, FöretagsId) {
    let (anna, företag) = testhjalp::företag(db).await;
    (testhjalp::inloggad(db, &nu(), &anna).await, företag)
}

async fn eget_företag(db: &Databas, aktör: &Aktör, orgnr: &str) -> FöretagsId {
    foretag::lägg_upp(
        db,
        &nu(),
        aktör,
        &NyttFöretag {
            orgnr: orgnr.into(),
            namn: format!("Bolag {orgnr}"),
            första_år_start: "2026-01-01".into(),
            första_år_slut: "2026-12-31".into(),
            bokföringsmetod: "faktura".into(),
            momsperiod: "kvartal".into(),
        },
    )
    .await
    .unwrap()
}

async fn har_åtkomst(db: &Databas, aktör: &Aktör, företag: FöretagsId) -> bool {
    foretag::mina_företag(db, aktör)
        .await
        .unwrap()
        .iter()
        .any(|f| f.id == företag)
}

fn länk(inbjudan: Inbjudan) -> inloggning::Registreringslänk {
    match inbjudan {
        Inbjudan::Länk(länk) => länk,
        Inbjudan::ÅtkomstGiven => panic!("väntade en länk"),
    }
}

#[tokio::test]
async fn en_inbjudningslänk_registrerar_en_ny_användare_med_åtkomst_till_företaget() {
    let db = Databas::tillfällig().await;
    let (anna, företag) = anna(&db).await;

    let länk = länk(
        atkomst::bjud_in(&db, &nu(), &anna, företag, " Bo@Example.se ")
            .await
            .unwrap(),
    );
    assert_eq!(länk.giltig_till, nu().nu() + SignedDuration::from_hours(24));

    let registrering = inloggning::registrering(&db, &nu(), länk.token())
        .await
        .unwrap();
    assert_eq!(registrering.användare.epost, "bo@example.se");
    let session = inloggning::slutför_registrering(&db, &nu(), länk.token(), passkey(b"bo"), None)
        .await
        .unwrap();
    let bo = inloggning::session(&db, &nu(), session.token())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(bo.användare.epost, "bo@example.se");
    assert!(har_åtkomst(&db, &bo.aktör, företag).await);

    // En gång.
    assert!(matches!(
        inloggning::registrering(&db, &nu(), länk.token()).await,
        Err(InloggningsFel::OgiltigLänk)
    ));
    assert!(matches!(
        inloggning::slutför_registrering(&db, &nu(), länk.token(), passkey(b"bo2"), None).await,
        Err(InloggningsFel::OgiltigLänk)
    ));
}

#[tokio::test]
async fn en_inbjudningslänk_gäller_i_24_timmar() {
    let db = Databas::tillfällig().await;
    let (anna, företag) = anna(&db).await;
    let länk = länk(
        atkomst::bjud_in(&db, &nu(), &anna, företag, "bo@example.se")
            .await
            .unwrap(),
    );

    let sent = klocka(SignedDuration::from_hours(24));
    assert!(matches!(
        inloggning::registrering(&db, &sent, länk.token()).await,
        Err(InloggningsFel::OgiltigLänk)
    ));
    assert!(matches!(
        inloggning::slutför_registrering(&db, &sent, länk.token(), passkey(b"bo"), None).await,
        Err(InloggningsFel::OgiltigLänk)
    ));
    let i_tid = klocka(SignedDuration::from_hours(24) - SignedDuration::from_secs(1));
    inloggning::slutför_registrering(&db, &i_tid, länk.token(), passkey(b"bo"), None)
        .await
        .unwrap();
}

#[tokio::test]
async fn öppna_inbjudningar_listas_tills_de_används_eller_löper_ut() {
    let db = Databas::tillfällig().await;
    let (anna, företag) = anna(&db).await;
    let bo = länk(
        atkomst::bjud_in(&db, &nu(), &anna, företag, "bo@example.se")
            .await
            .unwrap(),
    );
    let senare = klocka(SignedDuration::from_hours(1));
    let anna_senare = testhjalp::inloggad(&db, &senare, &anna.aktör).await;
    atkomst::bjud_in(&db, &senare, &anna_senare, företag, "cilla@example.se")
        .await
        .unwrap();

    let lista = atkomst::åtkomst(&db, &nu(), &anna.aktör, företag)
        .await
        .unwrap();
    let öppna: Vec<_> = lista
        .inbjudningar
        .iter()
        .map(|i| i.epost.as_str())
        .collect();
    assert_eq!(öppna, ["bo@example.se", "cilla@example.se"]);
    assert_eq!(lista.inbjudningar[0].inbjuden_av, "anna@example.se");

    inloggning::slutför_registrering(&db, &nu(), bo.token(), passkey(b"bo"), None)
        .await
        .unwrap();
    let senare = klocka(SignedDuration::from_hours(24) + SignedDuration::from_mins(30));
    let lista = atkomst::åtkomst(&db, &senare, &anna.aktör, företag)
        .await
        .unwrap();
    let öppna: Vec<_> = lista
        .inbjudningar
        .iter()
        .map(|i| i.epost.as_str())
        .collect();
    assert_eq!(öppna, ["cilla@example.se"]);
    let användare: Vec<_> = lista.användare.iter().map(|a| a.epost.as_str()).collect();
    assert_eq!(användare, ["anna@example.se", "bo@example.se"]);
}

#[tokio::test]
async fn registrering_med_en_inbjudan_använder_varje_öppen_inbjudan_till_samma_adress() {
    let db = Databas::tillfällig().await;
    let (anna, första) = anna(&db).await;
    let andra = eget_företag(&db, &anna.aktör, "556677-8899").await;
    let länk1 = länk(
        atkomst::bjud_in(&db, &nu(), &anna, första, "bo@example.se")
            .await
            .unwrap(),
    );
    länk(
        atkomst::bjud_in(&db, &nu(), &anna, andra, "Bo@example.se")
            .await
            .unwrap(),
    );

    let session =
        inloggning::slutför_registrering(&db, &nu(), länk1.token(), passkey(b"bo"), None)
            .await
            .unwrap();
    let bo = inloggning::session(&db, &nu(), session.token())
        .await
        .unwrap()
        .unwrap();

    assert!(har_åtkomst(&db, &bo.aktör, första).await);
    assert!(har_åtkomst(&db, &bo.aktör, andra).await);
    let lista = atkomst::åtkomst(&db, &nu(), &anna.aktör, andra)
        .await
        .unwrap();
    assert!(lista.inbjudningar.is_empty(), "{:?}", lista.inbjudningar);
    let typer: Vec<String> = behandlingshistorik::läs(&db, &anna.aktör, andra)
        .await
        .unwrap()
        .into_iter()
        .filter(|p| p.aktör == bo.aktör)
        .map(|p| p.typ)
        .collect();
    assert_eq!(typer, ["inbjudan använd", "åtkomst given"]);
}

#[tokio::test]
async fn en_inbjudan_till_en_befintlig_epost_ger_bara_åtkomst() {
    let db = Databas::tillfällig().await;
    let (anna, företag) = anna(&db).await;
    let bo = testhjalp::användare(&db, "bo@example.se").await;

    let svar = atkomst::bjud_in(&db, &nu(), &anna, företag, "BO@example.se")
        .await
        .unwrap();

    assert!(matches!(svar, Inbjudan::ÅtkomstGiven));
    assert!(har_åtkomst(&db, &bo, företag).await);
    let lista = atkomst::åtkomst(&db, &nu(), &anna.aktör, företag)
        .await
        .unwrap();
    assert!(lista.inbjudningar.is_empty());
    assert!(matches!(
        atkomst::bjud_in(&db, &nu(), &anna, företag, "bo@example.se").await,
        Err(DelningsFel::HarRedanÅtkomst)
    ));
}

#[tokio::test]
async fn en_inbjudan_kräver_en_giltig_epostadress() {
    let db = Databas::tillfällig().await;
    let (anna, företag) = anna(&db).await;
    assert!(matches!(
        atkomst::bjud_in(&db, &nu(), &anna, företag, "inte en adress").await,
        Err(DelningsFel::OgiltigEpost(_))
    ));
}

#[tokio::test]
async fn den_sista_åtkomsten_till_ett_företag_kan_inte_tas_bort() {
    let db = Databas::tillfällig().await;
    let (anna, företag) = anna(&db).await;
    let anna_id = anna.användare.id;

    assert!(matches!(
        atkomst::ta_åtkomst(&db, &nu(), &anna, företag, anna_id).await,
        Err(DelningsFel::SistaÅtkomsten)
    ));
    assert!(har_åtkomst(&db, &anna.aktör, företag).await);

    let bo = testhjalp::användare(&db, "bo@example.se").await;
    atkomst::bjud_in(&db, &nu(), &anna, företag, "bo@example.se")
        .await
        .unwrap();
    atkomst::ta_åtkomst(&db, &nu(), &anna, företag, id(&bo))
        .await
        .unwrap();
    assert!(!har_åtkomst(&db, &bo, företag).await);

    // Med Bo tillbaka kan Anna ta bort sin egen åtkomst; då är Bo den sista.
    atkomst::bjud_in(&db, &nu(), &anna, företag, "bo@example.se")
        .await
        .unwrap();
    atkomst::ta_åtkomst(&db, &nu(), &anna, företag, anna_id)
        .await
        .unwrap();
    assert!(!har_åtkomst(&db, &anna.aktör, företag).await);
    let bo = testhjalp::inloggad(&db, &nu(), &bo).await;
    assert!(matches!(
        atkomst::ta_åtkomst(&db, &nu(), &bo, företag, bo.användare.id).await,
        Err(DelningsFel::SistaÅtkomsten)
    ));
}

#[tokio::test]
async fn en_avaktiverad_användare_räknas_inte_som_åtkomst() {
    let db = Databas::tillfällig().await;
    let (anna, företag) = anna(&db).await;
    testhjalp::användare(&db, "bo@example.se").await;
    atkomst::bjud_in(&db, &nu(), &anna, företag, "bo@example.se")
        .await
        .unwrap();
    inloggning::avaktivera_användare(&db, &nu(), &Aktör::cli(), "bo@example.se")
        .await
        .unwrap();

    assert!(matches!(
        atkomst::ta_åtkomst(&db, &nu(), &anna, företag, anna.användare.id).await,
        Err(DelningsFel::SistaÅtkomsten)
    ));
}

#[tokio::test]
async fn inbjudan_och_ändrad_åtkomst_kräver_en_passkey_inom_5_minuter() {
    let db = Databas::tillfällig().await;
    let (anna, företag) = anna(&db).await;
    let bo = testhjalp::användare(&db, "bo@example.se").await;
    let sent = klocka(SignedDuration::from_mins(5));

    assert!(matches!(
        atkomst::bjud_in(&db, &sent, &anna, företag, "cilla@example.se").await,
        Err(DelningsFel::KräverFärskPasskey)
    ));
    assert!(matches!(
        atkomst::ta_åtkomst(&db, &sent, &anna, företag, id(&bo)).await,
        Err(DelningsFel::KräverFärskPasskey)
    ));
    assert!(matches!(
        atkomst::ge_åtkomst(&db, &sent, &anna, företag, id(&bo)).await,
        Err(DelningsFel::KräverFärskPasskey)
    ));
    let i_tid = klocka(SignedDuration::from_mins(5) - SignedDuration::from_secs(1));
    atkomst::bjud_in(&db, &i_tid, &anna, företag, "cilla@example.se")
        .await
        .unwrap();
}

#[tokio::test]
async fn bara_den_som_har_åtkomst_kan_bjuda_in_och_ändra_åtkomst() {
    let db = Databas::tillfällig().await;
    let (anna, företag) = anna(&db).await;
    let bo = testhjalp::användare(&db, "bo@example.se").await;
    let bo = testhjalp::inloggad(&db, &nu(), &bo).await;

    assert!(matches!(
        atkomst::bjud_in(&db, &nu(), &bo, företag, "cilla@example.se").await,
        Err(DelningsFel::Åtkomst(_))
    ));
    assert!(matches!(
        atkomst::ta_åtkomst(&db, &nu(), &bo, företag, anna.användare.id).await,
        Err(DelningsFel::Åtkomst(_))
    ));
    assert!(matches!(
        atkomst::ge_åtkomst(&db, &nu(), &bo, företag, bo.användare.id).await,
        Err(DelningsFel::Åtkomst(_))
    ));
    assert!(
        atkomst::åtkomst(&db, &nu(), &bo.aktör, företag)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn en_användare_ser_bara_användare_hen_delar_ett_företag_med() {
    let db = Databas::tillfällig().await;
    let (anna, exempel) = anna(&db).await;
    let bo = testhjalp::användare(&db, "bo@example.se").await;
    let cilla = testhjalp::användare(&db, "cilla@example.se").await;
    // Bo och Anna delar Bolag 2; Cilla har ett eget företag som Anna inte ser.
    let delat = eget_företag(&db, &bo, "556677-8899").await;
    eget_företag(&db, &cilla, "559000-0005").await;
    let bo_in = testhjalp::inloggad(&db, &nu(), &bo).await;
    atkomst::bjud_in(&db, &nu(), &bo_in, delat, "anna@example.se")
        .await
        .unwrap();

    let lista = atkomst::åtkomst(&db, &nu(), &anna.aktör, exempel)
        .await
        .unwrap();
    let användare: Vec<_> = lista.användare.iter().map(|a| a.epost.as_str()).collect();
    assert_eq!(användare, ["anna@example.se"]);
    let kandidater: Vec<_> = lista.kandidater.iter().map(|a| a.epost.as_str()).collect();
    assert_eq!(kandidater, ["bo@example.se"]);

    assert!(matches!(
        atkomst::ge_åtkomst(&db, &nu(), &anna, exempel, id(&cilla)).await,
        Err(DelningsFel::OkändAnvändare)
    ));
    assert!(matches!(
        atkomst::ta_åtkomst(&db, &nu(), &anna, exempel, id(&cilla)).await,
        Err(DelningsFel::OkändAnvändare)
    ));
    atkomst::ge_åtkomst(&db, &nu(), &anna, exempel, id(&bo))
        .await
        .unwrap();
    assert!(har_åtkomst(&db, &bo, exempel).await);
    assert!(matches!(
        atkomst::ge_åtkomst(&db, &nu(), &anna, exempel, id(&bo)).await,
        Err(DelningsFel::HarRedanÅtkomst)
    ));
    let lista = atkomst::åtkomst(&db, &nu(), &anna.aktör, exempel)
        .await
        .unwrap();
    assert!(lista.kandidater.is_empty());
}

#[tokio::test]
async fn alla_åtkomsthändelser_loggas_med_aktör_och_tid() {
    let db = Databas::tillfällig().await;
    let (anna, företag) = anna(&db).await;
    let länk = länk(
        atkomst::bjud_in(&db, &nu(), &anna, företag, "bo@example.se")
            .await
            .unwrap(),
    );
    let senare = klocka(SignedDuration::from_mins(2));
    let session =
        inloggning::slutför_registrering(&db, &senare, länk.token(), passkey(b"bo"), None)
            .await
            .unwrap();
    let bo = inloggning::session(&db, &senare, session.token())
        .await
        .unwrap()
        .unwrap();
    testhjalp::användare(&db, "cilla@example.se").await;
    atkomst::bjud_in(&db, &nu(), &anna, företag, "cilla@example.se")
        .await
        .unwrap();
    let cilla = atkomst::åtkomst(&db, &nu(), &anna.aktör, företag)
        .await
        .unwrap()
        .användare
        .into_iter()
        .find(|a| a.epost == "cilla@example.se")
        .unwrap()
        .id;
    atkomst::ta_åtkomst(&db, &nu(), &anna, företag, cilla)
        .await
        .unwrap();

    let poster = behandlingshistorik::läs(&db, &anna.aktör, företag)
        .await
        .unwrap();
    let åtkomst: Vec<_> = poster
        .iter()
        .filter(|p| p.typ != "företag skapat")
        .map(|p| (p.typ.as_str(), p.objekt.as_str(), p.aktör, p.tid))
        .collect();
    assert_eq!(
        åtkomst,
        [
            ("inbjudan skapad", "bo@example.se", anna.aktör, nu().nu()),
            ("inbjudan använd", "bo@example.se", bo.aktör, senare.nu()),
            ("åtkomst given", "bo@example.se", bo.aktör, senare.nu()),
            ("åtkomst given", "cilla@example.se", anna.aktör, nu().nu()),
            ("åtkomst tagen", "cilla@example.se", anna.aktör, nu().nu()),
        ]
    );
    let skapad = &poster.iter().find(|p| p.typ == "inbjudan skapad").unwrap();
    assert_eq!(
        skapad.beskrivning["giltig_till"],
        (nu().nu() + SignedDuration::from_hours(24)).to_string()
    );
}

#[tokio::test]
async fn ett_företag_med_inbjudningar_kan_raderas() {
    let db = Databas::tillfällig().await;
    let (anna, företag) = anna(&db).await;
    let använd = länk(
        atkomst::bjud_in(&db, &nu(), &anna, företag, "bo@example.se")
            .await
            .unwrap(),
    );
    inloggning::slutför_registrering(&db, &nu(), använd.token(), passkey(b"bo"), None)
        .await
        .unwrap();
    let öppen = länk(
        atkomst::bjud_in(&db, &nu(), &anna, företag, "cilla@example.se")
            .await
            .unwrap(),
    );

    foretag::radera(&db, &nu(), &anna.aktör, företag)
        .await
        .unwrap();

    assert!(matches!(
        inloggning::registrering(&db, &nu(), öppen.token()).await,
        Err(InloggningsFel::OgiltigLänk)
    ));
}
