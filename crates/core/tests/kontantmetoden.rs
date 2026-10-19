//! Kontantmetoden för fakturor genom `core` mot en tillfällig databas (#149): varje rad i
//! tabellen över automatkonteringar under båda bokföringsmetoderna. Under fakturametoden är
//! fakturans fordran eller skuld bokförd från utfärdandet eller registreringen; under
//! kontantmetoden bokförs inget då, och betalning, kredit, kundförlust, öresutjämning och
//! kursdifferens bokförs enligt raderna "fordran/skuld inte bokförd". Bokföringsmetoden på
//! fakturadatumet avgör, så ett metodbyte kräver ingen övergångsbokning.

use maya_core::betalning::{self, Faktura};
use maya_core::kundfaktura::{self, KundfakturaFel, KundfakturaId};
use maya_core::leverantorsfaktura::{self, LeverantörsfakturaId};
use maya_core::verifikat::{self, VerifikatFel, VerifikatId};
use maya_core::{Databas, Klocka, extern_kundfaktura, foretag, kontering, kundforlust, testhjalp};
use maya_core::{aktor::Aktör, foretag::FöretagsId, underlag};
use maya_domain::betalning::NyBetalning;
use maya_domain::kreditfaktura::NyKreditering;
use maya_domain::kundforlust::NyKundförlust;
use maya_domain::leverantorsfaktura::{NyLeverantörsfaktura, NyRad};

const NU: &str = "2026-10-18T08:00:00Z";

fn klocka() -> Klocka {
    Klocka::fast(NU.parse().unwrap())
}

/// Båda bokföringsmetoderna, med om fordran eller skulden bokförs när fakturan utfärdas eller
/// registreras.
const METODER: [(&str, bool); 2] = [("faktura", true), ("kontant", false)];

/// Kundfakturan från `testhjalp::kundfakturautkast`: 3001 netto 9 500 kr och 2611 2 375 kr.
const TOTAL: i64 = 1_187_500;

async fn rader(
    db: &Databas, anna: &Aktör, id: FöretagsId, v: VerifikatId
) -> Vec<(u16, i64, i64)> {
    let mut r: Vec<_> = verifikat::verifikatet(db, anna, id, v)
        .await
        .unwrap()
        .unwrap()
        .rader
        .iter()
        .map(|r| (r.konto.unwrap().get(), r.debet.öre(), r.kredit.öre()))
        .collect();
    r.sort();
    r
}

fn sorterat(mut r: Vec<(u16, i64, i64)>) -> Vec<(u16, i64, i64)> {
    r.sort();
    r
}

async fn antal_verifikat(db: &Databas, anna: &Aktör, id: FöretagsId) -> usize {
    verifikat::lista(db, anna, id, verifikat::Urval::Bokförda)
        .await
        .unwrap()
        .len()
}

fn betalning(betaldag: &str, belopp: &str) -> NyBetalning {
    NyBetalning {
        betaldag: betaldag.into(),
        belopp: belopp.into(),
        konto: "1930".into(),
        ..Default::default()
    }
}

async fn betala(
    db: &Databas,
    anna: &Aktör,
    id: FöretagsId,
    faktura: Faktura,
    b: &NyBetalning,
) -> betalning::Registrerad {
    betalning::registrera(db, &klocka(), anna, id, faktura, b)
        .await
        .unwrap()
}

async fn reskontrapost(
    db: &Databas,
    anna: &Aktör,
    id: FöretagsId,
    faktura: Faktura,
) -> betalning::Reskontrapost {
    betalning::reskontrapost(db, anna, id, faktura)
        .await
        .unwrap()
}

async fn kundfaktura(db: &Databas, metod: &str) -> (Aktör, FöretagsId, KundfakturaId) {
    let (anna, id) = testhjalp::fakturaföretag_med_metod(db, metod).await;
    let f = testhjalp::utfärdad_kundfaktura(db, &anna, id, "Kund AB", "2026-03-10").await;
    (anna, id, f.id)
}

// Utfärda eller registrera kundfaktura

#[tokio::test]
async fn utfärdandet_bokförs_under_fakturametoden_men_inte_under_kontantmetoden() {
    for (metod, bokförd) in METODER {
        let db = Databas::tillfällig().await;
        let (anna, id) = testhjalp::fakturaföretag_med_metod(&db, metod).await;
        let f = testhjalp::utfärdad_kundfaktura(&db, &anna, id, "Kund AB", "2026-03-10").await;
        assert_eq!(f.nummer, 1, "{metod}");
        match f.verifikat {
            Some(v) => {
                assert!(bokförd, "{metod}");
                assert_eq!(
                    rader(&db, &anna, id, v.id).await,
                    [(1510, TOTAL, 0), (2611, 0, 237_500), (3001, 0, 950_000)]
                );
            }
            None => {
                assert!(!bokförd, "{metod}");
                assert_eq!(antal_verifikat(&db, &anna, id).await, 0);
            }
        }
        let k = kundfaktura::kundfakturan(&db, &anna, id, f.id)
            .await
            .unwrap();
        let text = k.historik.last().unwrap().post.text();
        assert_eq!(
            text.ends_with("inte bokförd (kontantmetoden)"),
            !bokförd,
            "{metod}: {text}"
        );
        let u = k.utfärdad.unwrap();
        assert_eq!(u.verifikat.is_some(), bokförd, "{metod}");
        assert_eq!(u.pdf.underlag, f.underlag);
        // Den arkiverade PDF:en hör till fakturan och är inget okopplat underlag.
        let okopplade = underlag::lista(&db, &anna, id, underlag::Urval::Okopplade)
            .await
            .unwrap();
        assert!(okopplade.is_empty(), "{metod}: {okopplade:?}");
        let post = reskontrapost(&db, &anna, id, Faktura::Kund(f.id)).await;
        assert_eq!((post.återstår, post.bokförd), (TOTAL, bokförd), "{metod}");
    }
}

