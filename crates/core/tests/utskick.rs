//! Utskick av kundfakturor genom `core` mot en tillfällig databas och en brevlåda i testet
//! (#138): mejlet med den arkiverade PDF:en och svarsadressen, att ett avvisat utskick loggas
//! utan att utfärdandet rullas tillbaka, lämnad på annat sätt och skyddet mot ändring.
//! Själva SMTP-samtalet testas genom webbens adapter i `crates/web/tests/utskick.rs`.

use maya_core::kundfaktura::{self, Status};
use maya_core::testhjalp::{self, Brevlåda};
use maya_core::utskick::{self, Sätt, Utfall, UtskickFel};
use maya_core::{Databas, Klocka, fakturauppgifter, kund, underlag};
use maya_domain::fakturauppgifter::Fakturauppgifter;
use maya_domain::kund::NyKund;
use maya_domain::utskick::Lämningssätt;

const NU: &str = "2026-03-12T09:30:00Z";

fn klocka() -> Klocka {
    Klocka::fast(NU.parse().unwrap())
}

/// Ett fakturaföretag med svarsadress och standardmeddelande.
async fn företag(db: &Databas) -> (maya_core::aktor::Aktör, maya_core::foretag::FöretagsId) {
    let (anna, id) = testhjalp::fakturaföretag(db).await;
    fakturauppgifter::ändra(
        db,
        &klocka(),
        &anna,
        id,
        &Fakturauppgifter {
            bankgiro: "5050-1055".into(),
            svarsadress: "faktura@exempel.se".into(),
            standardmeddelande: "Hej! Här är fakturan.".into(),
            ..Fakturauppgifter::default()
        },
    )
    .await
    .unwrap();
    (anna, id)
}

#[tokio::test]
async fn utskicket_har_mottagaren_svarsadressen_och_den_arkiverade_pdfen_och_loggas() {
    let db = Databas::tillfällig().await;
    let (anna, id) = företag(&db).await;
    let f = testhjalp::utfärdad_kundfaktura(&db, &anna, id, "Kund AB", "2026-03-10").await;
    let låda = Brevlåda::ny();

    let förslag = utskick::förslag(&db, &anna, id, f.id).await.unwrap();
    assert_eq!(förslag.till, "ekonomi@kund.se");
    assert_eq!(förslag.svarsadress, "faktura@exempel.se");
    assert_eq!(förslag.meddelande, "Hej! Här är fakturan.");
    assert_eq!(förslag.bilaga, format!("Kundfaktura_{}.pdf", f.nummer));

    let utfall = utskick::skicka(
        &db,
        &klocka(),
        &anna,
        id,
        f.id,
        &utskick::NyttUtskick {
            till: " Ekonomi@Kund.se ".into(),
            meddelande: "Hej! Här är fakturan, med rabatten.".into(),
        },
        &låda,
    )
    .await
    .unwrap();
    assert_eq!(utfall, Utfall::Skickat);

    let mejl = låda.mejl();
    assert_eq!(mejl.len(), 1);
    let m = &mejl[0];
    assert_eq!(m.till, "ekonomi@kund.se");
    assert_eq!(m.svara_till.as_deref(), Some("faktura@exempel.se"));
    assert_eq!(m.avsändarnamn, "Exempel AB");
    assert_eq!(m.ämne, format!("Faktura {} från Exempel AB", f.nummer));
    assert_eq!(m.text, "Hej! Här är fakturan, med rabatten.");
    let arkiverad = underlag::filen(&db, &anna, id, f.underlag)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(m.bilaga.filnamn, arkiverad.filnamn);
    assert_eq!(m.bilaga.mime, "application/pdf");
    assert_eq!(m.bilaga.innehåll, arkiverad.innehåll);

    let utskicken = utskick::utskicken(&db, &anna, id, f.id).await.unwrap();
    assert_eq!(utskicken.len(), 1);
    let u = &utskicken[0];
    assert_eq!(u.signatur.tid, NU.parse().unwrap());
    assert_eq!(u.signatur.namn, "anna@example.se");
    assert_eq!(
        u.sätt,
        Sätt::Epost {
            till: "ekonomi@kund.se".into(),
            svarsadress: Some("faktura@exempel.se".into()),
        }
    );
    assert_eq!(u.fel, None);

    let historik = kundfaktura::kundfakturan(&db, &anna, id, f.id)
        .await
        .unwrap()
        .historik;
    let post = historik.last().unwrap();
    assert_eq!(post.post.typ, "utskick gjort");
    assert_eq!(
        post.post.text(),
        format!("Faktura {} skickad till ekonomi@kund.se", f.nummer)
    );
}

