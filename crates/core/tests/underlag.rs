//! Underlag: ladda upp, visa, koppla till utkast, komplettera bokförda verifikat och ta bort,
//! genom `core` mot en tillfällig databas.

use maya_core::aktor::Aktör;
use maya_core::foretag::{self, FöretagsId};
use maya_core::underlag::{self, UnderlagFel, UnderlagId, Urval};
use maya_core::verifikat::{self, VerifikatFel};
use maya_core::{Databas, Klocka, behandlingshistorik, testhjalp};
use maya_domain::foretag::NyttFöretag;
use maya_domain::underlag::STÖRSTA_STORLEK;
use maya_domain::verifikat::{NyRad, NyttVerifikat};

const NU: &str = "2026-10-18T08:00:00Z";

fn klocka() -> Klocka {
    Klocka::fast(NU.parse().unwrap())
}

const KVITTO: &[u8] = b"%PDF-1.7\nett kvitto\n%%EOF\n";
// SHA-256 av KVITTO, räknad med `shasum -a 256`.
const KVITTOTS_HASH: &str = "7228b83aa5f1b9ef37d5d26106b76a3448842824c67176dbc947b0a9a77503b5";

async fn annat_företag(db: &Databas, aktör: &Aktör) -> FöretagsId {
    foretag::lägg_upp(
        db,
        &klocka(),
        aktör,
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
    .unwrap()
}

#[tokio::test]
async fn ett_uppladdat_underlag_arkiveras_med_sha256_från_mottagandet() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;

    let uppladdat = underlag::ladda_upp(&db, &klocka(), &anna, id, "kvitto.pdf", "", KVITTO)
        .await
        .unwrap();
    assert!(uppladdat.dubbletter.is_empty());

    let u = underlag::underlaget(&db, &anna, id, uppladdat.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(u.filnamn, "kvitto.pdf");
    assert_eq!(u.mime, "application/pdf");
    assert_eq!(u.storlek, KVITTO.len() as u64);
    assert_eq!(u.sha256, KVITTOTS_HASH);
    assert_eq!(u.uppladdad.aktör, anna);
    assert_eq!(u.uppladdad.tid, NU.parse().unwrap());
    assert!(u.kopplingar.is_empty());
    assert!(u.kan_tas_bort());

    let fil = underlag::filen(&db, &anna, id, uppladdat.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fil.innehåll, KVITTO);
    assert_eq!(fil.mime, "application/pdf");

    let okopplade = underlag::lista(&db, &anna, id, Urval::Okopplade)
        .await
        .unwrap();
    assert_eq!(okopplade.len(), 1);
    assert_eq!(okopplade[0].id, uppladdat.id);

    let historik = behandlingshistorik::läs(&db, &anna, id).await.unwrap();
    let post = historik.last().unwrap();
    assert_eq!(post.typ, "underlag uppladdat");
    assert_eq!(post.objekt, "kvitto.pdf");
    assert_eq!(post.beskrivning["sha256"], KVITTOTS_HASH);
    assert_eq!(post.beskrivning["storlek"], KVITTO.len());
    assert_eq!(
        post.beskrivning["underlag"],
        uppladdat.id.to_string().parse::<i64>().unwrap()
    );
}

#[tokio::test]
async fn ett_underlag_över_25_mb_nekas() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    let för_stort = vec![0u8; STÖRSTA_STORLEK + 1];

    let fel = underlag::ladda_upp(&db, &klocka(), &anna, id, "stor.bin", "", &för_stort)
        .await
        .unwrap_err();
    assert!(matches!(fel, UnderlagFel::FörStort), "{fel:?}");
    assert!(
        underlag::lista(&db, &anna, id, Urval::Alla)
            .await
            .unwrap()
            .is_empty()
    );

    let precis = vec![0u8; STÖRSTA_STORLEK];
    underlag::ladda_upp(&db, &klocka(), &anna, id, "precis.bin", "", &precis)
        .await
        .unwrap();
}