async fn extern_faktura(db: &Databas, metod: &str) -> (Aktör, FöretagsId, KundfakturaId) {
    let (anna, id) = testhjalp::fakturaföretag_med_metod(db, metod).await;
    let kund = testhjalp::kund(db, &anna, id, "Kund AB").await;
    let f = testhjalp::registrerad_extern_kundfaktura(db, &anna, id, kund, "A-17").await;
    (anna, id, f)
}

#[tokio::test]
async fn registreringen_av_en_extern_kundfaktura_bokförs_bara_under_fakturametoden() {
    for (metod, bokförd) in METODER {
        let db = Databas::tillfällig().await;
        let (anna, id, f) = extern_faktura(&db, metod).await;
        let e = extern_kundfaktura::fakturan(&db, &anna, id, f)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(e.verifikat.is_some(), bokförd, "{metod}");
        assert_eq!(antal_verifikat(&db, &anna, id).await, usize::from(bokförd));
        // Betalningen: mot 1510, eller mot intäkt och moms.
        let r = betala(
            &db,
            &anna,
            id,
            Faktura::Kund(f),
            &betalning("2026-04-09", "12 500"),
        )
        .await;
        let väntat = if bokförd {
            vec![(1510, 0, 1_250_000), (1930, 1_250_000, 0)]
        } else {
            vec![
                (1930, 1_250_000, 0),
                (2611, 0, 250_000),
                (3001, 0, 1_000_000),
            ]
        };
        assert_eq!(
            rader(&db, &anna, id, r.verifikat.id).await,
            väntat,
            "{metod}"
        );
    }
}

#[tokio::test]
async fn en_ny_kund_från_det_externa_utkastet_blir_verklig_under_båda_metoderna() {
    use maya_domain::extern_kundfaktura::{NyExternFaktura, NyExternRad};
    for (metod, bokförd) in METODER {
        let db = Databas::tillfällig().await;
        let (anna, id) = testhjalp::fakturaföretag_med_metod(&db, metod).await;
        testhjalp::kund(&db, &anna, id, "Kund AB").await;
        let (f, _) = extern_kundfaktura::ladda_upp(
            &db,
            &klocka(),
            &anna,
            id,
            (
                "A-18.pdf",
                "application/pdf",
                b"%PDF-1.4 extern kundfaktura A-18",
            ),
        )
        .await
        .unwrap();
        let utkast = NyExternFaktura {
            kund: maya_domain::kundfaktura::NY.into(),
            ny_kund: maya_domain::kund::NyKund {
                namn: "Nya Kunden AB".into(),
                betalningsvillkor: "10".into(),
                ..Default::default()
            },
            fakturanummer: "A-18".into(),
            fakturadatum: "2026-03-10".into(),
            rader: vec![NyExternRad {
                momssats: "25".into(),
                konto: "3001".into(),
                belopp: "10 000".into(),
            }],
            ..Default::default()
        };
        extern_kundfaktura::spara_utkast(&db, &klocka(), &anna, id, f, &utkast)
            .await
            .unwrap();
        let formulär = extern_kundfaktura::fakturan(&db, &anna, id, f)
            .await
            .unwrap()
            .unwrap()
            .formulär;
        let r = extern_kundfaktura::registrera(&db, &klocka(), &anna, id, f, &formulär)
            .await
            .unwrap();
        assert_eq!(r.verifikat.is_some(), bokförd, "{metod}");

        let kunder: Vec<String> = maya_core::kund::kunder(&db, &anna, id)
            .await
            .unwrap()
            .into_iter()
            .map(|k| k.namn)
            .collect();
        assert_eq!(kunder, ["Kund AB", "Nya Kunden AB"], "{metod}");
        let e = extern_kundfaktura::fakturan(&db, &anna, id, f)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(e.kund, "Nya Kunden AB", "{metod}");

        // Betalningen bokförs enligt metoden även för den nya kundens faktura.
        let b = betala(
            &db,
            &anna,
            id,
            Faktura::Kund(f),
            &betalning("2026-03-20", "12 500"),
        )
        .await;
        let väntat = if bokförd {
            vec![(1510, 0, 1_250_000), (1930, 1_250_000, 0)]
        } else {
            vec![
                (1930, 1_250_000, 0),
                (2611, 0, 250_000),
                (3001, 0, 1_000_000),
            ]
        };
        assert_eq!(
            rader(&db, &anna, id, b.verifikat.id).await,
            väntat,
            "{metod}"
        );
    }
}

