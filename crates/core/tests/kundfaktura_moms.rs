//! Omvänd skattskyldighet och företag utan momsregistrering på kundfakturor genom `core` mot en
//! tillfällig databas (#136): intäktskontot, ingen momsrad, spärren för köparens momsreg-nr,
//! texten på PDF:en och kundens förval.

use maya_core::aktor::Aktör;
use maya_core::foretag::{self, FöretagsId};
use maya_core::kundfaktura::{self, KundfakturaFel, KundfakturaId, Utfärdad};
use maya_core::{Databas, Klocka, kund, testhjalp, underlag, verifikat};
use maya_domain::foretag::Byte;
use maya_domain::kund::{NyKund, OmvändSkattskyldighet};
use maya_domain::kundfaktura::{Momsläge, NyFakturarad, NyKundfaktura};
use maya_pdf::testhjalp::{innehåller, sidor};

fn klocka() -> Klocka {
    Klocka::fast("2026-10-18T08:00:00Z".parse().unwrap())
}

fn rad(beskrivning: &str, antal: &str, à_pris: &str, sats: &str, konto: &str) -> NyFakturarad {
    NyFakturarad {
        beskrivning: beskrivning.into(),
        antal: antal.into(),
        enhet: "st".into(),
        à_pris: à_pris.into(),
        rabatt: String::new(),
        momssats: sats.into(),
        konto: konto.into(),
    }
}

/// En kund med adress, momsreg-nr `momsregnr` och förvald omvänd skattskyldighet `omvänd`.
async fn kund(
    db: &Databas,
    anna: &Aktör,
    id: FöretagsId,
    momsregnr: &str,
    omvänd: OmvändSkattskyldighet,
) -> kund::KundId {
    kund::skapa(
        db,
        &klocka(),
        anna,
        id,
        &NyKund {
            namn: "Bergström Bygg AB".into(),
            adress: "Industrivägen 3\n721 30 Västerås".into(),
            momsregnr: momsregnr.into(),
            betalningsvillkor: "30".into(),
            omvänd_skattskyldighet: omvänd.kod().into(),
            ..Default::default()
        },
    )
    .await
    .unwrap()
}

fn faktura(kund: kund::KundId, omvänd: &str, konto: &str) -> NyKundfaktura {
    NyKundfaktura {
        kund: kund.to_string(),
        fakturadatum: "2026-03-10".into(),
        omvänd_skattskyldighet: omvänd.into(),
        rader: vec![
            rad("Elinstallation etapp 2", "1", "62000", "0", konto),
            rad("Material", "2", "500", "0", konto),
        ],
        ..Default::default()
    }
}

async fn utkast(
    db: &Databas,
    anna: &Aktör,
    id: FöretagsId,
    formulär: &NyKundfaktura,
) -> KundfakturaId {
    kundfaktura::spara_utkast(db, &klocka(), anna, id, None, formulär)
        .await
        .unwrap()
}

async fn utfärda(db: &Databas, anna: &Aktör, id: FöretagsId, f: KundfakturaId) -> Utfärdad {
    kundfaktura::utfärda(db, &klocka(), anna, id, f)
        .await
        .unwrap()
}

async fn kontering(
    db: &Databas,
    anna: &Aktör,
    id: FöretagsId,
    u: &Utfärdad,
) -> Vec<(u16, i64, i64)> {
    verifikat::verifikatet(db, anna, id, u.verifikat.as_ref().unwrap().id)
        .await
        .unwrap()
        .unwrap()
        .rader
        .iter()
        .map(|r| (r.konto.unwrap().get(), r.debet.öre(), r.kredit.öre()))
        .collect()
}

async fn pdf_text(db: &Databas, anna: &Aktör, id: FöretagsId, u: &Utfärdad) -> String {
    let fil = underlag::filen(db, anna, id, u.underlag)
        .await
        .unwrap()
        .unwrap();
    sidor(&fil.innehåll).join("")
}

