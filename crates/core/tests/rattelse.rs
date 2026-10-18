//! Rättelse av ett bokfört verifikat och registrera om, genom `core` mot en tillfällig databas
//! (R1, R2, VF7).

use maya_core::aktor::Aktör;
use maya_core::foretag::FöretagsId;
use maya_core::verifikat::{self, Källa, Rättelseval, Urval, VerifikatFel, VerifikatId};
use maya_core::{Databas, Klocka, behandlingshistorik, testhjalp, underlag};
use maya_domain::verifikat::{Fält, NyRad, NyttVerifikat};

const NU: &str = "2026-10-18T08:00:00Z";

fn klocka() -> Klocka {
    Klocka::fast(NU.parse().unwrap())
}

fn rad(konto: &str, debet: &str, kredit: &str, text: &str) -> NyRad {
    NyRad {
        konto: konto.into(),
        debet: debet.into(),
        kredit: kredit.into(),
        text: text.into(),
    }
}

/// Ett verifikat med tre rader och radtext, bokfört som V2 efter ett annat.
fn mobilabonnemang() -> NyttVerifikat {
    NyttVerifikat {
        datum: "2026-10-02".into(),
        beskrivning: "Mobilabonnemang oktober".into(),
        rader: vec![
            rad("6540", "479,20", "", "Telia"),
            rad("2641", "119,80", "", ""),
            rad("1930", "", "599", "Kortet"),
        ],
    }
}

fn rader(v: &verifikat::Verifikat) -> Vec<(u16, i64, i64, Option<&str>)> {
    v.rader
        .iter()
        .map(|r| {
            (
                r.konto.unwrap().get(),
                r.debet.öre(),
                r.kredit.öre(),
                r.text.as_deref(),
            )
        })
        .collect()
}

#[tokio::test]
async fn rättelsen_vänder_varje_rad_och_bokförs_med_nästa_nummer() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    testhjalp::bokför(
        &db,
        &anna,
        id,
        "2026-03-14",
        &[(1930, 100, 0), (3001, 0, 100)],
    )
    .await;
    let original = verifikat::bokför(&db, &klocka(), &anna, id, &mobilabonnemang())
        .await
        .unwrap();

    let rättad = verifikat::rätta(
        &db,
        &klocka(),
        &anna,
        id,
        original.id,
        "2026-10-18",
        "",
        Rättelseval::BaraRättelse,
    )
    .await
    .unwrap();

    assert_eq!(rättad.rättelse.nummer.to_string(), "V3");
    assert_eq!(rättad.kopia, None);
    let r = verifikat::verifikatet(&db, &anna, id, rättad.rättelse.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        rader(&r),
        [
            (6540, 0, 47_920, Some("Telia")),
            (2641, 0, 11_980, None),
            (1930, 59_900, 0, Some("Kortet")),
        ]
    );
    assert_eq!(r.datum.unwrap().to_string(), "2026-10-18");
    assert_eq!(r.beskrivning, "Rättelse av V2");
    assert!(r.bokförd.is_some());
}

/// Bokför mobilabonnemanget och rättar det den 18 oktober med `orsak`.
async fn bokfört_och_rättat(
    db: &Databas,
    anna: &Aktör,
    id: FöretagsId,
    orsak: &str,
    val: Rättelseval,
) -> (verifikat::Bokfört, verifikat::Rättad) {
    let original = verifikat::bokför(db, &klocka(), anna, id, &mobilabonnemang())
        .await
        .unwrap();
    let rättad = verifikat::rätta(
        db,
        &klocka(),
        anna,
        id,
        original.id,
        "2026-10-18",
        orsak,
        val,
    )
    .await
    .unwrap();
    (original, rättad)
}

async fn läs(
    db: &Databas, anna: &Aktör, id: FöretagsId, v: VerifikatId
) -> verifikat::Verifikat {
    verifikat::verifikatet(db, anna, id, v)
        .await
        .unwrap()
        .unwrap()
}

#[tokio::test]
async fn beskrivningen_följer_mallen_med_och_utan_orsak() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    let (_, utan) = bokfört_och_rättat(&db, &anna, id, "", Rättelseval::BaraRättelse).await;
    let (_, med) =
        bokfört_och_rättat(&db, &anna, id, " fel konto ", Rättelseval::BaraRättelse).await;

    assert_eq!(
        läs(&db, &anna, id, utan.rättelse.id).await.beskrivning,
        "Rättelse av V1"
    );
    assert_eq!(
        läs(&db, &anna, id, med.rättelse.id).await.beskrivning,
        "Rättelse av V3, fel konto"
    );
}