// Betalning av kundfaktura

#[tokio::test]
async fn betalningen_av_en_kundfaktura_bokförs_mot_1510_eller_mot_intäkt_och_moms_proportionellt() {
    for (metod, bokförd) in METODER {
        let db = Databas::tillfällig().await;
        let (anna, id, f) = kundfaktura(&db, metod).await;
        let r = betala(
            &db,
            &anna,
            id,
            Faktura::Kund(f),
            &betalning("2026-04-01", "5000"),
        )
        .await;
        let väntat = if bokförd {
            vec![(1510, 0, 500_000), (1930, 500_000, 0)]
        } else {
            vec![(1930, 500_000, 0), (2611, 0, 100_000), (3001, 0, 400_000)]
        };
        assert_eq!(
            rader(&db, &anna, id, r.verifikat.id).await,
            väntat,
            "{metod}"
        );
        assert_eq!(r.återstår, TOTAL - 500_000);
        let v = verifikat::verifikatet(&db, &anna, id, r.verifikat.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            v.datum.map(|d| d.to_string()).as_deref(),
            Some("2026-04-01")
        );
    }
}

#[tokio::test]
async fn öresutjämningen_bokförs_mot_3740_bara_när_fordran_är_bokförd() {
    for (metod, bokförd) in METODER {
        let db = Databas::tillfällig().await;
        let (anna, id, f) = kundfaktura(&db, metod).await;
        let b = NyBetalning {
            öresutjämning: true,
            ..betalning("2026-04-01", "11 865")
        };
        let r = betala(&db, &anna, id, Faktura::Kund(f), &b).await;
        assert_eq!(r.återstår, 0, "{metod}");
        let väntat = if bokförd {
            vec![(1510, 0, TOTAL), (1930, 1_186_500, 0), (3740, 1_000, 0)]
        } else {
            // Resten stängs utan verifikat: intäkten och momsen gäller bara det betalda.
            vec![(1930, 1_186_500, 0), (2611, 0, 237_300), (3001, 0, 949_200)]
        };
        assert_eq!(
            rader(&db, &anna, id, r.verifikat.id).await,
            väntat,
            "{metod}"
        );
    }
}

#[tokio::test]
async fn rätta_betalning_vänder_betalningens_verifikat_under_båda_metoderna() {
    for (metod, _) in METODER {
        let db = Databas::tillfällig().await;
        let (anna, id, f) = kundfaktura(&db, metod).await;
        let r = betala(
            &db,
            &anna,
            id,
            Faktura::Kund(f),
            &betalning("2026-04-01", "5000"),
        )
        .await;
        let rättad = betalning::rätta(&db, &klocka(), &anna, id, r.id, "2026-10-18", "fel belopp")
            .await
            .unwrap();
        assert_eq!(rättad.återstår, TOTAL, "{metod}");
        let original = rader(&db, &anna, id, r.verifikat.id).await;
        let vänd: Vec<_> = original.iter().map(|&(k, d, c)| (k, c, d)).collect();
        assert_eq!(
            rader(&db, &anna, id, rättad.rättelse.id).await,
            sorterat(vänd),
            "{metod}"
        );
    }
}

#[tokio::test]
async fn en_betalning_som_en_bokförd_kredit_bygger_på_rättas_inte_under_kontantmetoden() {
    let db = Databas::tillfällig().await;
    let (anna, id, f) = kundfaktura(&db, "kontant").await;
    let r = betala(
        &db,
        &anna,
        id,
        Faktura::Kund(f),
        &betalning("2026-04-01", "11 875"),
    )
    .await;
    // Krediten av den betalda fakturan bokförs mot 1510, eftersom inget var obetalt.
    let k = kreditera_hela(&db, &anna, id, f).await;
    assert!(k.verifikat.is_some());
    // Utan betalningen hade krediten rymts i det obetalda: en rättelse av betalningen skulle
    // lämna krediten på 1510 fast fakturan inte längre har något tillgodo.
    let fel = betalning::rätta(&db, &klocka(), &anna, id, r.id, "2026-10-18", "")
        .await
        .unwrap_err();
    assert!(
        matches!(fel, betalning::BetalningFel::BokförtUtifrån),
        "{fel:?}"
    );
    assert_eq!(antal_verifikat(&db, &anna, id).await, 2);
}

// Kreditfaktura och återbetalning till kund

