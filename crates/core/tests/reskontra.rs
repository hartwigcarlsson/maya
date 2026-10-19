//! Reskontrorna genom `core` mot en tillfällig databas (#151): reskontran per datum med öppna
//! poster per faktura och avstämningen mot 1510 och 2440 (R4). Summan av det som återstår på
//! fakturor vars fordran eller skuld är bokförd jämförs med kontots saldo per samma datum, och en
//! avvikelse visas med differensen.

use jiff::civil::Date;
use maya_core::betalning::{self, Faktura};
use maya_core::reskontra::{self, Läge};
use maya_core::{Databas, Klocka, testhjalp};
use maya_core::{aktor::Aktör, foretag::FöretagsId};
use maya_domain::betalning::NyBetalning;

const NU: &str = "2026-10-18T08:00:00Z";

fn klocka() -> Klocka {
    Klocka::fast(NU.parse().unwrap())
}

fn dag(text: &str) -> Date {
    text.parse().unwrap()
}

fn betalning(betaldag: &str, belopp: &str) -> NyBetalning {
    NyBetalning {
        betaldag: betaldag.into(),
        belopp: belopp.into(),
        konto: "1930".into(),
        ..Default::default()
    }
}

async fn betala(db: &Databas, anna: &Aktör, id: FöretagsId, faktura: Faktura, b: &NyBetalning) {
    betalning::registrera(db, &klocka(), anna, id, faktura, b)
        .await
        .expect("betalningen registreras");
}

#[tokio::test]
async fn reskontran_per_datum_visar_öppna_poster_och_stämmer_med_1510_och_2440() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::fakturaföretag(&db).await;
    let k = testhjalp::utfärdad_kundfaktura(&db, &anna, id, "Kund AB", "2026-03-10").await;
    let l = testhjalp::leverantör(&db, &anna, id, "Kontorsvaror AB").await;
    let l = testhjalp::registrerad_leverantörsfaktura(&db, &anna, id, l, "8812", "").await;
    betala(
        &db,
        &anna,
        id,
        Faktura::Kund(k.id),
        &betalning("2026-04-02", "1 875"),
    )
    .await;
    betala(
        &db,
        &anna,
        id,
        Faktura::Leverantör(l),
        &betalning("2026-04-13", "1 250"),
    )
    .await;

    // Före betalningarna är båda fakturorna öppna med hela beloppet.
    let r = reskontra::reskontran(&db, &anna, id, dag("2026-03-31"))
        .await
        .unwrap();
    assert_eq!(r.dag, dag("2026-03-31"));
    let kund: Vec<_> = r
        .kund
        .poster
        .iter()
        .map(|p| {
            (
                p.fakturanummer.as_str(),
                p.motpart.as_str(),
                p.total,
                p.återstår,
                p.bokfört,
                p.läge,
            )
        })
        .collect();
    assert_eq!(
        kund,
        [(
            "1",
            "Kund AB",
            1_187_500,
            1_187_500,
            1_187_500,
            Läge::Bokförd
        )]
    );
    assert_eq!(r.kund.återstår, 1_187_500);
    assert_eq!(r.kund.bokfört, 1_187_500);
    assert_eq!(r.kund.saldo, 1_187_500);
    assert_eq!(r.kund.differens, 0);
    let lev: Vec<_> = r
        .leverantör
        .poster
        .iter()
        .map(|p| (p.fakturanummer.as_str(), p.motpart.as_str(), p.återstår))
        .collect();
    assert_eq!(lev, [("8812", "Kontorsvaror AB", 125_000)]);
    assert_eq!((r.leverantör.saldo, r.leverantör.differens), (125_000, 0));

    // En fordran före fakturadatumet finns inte; den betalda skulden är inte öppen.
    let r = reskontra::reskontran(&db, &anna, id, dag("2026-03-01"))
        .await
        .unwrap();
    assert!(r.kund.poster.is_empty());
    let r = reskontra::reskontran(&db, &anna, id, dag("2026-10-18"))
        .await
        .unwrap();
    assert_eq!(r.kund.poster[0].återstår, 1_000_000);
    assert_eq!((r.kund.saldo, r.kund.differens), (1_000_000, 0));
    assert!(r.leverantör.poster.is_empty());
    assert_eq!((r.leverantör.saldo, r.leverantör.differens), (0, 0));
}

#[tokio::test]
async fn ett_verifikat_på_1510_utanför_reskontran_ger_en_differens_från_sitt_datum() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::fakturaföretag(&db).await;
    testhjalp::utfärdad_kundfaktura(&db, &anna, id, "Kund AB", "2026-03-10").await;
    testhjalp::bokför(
        &db,
        &anna,
        id,
        "2026-05-01",
        &[(1510, 50_000, 0), (3001, 0, 50_000)],
    )
    .await;
    testhjalp::bokför(
        &db,
        &anna,
        id,
        "2026-06-01",
        &[(2440, 20_000, 0), (1930, 0, 20_000)],
    )
    .await;

    let r = reskontra::reskontran(&db, &anna, id, dag("2026-04-30"))
        .await
        .unwrap();
    assert_eq!((r.kund.differens, r.leverantör.differens), (0, 0));
    let r = reskontra::reskontran(&db, &anna, id, dag("2026-06-30"))
        .await
        .unwrap();
    assert_eq!(r.kund.saldo, 1_237_500);
    assert_eq!(r.kund.differens, -50_000);
    // 2440 i debet är en negativ skuld.
    assert_eq!(
        (r.leverantör.saldo, r.leverantör.differens),
        (-20_000, 20_000)
    );
}

