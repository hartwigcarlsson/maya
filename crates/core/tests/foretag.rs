//! Att lägga upp ett företag, genom `core` mot en tillfällig databas.

use jiff::Timestamp;
use jiff::civil::date;
use maya_core::aktor::{Aktör, Kanal};
use maya_core::atkomst::ÅtkomstFel;
use maya_core::foretag::{self, FöretagsFel};
use maya_core::kontoplan::{self, Ursprung};
use maya_core::{Databas, Klocka, behandlingshistorik, testhjalp};
use maya_domain::foretag::{Bokföringsmetod, Momsperiod, NyttFöretag};
use maya_domain::kontoplan::{Kontonummer, bas, flödenas_konton};

const NU: &str = "2026-10-18T08:00:00Z";

fn klocka() -> Klocka {
    Klocka::fast(NU.parse().unwrap())
}

fn formulär(orgnr: &str) -> NyttFöretag {
    NyttFöretag {
        orgnr: orgnr.into(),
        namn: "Exempel AB".into(),
        första_år_start: "2026-03-17".into(),
        första_år_slut: "2027-06-30".into(),
        bokföringsmetod: "kontant".into(),
        momsperiod: "kvartal".into(),
    }
}

#[tokio::test]
async fn ett_giltigt_formulär_skapar_företaget_och_det_första_räkenskapsåret_med_perioder() {
    let db = Databas::tillfällig().await;
    let anna = testhjalp::användare(&db, "anna@example.se").await;

    let id = foretag::lägg_upp(&db, &klocka(), &anna, &formulär("556036-0793"))
        .await
        .unwrap();

    let företag = foretag::företaget(&db, &anna, id).await.unwrap();
    assert_eq!(företag.id, id);
    assert_eq!(företag.namn, "Exempel AB");
    assert_eq!(företag.orgnr.to_string(), "556036-0793");
    assert_eq!(företag.räkenskapsår.len(), 1);
    let år = &företag.räkenskapsår[0];
    assert_eq!((år.start, år.slut), (date(2026, 3, 17), date(2027, 6, 30)));
    assert_eq!(år.perioder.len(), 16);
    assert_eq!(
        (år.perioder[0].start, år.perioder[0].slut),
        (date(2026, 3, 17), date(2026, 3, 31))
    );
    assert_eq!(år.perioder[15].start, date(2027, 6, 1));
    assert_eq!(
        företag.bokföringsmetoder,
        [(date(2026, 3, 17), Bokföringsmetod::Kontant)]
    );
    assert_eq!(
        företag.momsperioder,
        [(date(2026, 3, 17), Momsperiod::Kvartal)]
    );
}

#[tokio::test]
async fn den_som_lägger_upp_företaget_får_åtkomst_till_det() {
    let db = Databas::tillfällig().await;
    let anna = testhjalp::användare(&db, "anna@example.se").await;

    let a = foretag::lägg_upp(&db, &klocka(), &anna, &formulär("556036-0793"))
        .await
        .unwrap();
    let mut b_formulär = formulär("212000-0142");
    b_formulär.namn = "Annat AB".into();
    let b = foretag::lägg_upp(&db, &klocka(), &anna, &b_formulär)
        .await
        .unwrap();

    let mina = foretag::mina_företag(&db, &anna).await.unwrap();
    let namn: Vec<_> = mina.iter().map(|f| (f.id, f.namn.as_str())).collect();
    assert_eq!(namn, [(b, "Annat AB"), (a, "Exempel AB")]);
    assert_eq!(mina[1].orgnr.to_string(), "556036-0793");
}