async fn kreditera_hela(
    db: &Databas,
    anna: &Aktör,
    id: FöretagsId,
    f: KundfakturaId,
) -> kundfaktura::Utfärdad {
    let utkast = kundfaktura::kreditera(
        db,
        &klocka(),
        anna,
        id,
        f,
        &NyKreditering {
            hela: true,
            antal: Vec::new(),
        },
    )
    .await
    .unwrap();
    kundfaktura::utfärda(db, &klocka(), anna, id, utkast)
        .await
        .unwrap()
}

#[tokio::test]
async fn en_återbetalning_dateras_inte_före_tillgodohavandet() {
    for (metod, _) in METODER {
        let db = Databas::tillfällig().await;
        let (anna, id, f) = kundfaktura(&db, metod).await;
        betala(
            &db,
            &anna,
            id,
            Faktura::Kund(f),
            &betalning("2026-04-02", "11 875"),
        )
        .await;
        // Krediten är daterad i dag (2026-10-18), så tillgodohavandet finns från då.
        kreditera_hela(&db, &anna, id, f).await;
        let fel = betalning::registrera(
            &db,
            &klocka(),
            &anna,
            id,
            Faktura::Kund(f),
            &betalning("2026-06-02", "11 875"),
        )
        .await
        .unwrap_err();
        assert!(
            matches!(&fel, betalning::BetalningFel::Spärrat(s) if s.iter().map(|s| s.kod).eq(["aterbetalning_fore_tillgodo"])),
            "{metod}: {fel:?}"
        );
        betala(
            &db,
            &anna,
            id,
            Faktura::Kund(f),
            &betalning("2026-10-18", "11 875"),
        )
        .await;
    }
}

#[tokio::test]
async fn kredit_av_en_obokförd_obetald_faktura_bokförs_inte() {
    let db = Databas::tillfällig().await;
    let (anna, id, f) = kundfaktura(&db, "kontant").await;
    let k = kreditera_hela(&db, &anna, id, f).await;
    assert_eq!(k.verifikat, None);
    assert_eq!(antal_verifikat(&db, &anna, id).await, 0);
    assert_eq!(
        reskontrapost(&db, &anna, id, Faktura::Kund(f))
            .await
            .återstår,
        0
    );
}

#[tokio::test]
async fn kredit_bokförs_mot_1510_för_hela_krediten_eller_för_det_som_överstiger_det_obetalda() {
    for (metod, bokförd) in METODER {
        let db = Databas::tillfällig().await;
        let (anna, id, f) = kundfaktura(&db, metod).await;
        // 5 000 kr är betalt och 6 875 kr obetalt när hela fakturan krediteras.
        betala(
            &db,
            &anna,
            id,
            Faktura::Kund(f),
            &betalning("2026-04-01", "5000"),
        )
        .await;
        let k = kreditera_hela(&db, &anna, id, f).await;
        let väntat = if bokförd {
            vec![(1510, 0, TOTAL), (2611, 237_500, 0), (3001, 950_000, 0)]
        } else {
            vec![(1510, 0, 500_000), (2611, 100_000, 0), (3001, 400_000, 0)]
        };
        assert_eq!(
            rader(&db, &anna, id, k.verifikat.unwrap().id).await,
            väntat,
            "{metod}"
        );
        // Kunden har 5 000 kr tillgodo, och återbetalningen bokförs mot 1510 under båda.
        assert_eq!(
            reskontrapost(&db, &anna, id, Faktura::Kund(f))
                .await
                .återstår,
            -500_000
        );
        let å = betala(
            &db,
            &anna,
            id,
            Faktura::Kund(f),
            &betalning("2026-10-18", "5000"),
        )
        .await;
        assert_eq!(
            rader(&db, &anna, id, å.verifikat.id).await,
            [(1510, 500_000, 0), (1930, 0, 500_000)],
            "{metod}"
        );
    }
}

// Kundförlust

#[tokio::test]
async fn kundförlust_bokförs_när_fordran_är_bokförd_och_ger_annars_bara_status() {
    for (metod, bokförd) in METODER {
        let db = Databas::tillfällig().await;
        let (anna, id, f) = kundfaktura(&db, metod).await;
        let förlust = kundforlust::bokför(
            &db,
            &klocka(),
            &anna,
            id,
            f,
            &NyKundförlust {
                skäl: "konkurs".into(),
                annat: String::new(),
                datum: "2026-09-30".into(),
            },
        )
        .await
        .unwrap();
        assert_eq!(förlust.verifikat.is_some(), bokförd, "{metod}");
        if let Some(v) = förlust.verifikat {
            assert_eq!(
                rader(&db, &anna, id, v.id).await,
                [(1510, 0, TOTAL), (2611, 237_500, 0), (6351, 950_000, 0)]
            );
        }
        let post = reskontrapost(&db, &anna, id, Faktura::Kund(f)).await;
        assert!(post.kundförlust, "{metod}");
        assert_eq!(post.återstår, 0);
        let å = kundforlust::återkalla(&db, &klocka(), &anna, id, förlust.id)
            .await
            .unwrap();
        assert_eq!(å.verifikat.is_some(), bokförd, "{metod}");
        assert_eq!(å.återstår, TOTAL);
        assert_eq!(
            antal_verifikat(&db, &anna, id).await,
            if bokförd { 3 } else { 0 }
        );
    }
}

