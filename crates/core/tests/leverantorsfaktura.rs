//! Leverantörsfakturor genom `core` (fakturametoden): vägen in med originalet kopplat,
//! registreringens verifikat i samma transaktion, spärrarna, nya leverantörer från utkastet och
//! triggrarna som låser en registrerad faktura.

use maya_core::aktor::Aktör;
use maya_core::atkomst::ÅtkomstFel;
use maya_core::foretag::{self, FöretagsId};
use maya_core::leverantor::{self, LeverantörsFel, Urval};
use maya_core::leverantorsfaktura::{self, FakturaFel, LeverantörsfakturaId, Status};
use maya_core::underlag::{self, UnderlagFel, UnderlagId};
use maya_core::verifikat::{self, Flöde, VerifikatFel};
use maya_core::{Databas, Klocka, behandlingshistorik, testhjalp};
use maya_domain::foretag::NyttFöretag;
use maya_domain::leverantor::{NyLeverantör, OmvändSkattskyldighet};
use maya_domain::leverantorsfaktura::{NY, NyLeverantörsfaktura, NyRad};
use maya_domain::rattelse::Rättelsespärr;

const NU: &str = "2026-10-19T08:00:00Z";

fn klocka() -> Klocka {
    Klocka::fast(NU.parse().unwrap())
}

async fn ladda_upp(db: &Databas, anna: &Aktör, id: FöretagsId) -> UnderlagId {
    underlag::ladda_upp(
        db,
        &klocka(),
        anna,
        id,
        "faktura-8812.pdf",
        "application/pdf",
        b"%PDF-1.4 faktura 8812",
    )
    .await
    .unwrap()
    .id
}

/// Ett företag, leverantören Kontorsvaror AB och ett utkast med ett uppladdat original.
async fn utkast() -> (
    maya_core::TillfälligDatabas,
    Aktör,
    FöretagsId,
    String,
    LeverantörsfakturaId,
) {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    let l = testhjalp::leverantör(&db, &anna, id, "Kontorsvaror AB").await;
    let (f, _) = leverantorsfaktura::ladda_upp(
        &db,
        &klocka(),
        &anna,
        id,
        (
            "faktura-8812.pdf",
            "application/pdf",
            b"%PDF-1.4 faktura 8812",
        ),
    )
    .await
    .unwrap();
    (db, anna, id, l.to_string(), f)
}

fn formulär(leverantör: &str) -> NyLeverantörsfaktura {
    NyLeverantörsfaktura {
        leverantör: leverantör.into(),
        fakturanummer: "8812".into(),
        fakturadatum: "2026-03-14".into(),
        förfallodag: "2026-04-13".into(),
        referens: "4455667788".into(),
        totalbelopp: "1 362,00".into(),
        originalvaluta: String::new(),
        rader: vec![
            NyRad {
                konto: String::new(),
                netto: "1 000".into(),
                momssats: "25".into(),
                moms: String::new(),
            },
            NyRad {
                konto: "6250".into(),
                netto: "100".into(),
                momssats: "12".into(),
                moms: String::new(),
            },
        ],
        ..Default::default()
    }
}

/// Raderna i verifikatet som `(konto, debet i öre, kredit i öre)`.
async fn rader(
    db: &Databas,
    anna: &Aktör,
    id: FöretagsId,
    v: verifikat::VerifikatId,
) -> Vec<(u16, i64, i64)> {
    verifikat::verifikatet(db, anna, id, v)
        .await
        .unwrap()
        .unwrap()
        .rader
        .iter()
        .map(|r| (r.konto.unwrap().get(), r.debet.öre(), r.kredit.öre()))
        .collect()
}

fn spärrkoder(fel: FakturaFel) -> Vec<&'static str> {
    match fel {
        FakturaFel::Spärrat(s) => s.iter().map(|s| s.kod).collect(),
        fel => panic!("väntade spärrar, fick {fel:?}"),
    }
}