#[tokio::test]
async fn samma_fil_igen_ger_en_dubblettvarning_men_laddas_upp() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    let första = underlag::ladda_upp(&db, &klocka(), &anna, id, "kvitto.pdf", "", KVITTO)
        .await
        .unwrap();

    let andra = underlag::ladda_upp(&db, &klocka(), &anna, id, "kvitto (1).pdf", "", KVITTO)
        .await
        .unwrap();

    assert_ne!(andra.id, första.id);
    assert_eq!(andra.dubbletter.len(), 1);
    assert_eq!(andra.dubbletter[0].id, första.id);
    assert_eq!(andra.dubbletter[0].filnamn, "kvitto.pdf");
    assert_eq!(
        underlag::lista(&db, &anna, id, Urval::Alla)
            .await
            .unwrap()
            .len(),
        2
    );

    // Samma fil i ett annat företag är ingen dubblett.
    let annat = annat_företag(&db, &anna).await;
    let tredje = underlag::ladda_upp(&db, &klocka(), &anna, annat, "kvitto.pdf", "", KVITTO)
        .await
        .unwrap();
    assert!(tredje.dubbletter.is_empty());
}

#[tokio::test]
async fn den_som_saknar_åtkomst_kan_varken_ladda_upp_eller_läsa() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    let bertil = testhjalp::användare(&db, "bertil@example.se").await;
    let u = underlag::ladda_upp(&db, &klocka(), &anna, id, "kvitto.pdf", "", KVITTO)
        .await
        .unwrap();

    assert!(matches!(
        underlag::ladda_upp(&db, &klocka(), &bertil, id, "x.pdf", "", KVITTO).await,
        Err(UnderlagFel::Åtkomst(_))
    ));
    assert!(underlag::underlaget(&db, &bertil, id, u.id).await.is_err());
    assert!(underlag::filen(&db, &bertil, id, u.id).await.is_err());
}

fn formulär(datum: &str) -> NyttVerifikat {
    let rad = |konto: &str, debet: &str, kredit: &str| NyRad {
        konto: konto.into(),
        debet: debet.into(),
        kredit: kredit.into(),
        text: String::new(),
    };
    NyttVerifikat {
        datum: datum.into(),
        beskrivning: "Datortillbehör".into(),
        rader: vec![rad("6540", "1000", ""), rad("1930", "", "1000")],
    }
}

async fn ladda_upp(db: &Databas, anna: &Aktör, id: FöretagsId, namn: &str) -> UnderlagId {
    underlag::ladda_upp(db, &klocka(), anna, id, namn, "", namn.as_bytes())
        .await
        .unwrap()
        .id
}

async fn okopplade(db: &Databas, anna: &Aktör, id: FöretagsId) -> Vec<UnderlagId> {
    underlag::lista(db, anna, id, Urval::Okopplade)
        .await
        .unwrap()
        .into_iter()
        .map(|u| u.id)
        .collect()
}

async fn spara(
    db: &Databas,
    anna: &Aktör,
    id: FöretagsId,
    utkast: Option<verifikat::VerifikatId>,
    underlag: &[UnderlagId],
) -> Result<verifikat::VerifikatId, VerifikatFel> {
    verifikat::spara_utkast_med_underlag(
        db,
        &klocka(),
        anna,
        id,
        utkast,
        &formulär("2026-03-14"),
        underlag,
    )
    .await
}

async fn verifikatet(
    db: &Databas,
    anna: &Aktör,
    id: FöretagsId,
    v: verifikat::VerifikatId,
) -> verifikat::Verifikat {
    verifikat::verifikatet(db, anna, id, v)
        .await
        .unwrap()
        .unwrap()
}

#[tokio::test]
async fn underlag_kopplas_till_och_från_ett_utkast() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    let kvitto = ladda_upp(&db, &anna, id, "kvitto.pdf").await;
    let avtal = ladda_upp(&db, &anna, id, "avtal.pdf").await;

    let utkast = spara(&db, &anna, id, None, &[kvitto, avtal]).await.unwrap();

    let v = verifikatet(&db, &anna, id, utkast).await;
    let namn: Vec<_> = v.underlag.iter().map(|u| u.filnamn.as_str()).collect();
    assert_eq!(namn, ["kvitto.pdf", "avtal.pdf"]);
    assert!(v.underlag.iter().all(|u| !u.komplettering));
    assert!(okopplade(&db, &anna, id).await.is_empty());

    // Koppla bort avtalet.
    spara(&db, &anna, id, Some(utkast), &[kvitto])
        .await
        .unwrap();
    let v = verifikatet(&db, &anna, id, utkast).await;
    assert_eq!(v.underlag.len(), 1);
    assert_eq!(v.underlag[0].id, kvitto);
    assert_eq!(okopplade(&db, &anna, id).await, [avtal]);

    // Att spara utan att ange underlag rör inte kopplingarna.
    verifikat::spara_utkast(
        &db,
        &klocka(),
        &anna,
        id,
        Some(utkast),
        &formulär("2026-03-15"),
    )
    .await
    .unwrap();
    assert_eq!(verifikatet(&db, &anna, id, utkast).await.underlag.len(), 1);
}