// Rätta kontering och Rätta registrering

#[tokio::test]
async fn rätta_kontering_finns_inte_när_utfärdandet_saknar_verifikat() {
    let db = Databas::tillfällig().await;
    let (anna, id, f) = kundfaktura(&db, "kontant").await;
    let fel = kontering::kontering(&db, &anna, id, f).await.unwrap_err();
    assert!(
        matches!(fel, KundfakturaFel::Verifikat(VerifikatFel::IngetVerifikat)),
        "{fel:?}"
    );
    let fel = kontering::rätta(
        &db,
        &klocka(),
        &anna,
        id,
        f,
        &maya_domain::kontering::NyKontering {
            konton: vec!["3041".into()],
            orsak: String::new(),
        },
    )
    .await
    .unwrap_err();
    assert!(
        matches!(fel, KundfakturaFel::Verifikat(VerifikatFel::IngetVerifikat)),
        "{fel:?}"
    );
    assert_eq!(antal_verifikat(&db, &anna, id).await, 0);
}

#[tokio::test]
async fn rätta_registrering_under_kontantmetoden_låser_upp_utan_rättelse_och_registrerar_om_utan_verifikat()
 {
    let db = Databas::tillfällig().await;
    let (anna, id, f) = extern_faktura(&db, "kontant").await;
    let r = extern_kundfaktura::rätta_registrering(&db, &klocka(), &anna, id, f, "fel datum")
        .await
        .unwrap();
    assert_eq!(r.rättelse, None);
    let formulär = extern_kundfaktura::fakturan(&db, &anna, id, f)
        .await
        .unwrap()
        .unwrap()
        .formulär;
    let om = extern_kundfaktura::registrera(&db, &klocka(), &anna, id, f, &formulär)
        .await
        .unwrap();
    assert_eq!(om.verifikat, None);
    assert_eq!(antal_verifikat(&db, &anna, id).await, 0);
    assert!(
        !reskontrapost(&db, &anna, id, Faktura::Kund(f))
            .await
            .bokförd
    );
}

#[tokio::test]
async fn en_rättad_registrering_under_fakturametoden_är_inte_längre_bokförd_förrän_den_registreras_om()
 {
    let db = Databas::tillfällig().await;
    let (anna, id, f) = extern_faktura(&db, "faktura").await;
    extern_kundfaktura::rätta_registrering(&db, &klocka(), &anna, id, f, "")
        .await
        .unwrap();
    let formulär = extern_kundfaktura::fakturan(&db, &anna, id, f)
        .await
        .unwrap()
        .unwrap()
        .formulär;
    let om = extern_kundfaktura::registrera(&db, &klocka(), &anna, id, f, &formulär)
        .await
        .unwrap();
    assert!(om.verifikat.is_some());
    assert!(
        reskontrapost(&db, &anna, id, Faktura::Kund(f))
            .await
            .bokförd
    );
    let r = betala(
        &db,
        &anna,
        id,
        Faktura::Kund(f),
        &betalning("2026-04-09", "12 500"),
    )
    .await;
    assert_eq!(
        rader(&db, &anna, id, r.verifikat.id).await,
        [(1510, 0, 1_250_000), (1930, 1_250_000, 0)]
    );
}

// Leverantörsfakturor

async fn leverantörsfaktura(
    db: &Databas,
    metod: &str,
    originalvaluta: &str,
) -> (Aktör, FöretagsId, LeverantörsfakturaId) {
    let (anna, id) = testhjalp::företag_med_metod(db, metod).await;
    let l = testhjalp::leverantör(db, &anna, id, "Kontorsvaror AB").await;
    let f =
        testhjalp::registrerad_leverantörsfaktura(db, &anna, id, l, "8812", originalvaluta).await;
    (anna, id, f)
}

#[tokio::test]
async fn registreringen_av_en_leverantörsfaktura_bokförs_bara_under_fakturametoden() {
    for (metod, bokförd) in METODER {
        let db = Databas::tillfällig().await;
        let (anna, id, f) = leverantörsfaktura(&db, metod, "").await;
        let l = leverantorsfaktura::fakturan(&db, &anna, id, f)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(l.verifikat.is_some(), bokförd, "{metod}");
        assert_eq!(antal_verifikat(&db, &anna, id).await, usize::from(bokförd));
        let post = reskontrapost(&db, &anna, id, Faktura::Leverantör(f)).await;
        assert_eq!((post.återstår, post.bokförd), (125_000, bokförd));
    }
}