#[tokio::test]
async fn ett_rättat_verifikat_och_en_rättelse_kan_inte_rättas_och_visar_varför() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    let (original, rättad) =
        bokfört_och_rättat(&db, &anna, id, "", Rättelseval::BaraRättelse).await;

    for (v, kod, text) in [
        (
            original.id,
            "redan_rattat",
            "Redan rättat av V2. Ett verifikat kan bara rättas en gång.",
        ),
        (
            rättad.rättelse.id,
            "ar_rattelse",
            "Det här är en rättelse av V1 och kan inte rättas. Blev något fel, registrera ett \
             nytt verifikat.",
        ),
    ] {
        let fel = verifikat::rätta(
            &db,
            &klocka(),
            &anna,
            id,
            v,
            "2026-10-18",
            "",
            Rättelseval::BaraRättelse,
        )
        .await
        .unwrap_err();
        let VerifikatFel::InteRättbart(spärr) = fel else {
            panic!("väntade InteRättbart, fick {fel:?}");
        };
        assert_eq!(spärr.kod(), kod);
        assert_eq!(spärr.to_string(), text);
        let läst = läs(&db, &anna, id, v).await;
        assert_eq!(läst.som_original().unwrap().kan_rättas(), Err(spärr));
    }
    assert_eq!(
        verifikat::lista(&db, &anna, id, Urval::Alla)
            .await
            .unwrap()
            .len(),
        2
    );
}

#[tokio::test]
async fn datum_före_originalets_nekas_och_inget_bokförs() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    let original = verifikat::bokför(&db, &klocka(), &anna, id, &mobilabonnemang())
        .await
        .unwrap();

    for val in [Rättelseval::BaraRättelse, Rättelseval::RegistreraOm] {
        let fel = verifikat::rätta(
            &db,
            &klocka(),
            &anna,
            id,
            original.id,
            "2026-10-01",
            "",
            val,
        )
        .await
        .unwrap_err();
        let VerifikatFel::Spärrat(spärrar) = fel else {
            panic!("väntade en spärr, fick {fel:?}");
        };
        assert_eq!(spärrar[0].kod, "fore_originalet");
        assert_eq!(spärrar[0].fält, Fält::Datum);
    }
    assert_eq!(
        verifikat::lista(&db, &anna, id, Urval::Alla)
            .await
            .unwrap()
            .len(),
        1
    );
    // Samma dag som originalet går.
    verifikat::rätta(
        &db,
        &klocka(),
        &anna,
        id,
        original.id,
        "2026-10-02",
        "",
        Rättelseval::BaraRättelse,
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn ett_utkast_kan_inte_rättas() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    let utkast = verifikat::spara_utkast(&db, &klocka(), &anna, id, None, &mobilabonnemang())
        .await
        .unwrap();
    let fel = verifikat::rätta(
        &db,
        &klocka(),
        &anna,
        id,
        utkast,
        "2026-10-18",
        "",
        Rättelseval::BaraRättelse,
    )
    .await
    .unwrap_err();
    assert!(matches!(fel, VerifikatFel::IngetVerifikat), "{fel:?}");
}

#[tokio::test]
async fn original_och_rättelse_länkar_till_varandra_med_vem_och_när_och_rättelsen_loggas() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    let (original, rättad) =
        bokfört_och_rättat(&db, &anna, id, "fel konto", Rättelseval::BaraRättelse).await;

    let o = läs(&db, &anna, id, original.id).await;
    let r = läs(&db, &anna, id, rättad.rättelse.id).await;
    assert_eq!(o.källa, Källa::Manuell);
    assert_eq!(r.källa, Källa::Rättelse);
    let till_rättelsen = o.rättad_av.clone().unwrap();
    assert_eq!(till_rättelsen.id, rättad.rättelse.id);
    assert_eq!(till_rättelsen.nummer.unwrap().to_string(), "V2");
    let bokförd = till_rättelsen.bokförd.unwrap();
    assert_eq!(bokförd.aktör, anna);
    assert_eq!(bokförd.tid, NU.parse().unwrap());
    assert_eq!(bokförd.namn, "anna@example.se");
    let till_originalet = r.rättar.clone().unwrap();
    assert_eq!(till_originalet.id, original.id);
    assert_eq!(till_originalet.nummer.unwrap().to_string(), "V1");
    assert_eq!(r.bokförd.as_ref().unwrap().aktör, anna);
    assert_eq!(o.rättar, None);
    assert_eq!(r.rättad_av, None);

    // Originalets historik visar rättelsen; behandlingshistoriken har en post.
    let typer: Vec<_> = o.historik.iter().map(|h| h.post.typ.as_str()).collect();
    assert_eq!(typer, ["verifikat bokfört", "verifikat rättat"]);
    let poster = behandlingshistorik::läs(&db, &anna, id).await.unwrap();
    let rättat = poster.iter().find(|p| p.typ == "verifikat rättat").unwrap();
    assert_eq!(rättat.objekt, "V1");
    assert_eq!(rättat.aktör, anna);
    assert_eq!(rättat.beskrivning["rättelse"]["nummer"], "V2");
    assert_eq!(rättat.beskrivning["orsak"], "fel konto");
    assert_eq!(rättat.beskrivning["datum"], "2026-10-18");
    let bokfört = poster
        .iter()
        .find(|p| p.typ == "verifikat bokfört" && p.objekt == "V2")
        .unwrap();
    assert_eq!(bokfört.beskrivning["rättar"]["nummer"], "V1");
}