#[tokio::test]
async fn kontoplanen_är_en_kopia_av_bas_med_grundkontona_och_flödenas_konton_aktiva() {
    let db = Databas::tillfällig().await;
    let anna = testhjalp::användare(&db, "anna@example.se").await;
    let id = foretag::lägg_upp(&db, &klocka(), &anna, &formulär("556036-0793"))
        .await
        .unwrap();

    let konton = kontoplan::konton(&db, &anna, id).await.unwrap();

    assert_eq!(konton.len(), bas::konton().len());
    let konto = |n: u16| {
        konton
            .iter()
            .find(|k| k.nummer == Kontonummer::new(n).unwrap())
            .unwrap()
    };
    assert!(konto(1930).aktiv, "grundkonto");
    assert_eq!(konto(1930).namn, "Företagskonto");
    for (nummer, _) in flödenas_konton() {
        assert!(konto(nummer.get()).aktiv, "flödenas konto {nummer}");
    }
    assert!(!konto(1950).aktiv, "inte grundkonto");
    let aktiva: Vec<_> = konton.iter().filter(|k| k.aktiv).collect();
    // BAS 2026 har 336 grundkonton, och 2099 är ett av dem.
    assert_eq!(aktiva.len(), 336);
    assert!(
        aktiva
            .iter()
            .all(|k| bas::konto(k.nummer).unwrap().grundkonto()
                || flödenas_konton().iter().any(|(n, _)| *n == k.nummer))
    );
    for k in &konton {
        assert_eq!(k.ursprung, Ursprung::Bas);
        assert_eq!(k.bas_version.as_deref(), Some("BAS 2026"));
        assert_eq!(k.bas_namn.as_deref(), Some(k.namn.as_str()));
    }
}

#[tokio::test]
async fn ett_ogiltigt_formulär_nekas_med_samma_koder_som_formuläret_och_skapar_inget() {
    let db = Databas::tillfällig().await;
    let anna = testhjalp::användare(&db, "anna@example.se").await;

    for (orgnr, kod) in [
        ("556036-079", "orgnr_langd"),
        ("556036-0794", "orgnr_kontrollsiffra"),
        ("800101-0019", "orgnr_tredje_siffran"),
    ] {
        let fel = foretag::lägg_upp(&db, &klocka(), &anna, &formulär(orgnr))
            .await
            .unwrap_err();
        let FöretagsFel::Ogiltigt(fel) = fel else {
            panic!("{orgnr}: {fel:?}")
        };
        assert_eq!(fel[0].kod, kod, "{orgnr}");
    }
    let mut för_långt = formulär("556036-0793");
    för_långt.första_år_slut = "2027-09-30".into();
    let fel = foretag::lägg_upp(&db, &klocka(), &anna, &för_långt)
        .await
        .unwrap_err();
    assert!(
        matches!(&fel, FöretagsFel::Ogiltigt(f) if f[0].kod == "ar_for_langt"),
        "{fel:?}"
    );

    assert!(foretag::mina_företag(&db, &anna).await.unwrap().is_empty());
}

#[tokio::test]
async fn ett_organisationsnummer_som_redan_finns_nekas_med_hänvisning_till_inbjudan() {
    let db = Databas::tillfällig().await;
    let anna = testhjalp::användare(&db, "anna@example.se").await;
    let bo = testhjalp::användare(&db, "bo@example.se").await;
    foretag::lägg_upp(&db, &klocka(), &anna, &formulär("556036-0793"))
        .await
        .unwrap();

    let fel = foretag::lägg_upp(&db, &klocka(), &bo, &formulär("5560360793"))
        .await
        .unwrap_err();

    assert!(matches!(fel, FöretagsFel::OrgnrFinns(_)), "{fel:?}");
    assert!(fel.to_string().contains("556036-0793"), "{fel}");
    assert!(fel.to_string().contains("inbjudan"), "{fel}");
    assert!(foretag::mina_företag(&db, &bo).await.unwrap().is_empty());
}

#[tokio::test]
async fn två_samtidiga_uppläggningar_av_samma_nummer_ger_ett_företag() {
    let db = Databas::tillfällig().await;
    let anna = testhjalp::användare(&db, "anna@example.se").await;
    let bo = testhjalp::användare(&db, "bo@example.se").await;
    let f = formulär("556036-0793");
    let k = klocka();

    let (a, b) = tokio::join!(
        foretag::lägg_upp(&db, &k, &anna, &f),
        foretag::lägg_upp(&db, &k, &bo, &f),
    );

    assert!(a.is_ok() != b.is_ok(), "{a:?} {b:?}");
    let alla = foretag::mina_företag(&db, &anna).await.unwrap().len()
        + foretag::mina_företag(&db, &bo).await.unwrap().len();
    assert_eq!(alla, 1);
}