#[tokio::test]
async fn vad_är_det_här_gör_ett_okopplat_underlag_till_ett_utkast_med_originalet() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    let u = ladda_upp(&db, &anna, id).await;

    let f = leverantorsfaktura::skapa_utkast(&db, &klocka(), &anna, id, u)
        .await
        .unwrap();

    let faktura = leverantorsfaktura::fakturan(&db, &anna, id, f)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(faktura.status, Status::Utkast);
    assert_eq!(faktura.underlag, u);
    assert_eq!(faktura.filnamn, "faktura-8812.pdf");
    // Underlaget är inte längre okopplat, och kan inte bli original till en faktura till.
    let okopplade = underlag::lista(&db, &anna, id, underlag::Urval::Okopplade)
        .await
        .unwrap();
    assert!(okopplade.is_empty(), "{okopplade:?}");
    assert!(matches!(
        leverantorsfaktura::skapa_utkast(&db, &klocka(), &anna, id, u).await,
        Err(FakturaFel::InteOkopplat)
    ));
    assert!(matches!(
        underlag::ta_bort(&db, &klocka(), &anna, id, u).await,
        Err(UnderlagFel::HörTillFaktura)
    ));
}

#[tokio::test]
async fn en_uppladdning_i_fliken_skapar_underlaget_och_utkastet_i_ett_steg() {
    let (db, anna, id, _, f) = utkast().await;
    let faktura = leverantorsfaktura::fakturan(&db, &anna, id, f)
        .await
        .unwrap()
        .unwrap();
    let u = underlag::underlaget(&db, &anna, id, faktura.underlag)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(u.leverantörsfaktura, Some(f));
    let lista = leverantorsfaktura::lista(&db, &anna, id).await.unwrap();
    assert_eq!(lista.len(), 1);
    assert_eq!(lista[0].status, Status::Utkast);
}