#[tokio::test]
async fn ett_underlag_kan_höra_till_flera_verifikat() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    let kvitto = ladda_upp(&db, &anna, id, "kvitto.pdf").await;
    spara(&db, &anna, id, None, &[kvitto]).await.unwrap();
    spara(&db, &anna, id, None, &[kvitto]).await.unwrap();

    let u = underlag::underlaget(&db, &anna, id, kvitto)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(u.kopplingar.len(), 2);
}

#[tokio::test]
async fn ett_underlag_i_ett_annat_företag_kan_inte_kopplas() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    let annat = annat_företag(&db, &anna).await;
    let främmande = ladda_upp(&db, &anna, annat, "kvitto.pdf").await;

    let fel = spara(&db, &anna, id, None, &[främmande]).await.unwrap_err();

    assert!(matches!(fel, VerifikatFel::OkäntUnderlag), "{fel:?}");
    assert!(
        verifikat::lista(&db, &anna, id, verifikat::Urval::Alla)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn kopplingarna_blir_permanenta_när_utkastet_bokförs() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    let kvitto = ladda_upp(&db, &anna, id, "kvitto.pdf").await;
    let utkast = spara(&db, &anna, id, None, &[kvitto]).await.unwrap();

    let bokfört =
        verifikat::bokför_utkast(&db, &klocka(), &anna, id, utkast, &formulär("2026-03-14"))
            .await
            .unwrap();

    let v = verifikatet(&db, &anna, id, bokfört.id).await;
    assert_eq!(v.underlag.len(), 1);
    assert!(!v.underlag[0].komplettering);
    // Bokföringen refererar till underlaget och dess hash (VF2).
    let post = &v.historik.last().unwrap().post;
    assert_eq!(post.typ, "verifikat bokfört");
    assert_eq!(post.beskrivning["underlag"][0]["filnamn"], "kvitto.pdf");
    assert_eq!(
        post.beskrivning["underlag"][0]["sha256"],
        v.underlag[0].sha256.as_str()
    );

    // Ett bokfört verifikats kopplingar kan inte ändras genom core …
    let fel = spara(&db, &anna, id, Some(bokfört.id), &[])
        .await
        .unwrap_err();
    assert!(matches!(fel, VerifikatFel::IngetUtkast), "{fel:?}");
    // … och schemat avvisar att kopplingen tas bort eller ändras.
    let fel = testhjalp::kör_sql(&db, "DELETE FROM underlagskoppling")
        .await
        .unwrap_err();
    assert!(fel.to_string().contains("går inte att ta bort"), "{fel}");
    let fel = testhjalp::kör_sql(&db, "UPDATE underlagskoppling SET komplettering = 1")
        .await
        .unwrap_err();
    assert!(fel.to_string().contains("går inte att ändra"), "{fel}");
    assert_eq!(
        verifikatet(&db, &anna, id, bokfört.id).await.underlag.len(),
        1
    );
}

#[tokio::test]
async fn ett_nytt_verifikat_bokförs_med_sina_underlag_i_samma_transaktion() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    let kvitto = ladda_upp(&db, &anna, id, "kvitto.pdf").await;

    let bokfört = verifikat::bokför_med_underlag(
        &db,
        &klocka(),
        &anna,
        id,
        None,
        &formulär("2026-03-14"),
        &[kvitto],
    )
    .await
    .unwrap();

    assert_eq!(
        verifikatet(&db, &anna, id, bokfört.id).await.underlag[0].id,
        kvitto
    );
    assert!(okopplade(&db, &anna, id).await.is_empty());

    // En spärr bokför inget och kopplar inget.
    let annat = ladda_upp(&db, &anna, id, "annat.pdf").await;
    let mut obalanserat = formulär("2026-03-14");
    obalanserat.rader[1].kredit = "999".into();
    verifikat::bokför_med_underlag(&db, &klocka(), &anna, id, None, &obalanserat, &[annat])
        .await
        .unwrap_err();
    assert_eq!(okopplade(&db, &anna, id).await, [annat]);
}