#[tokio::test]
async fn betalningen_av_en_leverantörsfaktura_bokförs_mot_2440_eller_mot_kostnad_och_moms() {
    for (metod, bokförd) in METODER {
        let db = Databas::tillfällig().await;
        let (anna, id, f) = leverantörsfaktura(&db, metod, "").await;
        let r = betala(
            &db,
            &anna,
            id,
            Faktura::Leverantör(f),
            &betalning("2026-04-13", "500"),
        )
        .await;
        let väntat = if bokförd {
            vec![(1930, 0, 50_000), (2440, 50_000, 0)]
        } else {
            vec![(1930, 0, 50_000), (2641, 10_000, 0), (5410, 40_000, 0)]
        };
        assert_eq!(
            rader(&db, &anna, id, r.verifikat.id).await,
            väntat,
            "{metod}"
        );
        assert_eq!(r.återstår, 75_000);
    }
}

#[tokio::test]
async fn öresutjämningen_på_en_leverantörsfaktura_bokförs_bara_när_skulden_är_bokförd() {
    for (metod, bokförd) in METODER {
        let db = Databas::tillfällig().await;
        let (anna, id, f) = leverantörsfaktura(&db, metod, "").await;
        let b = NyBetalning {
            öresutjämning: true,
            ..betalning("2026-04-13", "1 245")
        };
        let r = betala(&db, &anna, id, Faktura::Leverantör(f), &b).await;
        assert_eq!(r.återstår, 0);
        let väntat = if bokförd {
            vec![(1930, 0, 124_500), (2440, 125_000, 0), (3740, 0, 500)]
        } else {
            vec![(1930, 0, 124_500), (2641, 24_900, 0), (5410, 99_600, 0)]
        };
        assert_eq!(
            rader(&db, &anna, id, r.verifikat.id).await,
            väntat,
            "{metod}"
        );
    }
}

#[tokio::test]
async fn kursdifferensen_bokförs_i_betalningens_verifikat_under_båda_metoderna() {
    for (metod, bokförd) in METODER {
        let db = Databas::tillfällig().await;
        let (anna, id, f) = leverantörsfaktura(&db, metod, "EUR 109,50").await;
        // Betalningen blev 10 kr mindre i SEK: kursvinst.
        let b = NyBetalning {
            kursdifferens: true,
            ..betalning("2026-04-13", "1 240")
        };
        let r = betala(&db, &anna, id, Faktura::Leverantör(f), &b).await;
        assert_eq!(r.återstår, 0);
        let väntat = if bokförd {
            vec![(1930, 0, 124_000), (2440, 125_000, 0), (3960, 0, 1_000)]
        } else {
            vec![
                (1930, 0, 124_000),
                (2641, 25_000, 0),
                (3960, 0, 1_000),
                (5410, 100_000, 0),
            ]
        };
        assert_eq!(
            rader(&db, &anna, id, r.verifikat.id).await,
            väntat,
            "{metod}"
        );
    }
}

async fn registrera(
    db: &Databas,
    anna: &Aktör,
    id: FöretagsId,
    formulär: NyLeverantörsfaktura,
) -> (LeverantörsfakturaId, leverantorsfaktura::Registrerat) {
    let (f, _) = leverantorsfaktura::ladda_upp(
        db,
        &klocka(),
        anna,
        id,
        (
            &format!("{}.pdf", formulär.fakturanummer),
            "application/pdf",
            format!("%PDF-1.4 {}", formulär.fakturanummer).as_bytes(),
        ),
    )
    .await
    .unwrap();
    let r = leverantorsfaktura::registrera(db, &klocka(), anna, id, f, &formulär)
        .await
        .unwrap();
    (f, r)
}

#[tokio::test]
async fn omvänd_skattskyldighet_bokför_den_fiktiva_momsen_vid_betalningen_när_skulden_inte_är_bokförd()
 {
    for (metod, bokförd) in METODER {
        let db = Databas::tillfällig().await;
        let (anna, id) = testhjalp::företag_med_metod(&db, metod).await;
        let l = testhjalp::leverantör(&db, &anna, id, "Cloud GmbH").await;
        let (f, reg) = registrera(
            &db,
            &anna,
            id,
            NyLeverantörsfaktura {
                leverantör: l.to_string(),
                fakturanummer: "EU-1".into(),
                fakturadatum: "2026-03-14".into(),
                förfallodag: "2026-04-13".into(),
                totalbelopp: "1 000".into(),
                omvänd_skattskyldighet: "eu_tjanst".into(),
                rader: vec![NyRad {
                    konto: "5410".into(),
                    netto: "1 000".into(),
                    momssats: "25".into(),
                    moms: String::new(),
                }],
                ..Default::default()
            },
        )
        .await;
        assert_eq!(reg.verifikat.is_some(), bokförd);
        let r = betala(
            &db,
            &anna,
            id,
            Faktura::Leverantör(f),
            &betalning("2026-04-13", "1 000"),
        )
        .await;
        let väntat = if bokförd {
            vec![(1930, 0, 100_000), (2440, 100_000, 0)]
        } else {
            vec![
                (1930, 0, 100_000),
                (2614, 0, 25_000),
                (2645, 25_000, 0),
                (5410, 100_000, 0),
            ]
        };
        assert_eq!(
            rader(&db, &anna, id, r.verifikat.id).await,
            väntat,
            "{metod}"
        );
    }
}