#[tokio::test]
async fn bara_webben_lägger_upp_företag() {
    let db = Databas::tillfällig().await;

    for aktör in [Aktör::cli(), Aktör::maya()] {
        let fel = foretag::lägg_upp(&db, &klocka(), &aktör, &formulär("556036-0793"))
            .await
            .unwrap_err();
        assert!(
            matches!(fel, FöretagsFel::Åtkomst(ÅtkomstFel::KanalenFårInte)),
            "{fel:?}"
        );
    }
}

#[tokio::test]
async fn behandlingshistoriken_får_företag_skapat_och_skaparens_åtkomst_med_aktör_och_tid() {
    let db = Databas::tillfällig().await;
    let anna = testhjalp::användare(&db, "anna@example.se").await;
    let id = foretag::lägg_upp(&db, &klocka(), &anna, &formulär("556036-0793"))
        .await
        .unwrap();

    let historik = behandlingshistorik::läs(&db, &anna, id).await.unwrap();

    assert_eq!(historik.len(), 2);
    let åtkomst = &historik[1];
    assert_eq!(åtkomst.typ, "åtkomst given");
    assert_eq!(åtkomst.aktör, anna);
    assert_eq!(åtkomst.objekt, "anna@example.se");
    assert_eq!(åtkomst.text(), "Åtkomst till företaget för anna@example.se");
    let post = &historik[0];
    assert_eq!(post.tid, NU.parse::<Timestamp>().unwrap());
    assert_eq!(post.aktör, anna);
    assert_eq!(post.aktör.kanal, Kanal::Webb);
    assert_eq!(post.typ, "företag skapat");
    assert_eq!(post.objekt, "556036-0793");
    assert_eq!(post.beskrivning["namn"], "Exempel AB");
    assert_eq!(
        post.beskrivning["första_räkenskapsår"]["start"],
        "2026-03-17"
    );
    assert_eq!(post.beskrivning["bokföringsmetod"], "kontant");
    assert_eq!(post.beskrivning["momsperiod"], "kvartal");
    assert_eq!(post.beskrivning["kontoplan"], "BAS 2026");
}

#[tokio::test]
async fn en_användare_utan_åtkomst_nekas_av_core() {
    let db = Databas::tillfällig().await;
    let anna = testhjalp::användare(&db, "anna@example.se").await;
    let bo = testhjalp::användare(&db, "bo@example.se").await;
    let id = foretag::lägg_upp(&db, &klocka(), &anna, &formulär("556036-0793"))
        .await
        .unwrap();

    assert!(matches!(
        foretag::företaget(&db, &bo, id).await,
        Err(ÅtkomstFel::IngenÅtkomst)
    ));
    assert!(matches!(
        kontoplan::konton(&db, &bo, id).await,
        Err(ÅtkomstFel::IngenÅtkomst)
    ));
    assert!(matches!(
        behandlingshistorik::läs(&db, &bo, id).await,
        Err(ÅtkomstFel::IngenÅtkomst)
    ));
    assert!(matches!(
        foretag::företaget(&db, &Aktör::cli(), id).await,
        Err(ÅtkomstFel::KanalenFårInte)
    ));
    assert!(foretag::mina_företag(&db, &bo).await.unwrap().is_empty());
}

#[tokio::test]
async fn ett_företag_som_inte_finns_ser_ut_som_ett_utan_åtkomst() {
    let db = Databas::tillfällig().await;
    let anna = testhjalp::användare(&db, "anna@example.se").await;

    let id = "4711".parse().unwrap();

    assert!(matches!(
        foretag::företaget(&db, &anna, id).await,
        Err(ÅtkomstFel::IngenÅtkomst)
    ));
}