#[tokio::test]
async fn ett_raderat_utkasts_underlag_blir_okopplade() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    let kvitto = ladda_upp(&db, &anna, id, "kvitto.pdf").await;
    let utkast = spara(&db, &anna, id, None, &[kvitto]).await.unwrap();

    verifikat::radera_utkast(&db, &klocka(), &anna, id, utkast)
        .await
        .unwrap();

    assert_eq!(okopplade(&db, &anna, id).await, [kvitto]);
    let historik = behandlingshistorik::läs(&db, &anna, id).await.unwrap();
    let post = historik.last().unwrap();
    assert_eq!(post.typ, "utkast raderat");
    assert_eq!(post.beskrivning["underlag"][0]["filnamn"], "kvitto.pdf");
}

#[tokio::test]
async fn ett_bokfört_verifikat_kompletteras_och_kompletteringen_loggas() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    let kvitto = ladda_upp(&db, &anna, id, "kvitto.pdf").await;
    let bokfört = testhjalp::bokför(
        &db,
        &anna,
        id,
        "2026-03-14",
        &[(6540, 1000, 0), (1930, 0, 1000)],
    )
    .await;
    let sen = Klocka::fast("2026-10-20T09:30:00Z".parse().unwrap());

    underlag::komplettera(&db, &sen, &anna, id, bokfört.id, kvitto)
        .await
        .unwrap();

    let v = verifikatet(&db, &anna, id, bokfört.id).await;
    assert_eq!(v.underlag.len(), 1);
    assert!(v.underlag[0].komplettering);
    assert_eq!(v.underlag[0].kopplad.aktör, anna);
    assert_eq!(
        v.underlag[0].kopplad.tid,
        "2026-10-20T09:30:00Z".parse().unwrap()
    );
    // Verifikatet i övrigt är oförändrat.
    assert_eq!(v.nummer, Some(bokfört.nummer));
    let post = &v.historik.last().unwrap().post;
    assert_eq!(post.typ, "verifikat kompletterat");
    assert_eq!(post.objekt, "V1");
    assert_eq!(post.beskrivning["filnamn"], "kvitto.pdf");
    assert_eq!(post.beskrivning["sha256"], v.underlag[0].sha256.as_str());
    assert!(okopplade(&db, &anna, id).await.is_empty());

    // Kompletteringen kan inte tas bort, och underlaget inte heller.
    testhjalp::kör_sql(&db, "DELETE FROM underlagskoppling")
        .await
        .unwrap_err();
    assert!(matches!(
        underlag::ta_bort(&db, &klocka(), &anna, id, kvitto).await,
        Err(UnderlagFel::HörTillBokfört)
    ));
}