#[tokio::test]
async fn omvänd_skattskyldighet_för_bygg_bokförs_på_3231_utan_momsrad_och_pdf_en_har_texten() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::fakturaföretag(&db).await;
    let k = kund(
        &db,
        &anna,
        id,
        "SE559100445501",
        OmvändSkattskyldighet::Bygg,
    )
    .await;
    let f = utkast(&db, &anna, id, &faktura(k, "bygg", "3231")).await;

    let utkastet = kundfaktura::kundfakturan(&db, &anna, id, f).await.unwrap();
    assert_eq!(utkastet.formulär.omvänd_skattskyldighet, "bygg");

    let u = utfärda(&db, &anna, id, f).await;
    assert_eq!(
        kontering(&db, &anna, id, &u).await,
        [(1510, 6_300_000, 0), (3231, 0, 6_300_000)]
    );
    let utfärdning = kundfaktura::kundfakturan(&db, &anna, id, f)
        .await
        .unwrap()
        .utfärdad
        .unwrap();
    assert_eq!(utfärdning.momsläge, Momsläge::Bygg);
    assert_eq!(utfärdning.moms, 0);
    assert_eq!(utfärdning.total, 6_300_000);

    let text = pdf_text(&db, &anna, id, &u).await;
    for uppgift in [
        "Omvänd betalningsskyldighet",
        "Momsreg.nr SE559100445501",
        "Momsreg.nr SE556036079301",
        "Att betala 63 000,00 kr",
    ] {
        assert!(innehåller(&text, uppgift), "{uppgift} saknas i {text}");
    }
}

#[tokio::test]
async fn omvänd_skattskyldighet_för_eu_tjänst_bokförs_på_3308_utan_momsrad() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::fakturaföretag(&db).await;
    let k = kund(
        &db,
        &anna,
        id,
        "DE811234567",
        OmvändSkattskyldighet::EuTjänst,
    )
    .await;
    // En sats som står kvar på en rad gäller inte: raden får 0 %.
    let mut formulär = faktura(k, "eu_tjanst", "3308");
    formulär.rader[1].momssats = "25".into();
    let f = utkast(&db, &anna, id, &formulär).await;

    let u = utfärda(&db, &anna, id, f).await;
    assert_eq!(
        kontering(&db, &anna, id, &u).await,
        [(1510, 6_300_000, 0), (3308, 0, 6_300_000)]
    );
    let text = pdf_text(&db, &anna, id, &u).await;
    assert!(innehåller(&text, "Reverse charge"), "{text}");
    assert!(innehåller(&text, "Momsreg.nr DE811234567"), "{text}");
}

#[tokio::test]
async fn utan_köparens_momsregistreringsnummer_spärras_utfärdandet_vid_omvänd_skattskyldighet() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::fakturaföretag(&db).await;
    let k = kund(&db, &anna, id, "", OmvändSkattskyldighet::Bygg).await;
    let f = utkast(&db, &anna, id, &faktura(k, "bygg", "3231")).await;

    let Err(KundfakturaFel::Spärrat(spärrar)) =
        kundfaktura::utfärda(&db, &klocka(), &anna, id, f).await
    else {
        panic!("utfärdades utan köparens momsreg-nr");
    };
    let koder: Vec<_> = spärrar.iter().map(|s| (s.fält, s.kod)).collect();
    assert_eq!(
        koder,
        [(
            maya_domain::kundfaktura::Fält::Kund,
            "kund_momsregnr_saknas"
        )]
    );
    assert!(
        verifikat::lista(&db, &anna, id, verifikat::Urval::Bokförda)
            .await
            .unwrap()
            .is_empty()
    );
    // Utan omvänd skattskyldighet går samma kund att fakturera.
    let mut utan = faktura(k, "ingen", "3001");
    utan.rader.truncate(1);
    utan.rader[0].momssats = "25".into();
    kundfaktura::spara_utkast(&db, &klocka(), &anna, id, Some(f), &utan)
        .await
        .unwrap();
    utfärda(&db, &anna, id, f).await;
}