#[tokio::test]
async fn under_kontantmetoden_står_bara_det_som_krediten_bokförde_utöver_det_obetalda_på_1510() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::fakturaföretag_med_metod(&db, "kontant").await;
    let k = testhjalp::utfärdad_kundfaktura(&db, &anna, id, "Kund AB", "2026-03-10").await;
    // Betald mot intäkt och moms: inget på 1510, och fakturan är inte öppen.
    betala(
        &db,
        &anna,
        id,
        Faktura::Kund(k.id),
        &betalning("2026-04-02", "11 875"),
    )
    .await;
    let r = reskontra::reskontran(&db, &anna, id, dag("2026-04-30"))
        .await
        .unwrap();
    assert!(r.kund.poster.is_empty());
    // En kredit av den betalda fakturan bokförs mot 1510 och ger ett tillgodohavande.
    let hela = maya_domain::kreditfaktura::NyKreditering {
        hela: true,
        antal: Vec::new(),
    };
    let utkast = maya_core::kundfaktura::kreditera(&db, &klocka(), &anna, id, k.id, &hela)
        .await
        .unwrap();
    maya_core::kundfaktura::utfärda(&db, &klocka(), &anna, id, utkast)
        .await
        .unwrap();
    let r = reskontra::reskontran(&db, &anna, id, dag("2026-10-18"))
        .await
        .unwrap();
    let p = &r.kund.poster[0];
    assert_eq!(
        (p.återstår, p.bokfört, p.läge),
        (-1_187_500, -1_187_500, Läge::InteBokförd)
    );
    assert_eq!((r.kund.saldo, r.kund.differens), (-1_187_500, 0));

    // En obetald faktura under kontantmetoden är öppen men står inte på 1510.
    let k2 = testhjalp::utfärdad_kundfaktura(&db, &anna, id, "Kund 2 AB", "2026-05-10").await;
    let r = reskontra::reskontran(&db, &anna, id, dag("2026-10-18"))
        .await
        .unwrap();
    let p = r
        .kund
        .poster
        .iter()
        .find(|p| p.faktura == Faktura::Kund(k2.id))
        .unwrap();
    assert_eq!(
        (p.återstår, p.bokfört, p.läge),
        (1_187_500, 0, Läge::InteBokförd)
    );
    assert_eq!(
        (r.kund.återstår, r.kund.bokfört, r.kund.differens),
        (0, -1_187_500, 0)
    );
}

#[tokio::test]
async fn efter_årets_slut_väntar_det_obetalda_på_t2_och_står_på_1510_när_t2_är_bokfört() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::fakturaföretag_med_metod(&db, "kontant").await;
    let k = testhjalp::utfärdad_kundfaktura(&db, &anna, id, "Kund AB", "2026-03-10").await;
    let efter = Klocka::fast("2027-01-20T10:00:00Z".parse().unwrap());
    // En delbetalning efter årets slut bokförs mot 1510 innan T2 är bokfört.
    let b = betalning("2027-01-15", "1 875");
    betalning::registrera(&db, &efter, &anna, id, Faktura::Kund(k.id), &b)
        .await
        .unwrap();
    let r = reskontra::reskontran(&db, &anna, id, dag("2027-01-20"))
        .await
        .unwrap();
    let p = &r.kund.poster[0];
    assert_eq!(
        (p.återstår, p.bokfört, p.läge),
        (1_000_000, -187_500, Läge::VäntarPåT2)
    );
    assert_eq!((r.kund.saldo, r.kund.differens), (-187_500, 0));
    // Vid årets slut är fakturan inte bokförd.
    let r = reskontra::reskontran(&db, &anna, id, dag("2026-12-31"))
        .await
        .unwrap();
    assert_eq!(r.kund.poster[0].läge, Läge::InteBokförd);

    maya_core::t2::kör(&db, &efter).await.unwrap();
    let t2 = maya_core::t2::lista(&db, &anna, id)
        .await
        .unwrap()
        .remove(0);
    maya_core::t2::bokför(&db, &efter, &anna, id, t2.verifikat)
        .await
        .unwrap();
    for (d, återstår) in [("2026-12-31", 1_187_500), ("2027-01-20", 1_000_000)] {
        let r = reskontra::reskontran(&db, &anna, id, dag(d)).await.unwrap();
        let p = &r.kund.poster[0];
        assert_eq!(
            (p.återstår, p.bokfört, p.läge),
            (återstår, återstår, Läge::Bokförd),
            "{d}"
        );
        assert_eq!((r.kund.saldo, r.kund.differens), (återstår, 0), "{d}");
    }
}

#[tokio::test]
async fn en_upplåst_faktura_står_på_2440_med_det_som_betalningarna_bokförde() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::fakturaföretag(&db).await;
    let l = testhjalp::leverantör(&db, &anna, id, "Kontorsvaror AB").await;
    let l = testhjalp::registrerad_leverantörsfaktura(&db, &anna, id, l, "8812", "").await;
    betala(
        &db,
        &anna,
        id,
        Faktura::Leverantör(l),
        &betalning("2026-04-13", "250"),
    )
    .await;
    maya_core::leverantorsfaktura::rätta_avskrift(&db, &klocka(), &anna, id, l, "fel konto")
        .await
        .unwrap();
    let r = reskontra::reskontran(&db, &anna, id, dag("2026-10-18"))
        .await
        .unwrap();
    let p = &r.leverantör.poster[0];
    assert_eq!(
        (p.återstår, p.bokfört, p.läge),
        (100_000, -25_000, Läge::Upplåst)
    );
    assert_eq!((r.leverantör.saldo, r.leverantör.differens), (-25_000, 0));
    // Före rättelsen var den bokförd.
    let r = reskontra::reskontran(&db, &anna, id, dag("2026-10-17"))
        .await
        .unwrap();
    assert_eq!(r.leverantör.poster[0].läge, Läge::Bokförd);
    assert_eq!(r.leverantör.differens, 0);
}