#[tokio::test]
async fn registrera_om_ger_i_samma_transaktion_ett_utkast_med_originalets_rader_och_underlag() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    let kvitto = underlag::ladda_upp(
        &db,
        &klocka(),
        &anna,
        id,
        "telia.pdf",
        "application/pdf",
        b"%PDF-1.4 telia",
    )
    .await
    .unwrap();
    let original = verifikat::bokför_med_underlag(
        &db,
        &klocka(),
        &anna,
        id,
        None,
        &mobilabonnemang(),
        &[kvitto.id],
    )
    .await
    .unwrap();

    let rättad = verifikat::rätta(
        &db,
        &klocka(),
        &anna,
        id,
        original.id,
        "2026-10-18",
        "fel konto",
        Rättelseval::RegistreraOm,
    )
    .await
    .unwrap();

    let kopia = rättad.kopia.expect("ett utkast");
    let k = läs(&db, &anna, id, kopia).await;
    let o = läs(&db, &anna, id, original.id).await;
    assert_eq!(k.nummer, None);
    assert_eq!(k.källa, Källa::Manuell);
    let kopia_av = k.kopia_av.clone().unwrap();
    assert_eq!(kopia_av.id, original.id);
    assert_eq!(kopia_av.nummer.unwrap().to_string(), "V1");
    assert_eq!(rader(&k), rader(&o));
    assert_eq!(k.datum, o.datum);
    assert_eq!(k.beskrivning, o.beskrivning);
    let underlag: Vec<_> = k.underlag.iter().map(|u| (u.id, u.komplettering)).collect();
    assert_eq!(underlag, [(kvitto.id, false)]);
    assert_eq!(o.underlag.len(), 1);
    let poster = behandlingshistorik::läs(&db, &anna, id).await.unwrap();
    let rättat = poster.iter().find(|p| p.typ == "verifikat rättat").unwrap();
    assert_eq!(rättat.beskrivning["kopia"].to_string(), kopia.to_string());

    // Utkastet följer de vanliga reglerna och bokförs som nästa nummer.
    let bokförd = verifikat::bokför_utkast(&db, &klocka(), &anna, id, kopia, &mobilabonnemang())
        .await
        .unwrap();
    assert_eq!(bokförd.nummer.to_string(), "V3");
}

#[tokio::test]
async fn schemat_avvisar_en_andra_rättelse_en_rättelse_av_en_rättelse_och_ändrade_länkar() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    bokfört_och_rättat(&db, &anna, id, "", Rättelseval::RegistreraOm).await;

    let andra = testhjalp::kör_sql(
        &db,
        "INSERT INTO verifikat (foretag_id, beskrivning, kalla, skapad, skapad_kanal, rattar)
         SELECT foretag_id, 'x', 'rattelse', skapad, skapad_kanal, rattar
         FROM verifikat WHERE rattar IS NOT NULL",
    )
    .await;
    assert!(andra.unwrap_err().to_string().contains("UNIQUE"));
    let av_rättelsen = testhjalp::kör_sql(
        &db,
        "INSERT INTO verifikat (foretag_id, beskrivning, kalla, skapad, skapad_kanal, rattar)
         SELECT foretag_id, 'x', 'rattelse', skapad, skapad_kanal, id
         FROM verifikat WHERE rattar IS NOT NULL",
    )
    .await;
    assert!(
        av_rättelsen
            .unwrap_err()
            .to_string()
            .contains("en rättelse kan inte rättas")
    );
    let länk = testhjalp::kör_sql(
        &db,
        "UPDATE verifikat SET kopia_av = NULL WHERE kopia_av IS NOT NULL",
    )
    .await;
    assert!(
        länk
            .unwrap_err()
            .to_string()
            .contains("källa och länkar går inte att ändra")
    );
}

#[tokio::test]
async fn ett_verifikat_rättas_också_när_ett_av_dess_konton_har_inaktiverats() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    let original = testhjalp::bokför(
        &db,
        &anna,
        id,
        "2026-03-14",
        &[(1930, 100, 0), (3001, 0, 100)],
    )
    .await;
    // Saldot på 3001 nollas, och kontot inaktiveras.
    testhjalp::bokför(
        &db,
        &anna,
        id,
        "2026-03-15",
        &[(3001, 100, 0), (1930, 0, 100)],
    )
    .await;
    let konto = maya_domain::kontoplan::Kontonummer::new(3001).unwrap();
    maya_core::kontoplan::inaktivera(&db, &klocka(), &anna, id, konto)
        .await
        .unwrap();

    let rättad = verifikat::rätta(
        &db,
        &klocka(),
        &anna,
        id,
        original.id,
        "2026-10-18",
        "fel konto",
        Rättelseval::BaraRättelse,
    )
    .await
    .unwrap();

    assert_eq!(rättad.rättelse.nummer.to_string(), "V3");
}