#[tokio::test]
async fn ett_avvisat_utskick_loggas_som_misslyckat_utan_att_utfärdandet_rullas_tillbaka() {
    let db = Databas::tillfällig().await;
    let (anna, id) = företag(&db).await;
    let k = testhjalp::kund(&db, &anna, id, "Kund AB").await;
    let utkast = testhjalp::kundfakturautkast(&db, &anna, id, k, "2026-03-10").await;
    let låda = Brevlåda::som_avvisar("554 Avvisat");

    let (utfärdad, utfall) = utskick::utfärda_och_skicka(&db, &klocka(), &anna, id, utkast, &låda)
        .await
        .unwrap();
    assert_eq!(utfall, Some(Utfall::Misslyckat("554 Avvisat".into())));

    let faktura = kundfaktura::kundfakturan(&db, &anna, id, utfärdad.id)
        .await
        .unwrap();
    assert_eq!(faktura.status, Status::Utfärdad);
    let typer: Vec<&str> = faktura
        .historik
        .iter()
        .map(|h| h.post.typ.as_str())
        .collect();
    assert_eq!(typer, ["kundfaktura utfärdad", "utskick misslyckat"]);
    assert_eq!(
        faktura.historik[1].post.text(),
        format!(
            "Faktura {} kunde inte skickas till ekonomi@kund.se: 554 Avvisat",
            utfärdad.nummer
        )
    );

    let utskicken = utskick::utskicken(&db, &anna, id, utfärdad.id)
        .await
        .unwrap();
    assert_eq!(utskicken.len(), 1);
    assert_eq!(utskicken[0].fel.as_deref(), Some("554 Avvisat"));
}

#[tokio::test]
async fn efter_ett_misslyckat_utskick_föreslås_samma_adress_och_kundregistrets_visas() {
    let db = Databas::tillfällig().await;
    let (anna, id) = företag(&db).await;
    let k = testhjalp::kund(&db, &anna, id, "Kund AB").await;
    let utkast = testhjalp::kundfakturautkast(&db, &anna, id, k, "2026-03-10").await;
    let f = kundfaktura::utfärda(&db, &klocka(), &anna, id, utkast)
        .await
        .unwrap();
    let ny = |till: &str| utskick::NyttUtskick {
        till: till.into(),
        meddelande: "Hej".into(),
    };
    let utfall = utskick::skicka(
        &db,
        &klocka(),
        &anna,
        id,
        f.id,
        &ny("fel@kund.se"),
        &Brevlåda::som_avvisar("550 Ingen sådan mottagare"),
    )
    .await
    .unwrap();
    assert!(matches!(utfall, Utfall::Misslyckat(_)));
    kund::ändra(
        &db,
        &klocka(),
        &anna,
        id,
        k,
        &NyKund {
            namn: "Kund AB".into(),
            adress: "Storgatan 1\n111 22 Stockholm".into(),
            epost: "faktura@kund.se".into(),
            ..NyKund::default()
        },
    )
    .await
    .unwrap();

    let förslag = utskick::förslag(&db, &anna, id, f.id).await.unwrap();
    assert_eq!(förslag.till, "fel@kund.se");
    assert_eq!(förslag.kundregistret, "faktura@kund.se");
    let senaste = förslag.misslyckat.expect("det misslyckade utskicket");
    assert_eq!(senaste.fel.as_deref(), Some("550 Ingen sådan mottagare"));

    // Skickas igen med kundregistrets adress.
    let låda = Brevlåda::ny();
    let utfall = utskick::skicka(
        &db,
        &klocka(),
        &anna,
        id,
        f.id,
        &ny(&förslag.kundregistret),
        &låda,
    )
    .await
    .unwrap();
    assert_eq!(utfall, Utfall::Skickat);
    assert_eq!(låda.mejl()[0].till, "faktura@kund.se");
    let förslag = utskick::förslag(&db, &anna, id, f.id).await.unwrap();
    assert_eq!(förslag.till, "faktura@kund.se");
    assert!(förslag.misslyckat.is_none());
}

#[tokio::test]
async fn en_faktura_lämnad_på_annat_sätt_loggas_som_ett_utskick() {
    let db = Databas::tillfällig().await;
    let (anna, id) = företag(&db).await;
    let f = testhjalp::utfärdad_kundfaktura(&db, &anna, id, "Kund AB", "2026-03-10").await;

    utskick::lämnad_på_annat_sätt(&db, &klocka(), &anna, id, f.id, Lämningssätt::Post)
        .await
        .unwrap();

    let utskicken = utskick::utskicken(&db, &anna, id, f.id).await.unwrap();
    assert_eq!(utskicken.len(), 1);
    assert_eq!(utskicken[0].sätt, Sätt::Lämnad(Lämningssätt::Post));
    assert_eq!(utskicken[0].signatur.namn, "anna@example.se");
    let historik = kundfaktura::kundfakturan(&db, &anna, id, f.id)
        .await
        .unwrap()
        .historik;
    let post = &historik.last().unwrap().post;
    assert_eq!(post.typ, "utskick lämnat på annat sätt");
    assert_eq!(
        post.text(),
        format!("Faktura {} lämnad på annat sätt: Med post", f.nummer)
    );
}