#[tokio::test]
async fn en_ny_fil_laddas_upp_och_kompletterar_i_samma_transaktion() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    let bokfört = testhjalp::bokför(
        &db,
        &anna,
        id,
        "2026-03-14",
        &[(6540, 1000, 0), (1930, 0, 1000)],
    )
    .await;

    let u = underlag::ladda_upp_och_komplettera(
        &db,
        &klocka(),
        &anna,
        id,
        bokfört.id,
        ("sent-kvitto.pdf", "", KVITTO),
    )
    .await
    .unwrap();

    let v = verifikatet(&db, &anna, id, bokfört.id).await;
    assert_eq!(v.underlag[0].id, u.id);
    assert!(v.underlag[0].komplettering);

    // Ett utkast kompletteras inte, och då laddas inget upp.
    let utkast = spara(&db, &anna, id, None, &[]).await.unwrap();
    assert!(matches!(
        underlag::ladda_upp_och_komplettera(
            &db,
            &klocka(),
            &anna,
            id,
            utkast,
            ("x.pdf", "", KVITTO)
        )
        .await,
        Err(UnderlagFel::IngetBokförtVerifikat)
    ));
    assert_eq!(
        underlag::lista(&db, &anna, id, Urval::Alla)
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn bara_ett_bokfört_verifikat_kompletteras_och_bara_en_gång_per_underlag() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    let kvitto = ladda_upp(&db, &anna, id, "kvitto.pdf").await;
    let utkast = spara(&db, &anna, id, None, &[]).await.unwrap();
    assert!(matches!(
        underlag::komplettera(&db, &klocka(), &anna, id, utkast, kvitto).await,
        Err(UnderlagFel::IngetBokförtVerifikat)
    ));

    let bokfört = testhjalp::bokför(
        &db,
        &anna,
        id,
        "2026-03-14",
        &[(6540, 1000, 0), (1930, 0, 1000)],
    )
    .await;
    underlag::komplettera(&db, &klocka(), &anna, id, bokfört.id, kvitto)
        .await
        .unwrap();
    assert!(matches!(
        underlag::komplettera(&db, &klocka(), &anna, id, bokfört.id, kvitto).await,
        Err(UnderlagFel::RedanKopplat)
    ));

    let annat = annat_företag(&db, &anna).await;
    let främmande = ladda_upp(&db, &anna, annat, "x.pdf").await;
    assert!(matches!(
        underlag::komplettera(&db, &klocka(), &anna, id, bokfört.id, främmande).await,
        Err(UnderlagFel::FinnsInte)
    ));
}

#[tokio::test]
async fn ett_underlag_som_aldrig_hört_till_ett_bokfört_verifikat_tas_bort_och_det_loggas() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    let fel_fil = ladda_upp(&db, &anna, id, "fel.pdf").await;
    let i_utkast = ladda_upp(&db, &anna, id, "utkastets.pdf").await;
    let utkast = spara(&db, &anna, id, None, &[i_utkast]).await.unwrap();

    underlag::ta_bort(&db, &klocka(), &anna, id, fel_fil)
        .await
        .unwrap();
    underlag::ta_bort(&db, &klocka(), &anna, id, i_utkast)
        .await
        .unwrap();

    assert!(
        underlag::lista(&db, &anna, id, Urval::Alla)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        verifikatet(&db, &anna, id, utkast)
            .await
            .underlag
            .is_empty()
    );
    let historik = behandlingshistorik::läs(&db, &anna, id).await.unwrap();
    let post = &historik[historik.len() - 2];
    assert_eq!(post.typ, "underlag borttaget");
    assert_eq!(post.objekt, "fel.pdf");
    assert_eq!(post.beskrivning["sha256"].as_str().unwrap().len(), 64);
    assert!(matches!(
        underlag::ta_bort(&db, &klocka(), &anna, id, fel_fil).await,
        Err(UnderlagFel::FinnsInte)
    ));
}

#[tokio::test]
async fn ett_underlag_som_hört_till_ett_bokfört_verifikat_kan_aldrig_tas_bort() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    let kvitto = ladda_upp(&db, &anna, id, "kvitto.pdf").await;
    verifikat::bokför_med_underlag(
        &db,
        &klocka(),
        &anna,
        id,
        None,
        &formulär("2026-03-14"),
        &[kvitto],
    )
    .await
    .unwrap();

    assert!(matches!(
        underlag::ta_bort(&db, &klocka(), &anna, id, kvitto).await,
        Err(UnderlagFel::HörTillBokfört)
    ));
    let u = underlag::underlaget(&db, &anna, id, kvitto)
        .await
        .unwrap()
        .unwrap();
    assert!(!u.kan_tas_bort());

    // Schemat avvisar det också, och att originalet ändras (VF5).
    let fel = testhjalp::kör_sql(&db, "DELETE FROM underlag")
        .await
        .unwrap_err();
    assert!(fel.to_string().contains("går inte att ta bort"), "{fel}");
    let fel = testhjalp::kör_sql(&db, "UPDATE underlag SET innehall = x'00', storlek = 1")
        .await
        .unwrap_err();
    assert!(fel.to_string().contains("går inte att ändra"), "{fel}");
}