/// En kreditfaktura på 500 kr (5410 400 kr och 100 kr moms) som hänvisar till `original`.
fn leverantörskredit(l: &str, original: LeverantörsfakturaId) -> NyLeverantörsfaktura {
    NyLeverantörsfaktura {
        leverantör: l.into(),
        fakturanummer: "K-1".into(),
        fakturadatum: "2026-05-02".into(),
        förfallodag: "2026-05-02".into(),
        totalbelopp: "-500".into(),
        krediterar: original.to_string(),
        rader: vec![NyRad {
            konto: "5410".into(),
            netto: "-400".into(),
            momssats: "25".into(),
            moms: String::new(),
        }],
        ..Default::default()
    }
}

#[tokio::test]
async fn kredit_från_leverantören_bokförs_mot_2440_för_hela_krediten_eller_det_som_överstiger_det_obetalda()
 {
    for (metod, bokförd) in METODER {
        let db = Databas::tillfällig().await;
        let (anna, id, f) = leverantörsfaktura(&db, metod, "").await;
        let l = leverantorsfaktura::fakturan(&db, &anna, id, f)
            .await
            .unwrap()
            .unwrap()
            .leverantör
            .unwrap();
        // 1 000 kr betalt, 250 kr obetalt: av krediten på 500 kr överstiger 250 kr det obetalda.
        betala(
            &db,
            &anna,
            id,
            Faktura::Leverantör(f),
            &betalning("2026-04-13", "1 000"),
        )
        .await;
        let (_, r) = registrera(&db, &anna, id, leverantörskredit(&l.to_string(), f)).await;
        let väntat = if bokförd {
            vec![(2440, 50_000, 0), (2641, 0, 10_000), (5410, 0, 40_000)]
        } else {
            vec![(2440, 25_000, 0), (2641, 0, 5_000), (5410, 0, 20_000)]
        };
        assert_eq!(
            rader(&db, &anna, id, r.verifikat.unwrap().id).await,
            väntat,
            "{metod}"
        );
        // Tillgodohavandet återbetalas mot 2440 under båda.
        let post = reskontrapost(&db, &anna, id, Faktura::Leverantör(f)).await;
        assert_eq!(post.återstår, -25_000);
        let å = betala(
            &db,
            &anna,
            id,
            Faktura::Leverantör(f),
            &betalning("2026-05-10", "250"),
        )
        .await;
        assert_eq!(
            rader(&db, &anna, id, å.verifikat.id).await,
            [(1930, 25_000, 0), (2440, 0, 25_000)]
        );
    }
}

#[tokio::test]
async fn kredit_från_leverantören_av_en_obokförd_obetald_faktura_bokförs_inte() {
    let db = Databas::tillfällig().await;
    let (anna, id, f) = leverantörsfaktura(&db, "kontant", "").await;
    let l = leverantorsfaktura::fakturan(&db, &anna, id, f)
        .await
        .unwrap()
        .unwrap()
        .leverantör
        .unwrap();
    let (_, r) = registrera(&db, &anna, id, leverantörskredit(&l.to_string(), f)).await;
    assert_eq!(r.verifikat, None);
    assert_eq!(antal_verifikat(&db, &anna, id).await, 0);
    let post = reskontrapost(&db, &anna, id, Faktura::Leverantör(f)).await;
    assert_eq!(post.återstår, 75_000);
}

#[tokio::test]
async fn rätta_avskrift_under_kontantmetoden_låser_upp_utan_rättelse() {
    let db = Databas::tillfällig().await;
    let (anna, id, f) = leverantörsfaktura(&db, "kontant", "").await;
    let r = leverantorsfaktura::rätta_avskrift(&db, &klocka(), &anna, id, f, "fel konto")
        .await
        .unwrap();
    assert_eq!(r.rättelse, None);
    let formulär = leverantorsfaktura::fakturan(&db, &anna, id, f)
        .await
        .unwrap()
        .unwrap()
        .formulär;
    let om = leverantorsfaktura::registrera(&db, &klocka(), &anna, id, f, &formulär)
        .await
        .unwrap();
    assert_eq!(om.verifikat, None);
    assert_eq!(antal_verifikat(&db, &anna, id).await, 0);
}

// Metodbyte