#[tokio::test]
async fn ett_utkast_och_en_ogiltig_adress_skickas_inte() {
    let db = Databas::tillfällig().await;
    let (anna, id) = företag(&db).await;
    let k = testhjalp::kund(&db, &anna, id, "Kund AB").await;
    let utkast = testhjalp::kundfakturautkast(&db, &anna, id, k, "2026-03-10").await;
    let låda = Brevlåda::ny();
    let ny = |till: &str| utskick::NyttUtskick {
        till: till.into(),
        meddelande: "Hej".into(),
    };

    let fel = utskick::skicka(&db, &klocka(), &anna, id, utkast, &ny("a@b.se"), &låda)
        .await
        .unwrap_err();
    assert!(matches!(fel, UtskickFel::InteUtfärdad), "{fel:?}");
    let fel =
        utskick::lämnad_på_annat_sätt(&db, &klocka(), &anna, id, utkast, Lämningssätt::Post)
            .await
            .unwrap_err();
    assert!(matches!(fel, UtskickFel::InteUtfärdad), "{fel:?}");

    let f = kundfaktura::utfärda(&db, &klocka(), &anna, id, utkast)
        .await
        .unwrap();
    let fel = utskick::skicka(&db, &klocka(), &anna, id, f.id, &ny("ingen adress"), &låda)
        .await
        .unwrap_err();
    assert!(matches!(fel, UtskickFel::Mottagare(_)), "{fel:?}");
    assert!(låda.mejl().is_empty());
    assert!(
        utskick::utskicken(&db, &anna, id, f.id)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn utan_e_post_hos_kunden_utfärdas_fakturan_utan_utskick() {
    let db = Databas::tillfällig().await;
    let (anna, id) = företag(&db).await;
    let k = kund::skapa(
        &db,
        &klocka(),
        &anna,
        id,
        &NyKund {
            namn: "Utan Post AB".into(),
            adress: "Storgatan 1\n111 22 Stockholm".into(),
            ..NyKund::default()
        },
    )
    .await
    .unwrap();
    let utkast = testhjalp::kundfakturautkast(&db, &anna, id, k, "2026-03-10").await;
    let låda = Brevlåda::ny();

    let (f, utfall) = utskick::utfärda_och_skicka(&db, &klocka(), &anna, id, utkast, &låda)
        .await
        .unwrap();
    assert_eq!(utfall, None);
    assert!(låda.mejl().is_empty());
    assert!(
        utskick::utskicken(&db, &anna, id, f.id)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn utskicken_går_inte_att_ändra_eller_radera() {
    let db = Databas::tillfällig().await;
    let (anna, id) = företag(&db).await;
    let f = testhjalp::utfärdad_kundfaktura(&db, &anna, id, "Kund AB", "2026-03-10").await;
    utskick::lämnad_på_annat_sätt(&db, &klocka(), &anna, id, f.id, Lämningssätt::Portal)
        .await
        .unwrap();

    for sql in [
        "UPDATE kundfaktura_utskick SET satt = 'post'",
        "DELETE FROM kundfaktura_utskick",
    ] {
        let fel = testhjalp::kör_sql(&db, sql).await.unwrap_err();
        assert!(
            fel.to_string().contains("ett utskick går inte att ändra"),
            "{sql}: {fel}"
        );
    }
}

#[tokio::test]
async fn ett_utskick_kräver_en_utfärdad_faktura_i_schemat() {
    let db = Databas::tillfällig().await;
    let (anna, id) = företag(&db).await;
    let k = testhjalp::kund(&db, &anna, id, "Kund AB").await;
    testhjalp::kundfakturautkast(&db, &anna, id, k, "2026-03-10").await;

    let fel = testhjalp::kör_sql(
        &db,
        "INSERT INTO kundfaktura_utskick
             (foretag_id, faktura_id, tid, kanal, satt, resultat)
         SELECT foretag_id, id, '2026-03-12T09:30:00Z', 'webb', 'post', 'lamnad'
         FROM kundfaktura",
    )
    .await
    .unwrap_err();
    assert!(
        fel.to_string().contains("bara en utfärdad faktura skickas"),
        "{fel}"
    );
}