#[tokio::test]
async fn registreringen_bokför_kostnad_och_moms_mot_2440_på_fakturadatumet_med_originalet() {
    let (db, anna, id, l, f) = utkast().await;

    let r = leverantorsfaktura::registrera(&db, &klocka(), &anna, id, f, &formulär(&l))
        .await
        .unwrap();

    assert_eq!(r.verifikat.as_ref().unwrap().nummer.to_string(), "V1");
    assert_eq!(
        rader(&db, &anna, id, r.verifikat.as_ref().unwrap().id).await,
        [
            (5410, 100_000, 0),
            (6250, 10_000, 0),
            (2641, 26_200, 0),
            (2440, 0, 136_200)
        ]
    );
    let v = verifikat::verifikatet(&db, &anna, id, r.verifikat.as_ref().unwrap().id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(v.datum.unwrap().to_string(), "2026-03-14");
    assert_eq!(v.beskrivning, "Leverantörsfaktura 8812, Kontorsvaror AB");
    assert_eq!(v.underlag.len(), 1);
    assert_eq!(v.underlag[0].filnamn, "faktura-8812.pdf");
    assert!(!v.underlag[0].komplettering);
    // Verifikatet länkar till fakturan, och fakturan till verifikatet.
    let flöde = v.flöde.clone().unwrap();
    assert_eq!(flöde.flöde, Flöde::Leverantörsfaktura);
    assert_eq!(flöde.id.to_string(), f.to_string());
    assert_eq!(flöde.text, "leverantörsfaktura 8812 från Kontorsvaror AB");
    let faktura = leverantorsfaktura::fakturan(&db, &anna, id, f)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(faktura.status, Status::Registrerad);
    assert_eq!(
        faktura.verifikat.unwrap().0,
        r.verifikat.as_ref().unwrap().id
    );
    assert!(faktura.registrerad.is_some());
    // Den beräknade momsen står på raderna, och standardkontot på raden utan konto.
    assert_eq!(faktura.formulär.rader[0].konto, "5410");
    assert_eq!(faktura.formulär.rader[0].moms, "250,00");
    let typer: Vec<String> = faktura.historik.iter().map(|p| p.typ.clone()).collect();
    assert_eq!(typer, ["leverantörsfaktura registrerad"]);
    assert_eq!(
        faktura.historik[0].text(),
        "Faktura 8812 från ”Kontorsvaror AB” daterad 2026-03-14, förfaller 2026-04-13, bokförd \
         som V1"
    );
    // Verifikatets historik visar registreringen.
    assert!(
        v.historik
            .iter()
            .any(|h| h.post.typ == "leverantörsfaktura registrerad")
    );
}

#[tokio::test]
async fn ett_företag_utan_momsregistrering_bokför_momsen_på_kostnadskontot() {
    let db = Databas::tillfällig().await;
    let anna = testhjalp::användare(&db, "anna@example.se").await;
    let id = foretag::lägg_upp(
        &db,
        &klocka(),
        &anna,
        &NyttFöretag {
            orgnr: "556677-8899".into(),
            namn: "Liten AB".into(),
            första_år_start: "2026-01-01".into(),
            första_år_slut: "2026-12-31".into(),
            bokföringsmetod: "faktura".into(),
            momsperiod: "ej_momsregistrerad".into(),
        },
    )
    .await
    .unwrap();
    let l = testhjalp::leverantör(&db, &anna, id, "Kontorsvaror AB").await;
    let u = ladda_upp(&db, &anna, id).await;
    let f = leverantorsfaktura::skapa_utkast(&db, &klocka(), &anna, id, u)
        .await
        .unwrap();
    let r = leverantorsfaktura::registrera(&db, &klocka(), &anna, id, f, &formulär(&l.to_string()))
        .await
        .unwrap();
    assert_eq!(
        rader(&db, &anna, id, r.verifikat.as_ref().unwrap().id).await,
        [(5410, 125_000, 0), (6250, 11_200, 0), (2440, 0, 136_200)]
    );
}

#[tokio::test]
async fn under_kontantmetoden_registreras_fakturan_utan_verifikat() {
    let db = Databas::tillfällig().await;
    let anna = testhjalp::användare(&db, "anna@example.se").await;
    let id = foretag::lägg_upp(
        &db,
        &klocka(),
        &anna,
        &NyttFöretag {
            orgnr: "556677-8899".into(),
            namn: "Kontant AB".into(),
            första_år_start: "2026-01-01".into(),
            första_år_slut: "2026-12-31".into(),
            bokföringsmetod: "kontant".into(),
            momsperiod: "kvartal".into(),
        },
    )
    .await
    .unwrap();
    let l = testhjalp::leverantör(&db, &anna, id, "Kontorsvaror AB").await;
    let u = ladda_upp(&db, &anna, id).await;
    let f = leverantorsfaktura::skapa_utkast(&db, &klocka(), &anna, id, u)
        .await
        .unwrap();
    // #149: fakturan registreras, men skulden bokförs först vid betalningen.
    let r = leverantorsfaktura::registrera(&db, &klocka(), &anna, id, f, &formulär(&l.to_string()))
        .await
        .unwrap();
    assert_eq!(r.verifikat, None);
}

#[tokio::test]
async fn varje_spärr_stoppar_registreringen_och_inget_bokförs() {
    let (db, anna, id, l, f) = utkast().await;
    let fall: Vec<(NyLeverantörsfaktura, &str)> = vec![
        (
            NyLeverantörsfaktura {
                leverantör: String::new(),
                ..formulär(&l)
            },
            "leverantor_saknas konto_saknas",
        ),
        (
            NyLeverantörsfaktura {
                totalbelopp: "1 300".into(),
                ..formulär(&l)
            },
            "summerar_inte",
        ),
        (
            NyLeverantörsfaktura {
                fakturadatum: "2028-01-02".into(),
                förfallodag: "2028-02-01".into(),
                ..formulär(&l)
            },
            "saknat_rakenskapsar",
        ),
        (
            NyLeverantörsfaktura {
                fakturadatum: "2025-12-31".into(),
                ..formulär(&l)
            },
            "fore_forsta_aret",
        ),
    ];
    for (formulär, kod) in fall {
        let fel = leverantorsfaktura::registrera(&db, &klocka(), &anna, id, f, &formulär)
            .await
            .unwrap_err();
        assert_eq!(spärrkoder(fel).join(" "), kod);
    }
    // Ett inaktivt konto på en rad, med genvägen Aktivera i formuläret.
    maya_core::kontoplan::inaktivera(&db, &klocka(), &anna, id, "6250".parse().unwrap())
        .await
        .unwrap();
    let fel = leverantorsfaktura::registrera(&db, &klocka(), &anna, id, f, &formulär(&l))
        .await
        .unwrap_err();
    assert_eq!(spärrkoder(fel), ["konto_inaktivt"]);

    let lista = verifikat::lista(&db, &anna, id, verifikat::Urval::Alla)
        .await
        .unwrap();
    assert!(lista.is_empty(), "{lista:?}");
    let faktura = leverantorsfaktura::fakturan(&db, &anna, id, f)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(faktura.status, Status::Utkast);
}

#[tokio::test]
async fn samma_leverantör_och_nummer_spärrar_och_samma_belopp_och_datum_varnar() {
    let (db, anna, id, l, f) = utkast().await;
    leverantorsfaktura::registrera(&db, &klocka(), &anna, id, f, &formulär(&l))
        .await
        .unwrap();
    let u = ladda_upp(&db, &anna, id).await;
    let andra = leverantorsfaktura::skapa_utkast(&db, &klocka(), &anna, id, u)
        .await
        .unwrap();

    let fel = leverantorsfaktura::registrera(&db, &klocka(), &anna, id, andra, &formulär(&l))
        .await
        .unwrap_err();
    let FakturaFel::Spärrat(spärrar) = fel else {
        panic!("väntade spärrar");
    };
    assert_eq!(spärrar[0].kod, "dubblett");
    assert_eq!(
        spärrar[0].åtgärd,
        Some(maya_domain::leverantorsfaktura::Åtgärd::Fakturanummer(
            "8812-2".into()
        ))
    );

    let mut annat_nummer = formulär(&l);
    annat_nummer.fakturanummer = "8813".into();
    let förutsättningar = leverantorsfaktura::förutsättningar(&db, &anna, id, Some(andra))
        .await
        .unwrap();
    let varningar = annat_nummer.varningar(&förutsättningar);
    assert_eq!(varningar.len(), 1);
    assert_eq!(varningar[0].kod, "trolig_dubblett");
    // Varningen spärrar inte.
    leverantorsfaktura::registrera(&db, &klocka(), &anna, id, andra, &annat_nummer)
        .await
        .unwrap();
}

#[tokio::test]
async fn en_ny_leverantör_från_utkastet_blir_verklig_vid_registreringen() {
    let (db, anna, id, _, f) = utkast().await;
    let mut ny = formulär(NY);
    ny.ny_leverantör = NyLeverantör {
        namn: "Tryckeriet AB".into(),
        bankgiro: "5050-1055".into(),
        standardkonto: "5410".into(),
        ..Default::default()
    };
    leverantorsfaktura::spara_utkast(&db, &klocka(), &anna, id, f, &ny)
        .await
        .unwrap();
    // Den nya leverantören finns inte i registret än.
    let namn = |l: Vec<leverantor::Leverantör>| {
        l.into_iter().map(|l| l.uppgifter.namn).collect::<Vec<_>>()
    };
    assert_eq!(
        namn(
            leverantor::lista(&db, &anna, id, Urval::Alla)
                .await
                .unwrap()
        ),
        ["Kontorsvaror AB"]
    );
    let sparat = leverantorsfaktura::fakturan(&db, &anna, id, f)
        .await
        .unwrap()
        .unwrap();
    assert!(sparat.ny_leverantör);
    assert_eq!(sparat.formulär.leverantör, NY);
    assert_eq!(sparat.formulär.ny_leverantör.namn, "Tryckeriet AB");

    let r = leverantorsfaktura::registrera(&db, &klocka(), &anna, id, f, &sparat.formulär)
        .await
        .unwrap();
    assert_eq!(
        rader(&db, &anna, id, r.verifikat.as_ref().unwrap().id).await[0],
        (5410, 100_000, 0)
    );
    assert_eq!(
        namn(
            leverantor::lista(&db, &anna, id, Urval::Aktiva)
                .await
                .unwrap()
        ),
        ["Kontorsvaror AB", "Tryckeriet AB"]
    );
    let poster = behandlingshistorik::läs(&db, &anna, id).await.unwrap();
    let skapad = poster
        .iter()
        .find(|p| p.typ == "leverantör skapad" && p.objekt == "Tryckeriet AB")
        .unwrap();
    assert_eq!(skapad.aktör, anna);
}

#[tokio::test]
async fn en_ny_leverantör_raderas_med_utkastet_loggat_med_maya_som_aktör() {
    let (db, anna, id, l, f) = utkast().await;
    let mut ny = formulär(NY);
    ny.ny_leverantör.namn = "Tryckeriet AB".into();
    leverantorsfaktura::spara_utkast(&db, &klocka(), &anna, id, f, &ny)
        .await
        .unwrap();
    // Ett utkast som byter till en leverantör i registret släpper den nya.
    leverantorsfaktura::spara_utkast(&db, &klocka(), &anna, id, f, &formulär(&l))
        .await
        .unwrap();
    leverantorsfaktura::spara_utkast(&db, &klocka(), &anna, id, f, &ny)
        .await
        .unwrap();
    leverantorsfaktura::radera_utkast(&db, &klocka(), &anna, id, f)
        .await
        .unwrap();

    let poster = behandlingshistorik::läs(&db, &anna, id).await.unwrap();
    let raderade: Vec<_> = poster
        .iter()
        .filter(|p| p.typ == "leverantör raderad")
        .collect();
    assert_eq!(raderade.len(), 2);
    assert!(raderade.iter().all(|p| p.aktör == Aktör::maya()));
    assert!(raderade.iter().all(|p| p.objekt == "Tryckeriet AB"));
    let radering = poster
        .iter()
        .find(|p| p.typ == "fakturautkast raderat")
        .unwrap();
    assert_eq!(radering.aktör, anna);
    assert_eq!(
        radering.text(),
        "Utkast till leverantörsfaktura, nummer ”8812”, från ”Tryckeriet AB”, 2 rader"
    );
    // Utkastet är borta och originalet är okopplat igen.
    assert!(
        leverantorsfaktura::fakturan(&db, &anna, id, f)
            .await
            .unwrap()
            .is_none()
    );
    let okopplade = underlag::lista(&db, &anna, id, underlag::Urval::Okopplade)
        .await
        .unwrap();
    assert_eq!(okopplade.len(), 1);
}

#[tokio::test]
async fn triggrarna_avvisar_ändring_och_radering_av_en_registrerad_faktura() {
    let (db, anna, id, l, f) = utkast().await;
    leverantorsfaktura::registrera(&db, &klocka(), &anna, id, f, &formulär(&l))
        .await
        .unwrap();
    for sql in [
        "UPDATE leverantorsfaktura SET total = 1",
        "UPDATE leverantorsfaktura SET status = 'utkast'",
        "DELETE FROM leverantorsfaktura",
        "UPDATE leverantorsfakturarad SET netto = 1",
        "DELETE FROM leverantorsfakturarad",
        "INSERT INTO leverantorsfakturarad (faktura_id, radnr, momssats)
         SELECT id, 9, 25 FROM leverantorsfaktura",
    ] {
        let fel = testhjalp::kör_sql(&db, sql).await.unwrap_err().to_string();
        assert!(
            fel.contains("registrerad leverantörsfaktura går inte att"),
            "{sql}: {fel}"
        );
    }
}

#[tokio::test]
async fn ett_verifikat_från_en_leverantörsfaktura_rättas_inte_som_ett_manuellt() {
    let (db, anna, id, l, f) = utkast().await;
    let r = leverantorsfaktura::registrera(&db, &klocka(), &anna, id, f, &formulär(&l))
        .await
        .unwrap();
    let fel = verifikat::rätta(
        &db,
        &klocka(),
        &anna,
        id,
        r.verifikat.as_ref().unwrap().id,
        "2026-10-19",
        "",
        verifikat::Rättelseval::BaraRättelse,
    )
    .await
    .unwrap_err();
    assert!(
        matches!(fel, VerifikatFel::InteRättbart(Rättelsespärr::FrånFlöde(_))),
        "{fel:?}"
    );
}

#[tokio::test]
async fn en_leverantör_med_fakturor_kan_inte_raderas() {
    let (db, anna, id, l, _) = utkast().await;
    let l: maya_core::leverantor::LeverantörsId = l.parse().unwrap();
    let mut f = formulär(&l.to_string());
    f.fakturanummer = "1".into();
    let u = ladda_upp(&db, &anna, id).await;
    let utkast = leverantorsfaktura::skapa_utkast(&db, &klocka(), &anna, id, u)
        .await
        .unwrap();
    leverantorsfaktura::registrera(&db, &klocka(), &anna, id, utkast, &f)
        .await
        .unwrap();
    let lev = leverantor::leverantören(&db, &anna, id, l)
        .await
        .unwrap()
        .unwrap();
    assert!(!lev.kan_raderas);
    assert!(matches!(
        leverantor::radera(&db, &klocka(), &anna, id, l).await,
        Err(LeverantörsFel::HarFakturor)
    ));
}

#[tokio::test]
async fn den_som_saknar_åtkomst_får_inte_röra_leverantörsfakturorna() {
    let (db, _, id, l, f) = utkast().await;
    let bo = testhjalp::användare(&db, "bo@example.se").await;
    assert!(matches!(
        leverantorsfaktura::registrera(&db, &klocka(), &bo, id, f, &formulär(&l)).await,
        Err(FakturaFel::Åtkomst(ÅtkomstFel::IngenÅtkomst))
    ));
    assert!(matches!(
        leverantorsfaktura::radera_utkast(&db, &klocka(), &bo, id, f).await,
        Err(FakturaFel::Åtkomst(ÅtkomstFel::IngenÅtkomst))
    ));
    assert!(matches!(
        leverantorsfaktura::fakturan(&db, &bo, id, f).await,
        Err(ÅtkomstFel::IngenÅtkomst)
    ));
}

// #146 omvänd skattskyldighet

/// Fakturan från [`formulär`] med omvänd skattskyldighet `kod`: totalen är nettot.
fn omvänt_formulär(leverantör: &str, kod: &str) -> NyLeverantörsfaktura {
    NyLeverantörsfaktura {
        omvänd_skattskyldighet: kod.into(),
        totalbelopp: "1 100,00".into(),
        ..formulär(leverantör)
    }
}

#[tokio::test]
async fn varje_variant_av_omvänd_skattskyldighet_bokför_fiktiv_moms_enligt_tabellen() {
    for (kod, ingående) in [
        ("eu_tjanst", 2645),
        ("eu_vara", 2645),
        ("utanfor_eu", 2645),
        ("bygg", 2647),
    ] {
        let (db, anna, id, l, f) = utkast().await;
        let r =
            leverantorsfaktura::registrera(&db, &klocka(), &anna, id, f, &omvänt_formulär(&l, kod))
                .await
                .unwrap_or_else(|e| panic!("{kod}: {e:?}"));
        assert_eq!(
            rader(&db, &anna, id, r.verifikat.as_ref().unwrap().id).await,
            [
                (5410, 100_000, 0),
                (6250, 10_000, 0),
                (ingående, 26_200, 0),
                (2440, 0, 110_000),
                (2614, 0, 25_000),
                (2624, 0, 1_200),
            ],
            "{kod}"
        );
        let faktura = leverantorsfaktura::fakturan(&db, &anna, id, f)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(faktura.formulär.omvänd_skattskyldighet, kod);
        assert_eq!(faktura.formulär.rader[0].moms, "250,00");
        let post = faktura.historik.last().unwrap();
        assert_eq!(post.beskrivning["omvänd_skattskyldighet"], kod);
    }
}

#[tokio::test]
async fn leverantörens_förval_föreslås_på_utkastet_och_bokförs() {
    let (db, anna, id, _, f) = utkast().await;
    let l = leverantor::skapa(
        &db,
        &klocka(),
        &anna,
        id,
        &NyLeverantör {
            namn: "Byggfirman AB".into(),
            bankgiro: "5050-1055".into(),
            standardkonto: "5410".into(),
            omvänd_skattskyldighet: "bygg".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .to_string();
    let mut utkastet = omvänt_formulär(&l, "");
    leverantorsfaktura::spara_utkast(&db, &klocka(), &anna, id, f, &utkastet)
        .await
        .unwrap();

    // Utkastet har inget eget val, så leverantörens förval föreslås.
    let sparat = leverantorsfaktura::fakturan(&db, &anna, id, f)
        .await
        .unwrap()
        .unwrap()
        .formulär;
    assert_eq!(sparat.omvänd_skattskyldighet, "");
    let fö = leverantorsfaktura::förutsättningar(&db, &anna, id, Some(f))
        .await
        .unwrap();
    assert_eq!(
        sparat.omvänd_skattskyldighet(&fö),
        OmvändSkattskyldighet::Bygg
    );

    // Ett val på utkastet sparas och går före förvalet.
    utkastet.omvänd_skattskyldighet = "ingen".into();
    leverantorsfaktura::spara_utkast(&db, &klocka(), &anna, id, f, &utkastet)
        .await
        .unwrap();
    let sparat = leverantorsfaktura::fakturan(&db, &anna, id, f)
        .await
        .unwrap()
        .unwrap()
        .formulär;
    assert_eq!(sparat.omvänd_skattskyldighet, "ingen");

    // Registreras utkastet utan eget val bokförs förvalet.
    utkastet.omvänd_skattskyldighet = String::new();
    let r = leverantorsfaktura::registrera(&db, &klocka(), &anna, id, f, &utkastet)
        .await
        .unwrap();
    assert_eq!(
        rader(&db, &anna, id, r.verifikat.as_ref().unwrap().id).await[2],
        (2647, 26_200, 0)
    );
    let registrerad = leverantorsfaktura::fakturan(&db, &anna, id, f)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(registrerad.formulär.omvänd_skattskyldighet, "bygg");
}

#[tokio::test]
async fn ett_företag_utan_momsregistrering_kan_inte_registrera_omvänd_skattskyldighet() {
    let db = Databas::tillfällig().await;
    let anna = testhjalp::användare(&db, "anna@example.se").await;
    let id = foretag::lägg_upp(
        &db,
        &klocka(),
        &anna,
        &NyttFöretag {
            orgnr: "556677-8899".into(),
            namn: "Liten AB".into(),
            första_år_start: "2026-01-01".into(),
            första_år_slut: "2026-12-31".into(),
            bokföringsmetod: "faktura".into(),
            momsperiod: "ej_momsregistrerad".into(),
        },
    )
    .await
    .unwrap();
    let l = testhjalp::leverantör(&db, &anna, id, "Kontorsvaror AB").await;
    let u = ladda_upp(&db, &anna, id).await;
    let f = leverantorsfaktura::skapa_utkast(&db, &klocka(), &anna, id, u)
        .await
        .unwrap();
    let fel = leverantorsfaktura::registrera(
        &db,
        &klocka(),
        &anna,
        id,
        f,
        &omvänt_formulär(&l.to_string(), "eu_tjanst"),
    )
    .await
    .unwrap_err();
    assert_eq!(spärrkoder(fel)[0], "omvand_ej_momsregistrerad");
}

#[tokio::test]
async fn en_okänd_kod_för_omvänd_skattskyldighet_sparas_inte() {
    let (db, anna, id, l, f) = utkast().await;
    let fel = leverantorsfaktura::spara_utkast(
        &db,
        &klocka(),
        &anna,
        id,
        f,
        &omvänt_formulär(&l, "import"),
    )
    .await
    .unwrap_err();
    assert_eq!(spärrkoder(fel), ["omvand_ogiltig"]);
    let svar = testhjalp::kör_sql(
        &db,
        "UPDATE leverantorsfaktura SET omvand_skattskyldighet = 'import'",
    )
    .await;
    assert!(svar.is_err(), "schemat avvisar en okänd kod");
}