async fn byt(db: &Databas, anna: &Aktör, id: FöretagsId, metod: &str) {
    foretag::byt_bokföringsmetod(
        db,
        &klocka(),
        anna,
        id,
        &maya_domain::foretag::Byte {
            gäller_från: "2027-01-01".into(),
            val: metod.into(),
        },
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn bokföringsmetoden_byts_inte_från_ett_år_som_redan_har_fakturor_utan_verifikat() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::fakturaföretag_med_metod(&db, "kontant").await;
    // Utfärdad under kontantmetoden utan verifikat: efter ett byte till fakturametoden skulle
    // fakturans fordran inte vara bokförd fast metoden på fakturadatumet säger att den är det.
    let k = testhjalp::kund(&db, &anna, id, "Kund AB").await;
    let utkast = testhjalp::kundfakturautkast(&db, &anna, id, k, "2027-01-20").await;
    let u = kundfaktura::utfärda(&db, &klocka(), &anna, id, utkast)
        .await
        .unwrap();
    assert_eq!(u.verifikat, None);
    let fel = foretag::byt_bokföringsmetod(
        &db,
        &klocka(),
        &anna,
        id,
        &maya_domain::foretag::Byte {
            gäller_från: "2027-01-01".into(),
            val: "faktura".into(),
        },
    )
    .await
    .unwrap_err();
    assert!(
        matches!(&fel, foretag::FöretagsFel::Ogiltigt(f) if f.iter().map(|f| f.kod).eq(["fakturor_efter"])),
        "{fel:?}"
    );
}

#[tokio::test]
async fn efter_byte_till_kontantmetoden_betalas_en_redan_bokförd_faktura_mot_1510() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::fakturaföretag_med_metod(&db, "faktura").await;
    let före = testhjalp::utfärdad_kundfaktura(&db, &anna, id, "Kund AB", "2026-12-10").await;
    assert!(före.verifikat.is_some());
    byt(&db, &anna, id, "kontant").await;
    // Fakturan från fakturametoden betalas under kontantmetoden mot 1510.
    let r = betala(
        &db,
        &anna,
        id,
        Faktura::Kund(före.id),
        &betalning("2027-01-15", "11 875"),
    )
    .await;
    assert_eq!(
        rader(&db, &anna, id, r.verifikat.id).await,
        [(1510, 0, TOTAL), (1930, TOTAL, 0)]
    );
    // En faktura daterad under kontantmetoden bokförs inte när den utfärdas, utan när den betalas.
    let efter = testhjalp::utfärdad_kundfaktura(&db, &anna, id, "Kund 2 AB", "2027-01-20").await;
    assert_eq!(efter.verifikat, None);
    let r = betala(
        &db,
        &anna,
        id,
        Faktura::Kund(efter.id),
        &betalning("2027-02-01", "11 875"),
    )
    .await;
    assert_eq!(
        rader(&db, &anna, id, r.verifikat.id).await,
        [(1930, TOTAL, 0), (2611, 0, 237_500), (3001, 0, 950_000)]
    );
}

#[tokio::test]
async fn efter_byte_till_fakturametoden_bokförs_bara_fakturor_daterade_efter_bytet_när_de_utfärdas()
{
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::fakturaföretag_med_metod(&db, "kontant").await;
    let före = testhjalp::utfärdad_kundfaktura(&db, &anna, id, "Kund AB", "2026-11-10").await;
    byt(&db, &anna, id, "faktura").await;
    assert_eq!(före.verifikat, None);
    // Den obokförda fakturan betalas före årets slut mot intäkt och moms.
    let r = betala(
        &db,
        &anna,
        id,
        Faktura::Kund(före.id),
        &betalning("2026-12-01", "11 875"),
    )
    .await;
    assert_eq!(
        rader(&db, &anna, id, r.verifikat.id).await,
        [(1930, TOTAL, 0), (2611, 0, 237_500), (3001, 0, 950_000)]
    );
    let efter = testhjalp::utfärdad_kundfaktura(&db, &anna, id, "Kund 2 AB", "2027-01-20").await;
    let v = efter
        .verifikat
        .expect("utfärdandet bokförs under fakturametoden");
    let utfärdandet = verifikat::verifikatet(&db, &anna, id, v.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        utfärdandet.datum.map(|d| d.to_string()).as_deref(),
        Some("2027-01-20")
    );
}

// Schemat

#[tokio::test]
async fn en_bokförd_fordran_ändras_och_raderas_aldrig_och_kräver_en_bokförd_faktura() {
    let db = Databas::tillfällig().await;
    let (_, _, _) = kundfaktura(&db, "faktura").await;
    for sql in [
        "UPDATE bokford_reskontrapost SET verifikat_id = verifikat_id",
        "DELETE FROM bokford_reskontrapost",
    ] {
        let fel = testhjalp::kör_sql(&db, sql).await.unwrap_err();
        assert!(
            fel.to_string().contains("går inte att ändra"),
            "{sql}: {fel}"
        );
    }
    // Ett utkast har ingen bokförd fordran, inte heller med ett bokfört verifikat.
    let fel = testhjalp::kör_sql(
        &db,
        "INSERT INTO kundfaktura (foretag_id, status, skapad, skapad_kanal)
         SELECT foretag_id, 'utkast', 'x', 'webb' FROM kundfaktura LIMIT 1",
    )
    .await
    .and(
        testhjalp::kör_sql(
            &db,
            "INSERT INTO bokford_reskontrapost (foretag_id, kundfaktura_id, verifikat_id)
             SELECT f.foretag_id, f.id, v.id FROM kundfaktura f, verifikat v
             WHERE f.status = 'utkast' LIMIT 1",
        )
        .await,
    )
    .unwrap_err();
    assert!(fel.to_string().contains("kräver en utfärdad"), "{fel}");
}