#[tokio::test]
async fn ett_företag_utan_momsregistrering_får_fakturor_utan_moms_och_utan_momsregistreringsnummer()
{
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::fakturaföretag(&db).await;
    foretag::byt_momsperiod(
        &db,
        &klocka(),
        &anna,
        id,
        &Byte {
            gäller_från: "2026-04-01".into(),
            val: "ej_momsregistrerad".into(),
        },
    )
    .await
    .unwrap();
    let k = kund(
        &db,
        &anna,
        id,
        "SE559100445501",
        OmvändSkattskyldighet::Bygg,
    )
    .await;
    // Momssatsen och den omvända skattskyldigheten i formuläret gäller inte.
    let mut formulär = faktura(k, "bygg", "3004");
    formulär.fakturadatum = "2026-05-10".into();
    formulär.rader[0].momssats = "25".into();
    let f = utkast(&db, &anna, id, &formulär).await;
    // Utkastet sparas med 0 % och listan räknar ingen moms.
    let lista = kundfaktura::lista(&db, &anna, id).await.unwrap();
    assert_eq!(lista[0].total, 6_300_000);

    let u = utfärda(&db, &anna, id, f).await;
    assert_eq!(
        kontering(&db, &anna, id, &u).await,
        [(1510, 6_300_000, 0), (3004, 0, 6_300_000)]
    );
    let utfärdning = kundfaktura::kundfakturan(&db, &anna, id, f)
        .await
        .unwrap()
        .utfärdad
        .unwrap();
    assert_eq!(utfärdning.momsläge, Momsläge::UtanMoms);
    assert_eq!(utfärdning.säljare.momsregnr, "");
    let text = pdf_text(&db, &anna, id, &u).await;
    for saknas in ["Momsreg", "Moms 0 %", "Omvänd betalningsskyldighet"] {
        assert!(!innehåller(&text, saknas), "{saknas} finns i {text}");
    }
    assert!(innehåller(&text, "Att betala 63 000,00 kr"), "{text}");

    // Före bytet var företaget momsregistrerat, och då gäller satserna.
    let mut före = faktura(k, "ingen", "3001");
    före.rader[0].momssats = "25".into();
    före.rader.truncate(1);
    let f = utkast(&db, &anna, id, &före).await;
    let u = utfärda(&db, &anna, id, f).await;
    assert_eq!(
        kontering(&db, &anna, id, &u).await,
        [
            (1510, 7_750_000, 0),
            (3001, 0, 6_200_000),
            (2611, 0, 1_550_000)
        ]
    );
}

#[tokio::test]
async fn kundens_förvalda_omvända_skattskyldighet_fylls_i_på_utkastet() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::fakturaföretag(&db).await;
    let k = kund(
        &db,
        &anna,
        id,
        "SE559100445501",
        OmvändSkattskyldighet::Bygg,
    )
    .await;
    let förutsättningar = kundfaktura::förutsättningar(&db, &anna, id).await.unwrap();
    assert_eq!(
        förutsättningar.kunder[0].omvänd_skattskyldighet,
        OmvändSkattskyldighet::Bygg
    );

    let mut formulär = NyKundfaktura {
        fakturadatum: "2026-03-10".into(),
        rader: vec![rad("Elinstallation", "1", "62000", "25", "3001")],
        ..Default::default()
    };
    formulär.välj_kund(&k.to_string(), &förutsättningar);
    let f = utkast(&db, &anna, id, &formulär).await;
    let utkastet = kundfaktura::kundfakturan(&db, &anna, id, f).await.unwrap();
    assert_eq!(utkastet.formulär.omvänd_skattskyldighet, "bygg");
    assert_eq!(utkastet.formulär.rader[0].momssats, "0");
    assert_eq!(utkastet.formulär.rader[0].konto, "3231");
}
