//! Att göra med Följ upp, fakturautkasten och de nya kunderna och leverantörerna (#153), genom
//! `core` mot en tillfällig databas: utskick som misslyckades och utfärdade fakturor som inte är
//! skickade tills de skickas eller markeras som lämnade, alla sorters fakturautkast, de nya
//! kunderna och leverantörerna från utkasten, och antalet i toppmenyn.

use maya_core::aktor::Aktör;
use maya_core::att_gora::{
    self, Fakturautkast, Fakturautkastsak, NyPart, NyPartsak, Sak, Uppföljningssak,
};
use maya_core::extern_kundfaktura;
use maya_core::foretag::FöretagsId;
use maya_core::kundfaktura;
use maya_core::leverantorsfaktura;
use maya_core::utskick::{self, NyttUtskick, Utfall};
use maya_core::{Databas, Klocka, testhjalp};
use maya_domain::kund::NyKund;
use maya_domain::leverantor::NyLeverantör;
use maya_domain::leverantorsfaktura::NyLeverantörsfaktura;
use maya_domain::utskick::Lämningssätt;

/// Onsdag 14 oktober 2026, förmiddag i Stockholm.
fn i_dag() -> Klocka {
    Klocka::fast("2026-10-14T08:00:00Z".parse().unwrap())
}

async fn saker(db: &Databas, anna: &Aktör, id: FöretagsId) -> Vec<Sak> {
    att_gora::att_göra(db, &i_dag(), &testhjalp::config(), anna, id)
        .await
        .unwrap()
        .saker
}

async fn antal(db: &Databas, anna: &Aktör, id: FöretagsId) -> usize {
    att_gora::antal(db, &i_dag(), &testhjalp::config(), anna)
        .await
        .unwrap()[&id]
}

fn uppföljningar(saker: &[Sak]) -> Vec<&Uppföljningssak> {
    saker
        .iter()
        .filter_map(|s| match s {
            Sak::Uppföljning(u) => Some(u),
            _ => None,
        })
        .collect()
}

fn fakturautkast(saker: &[Sak]) -> Vec<&Fakturautkastsak> {
    saker
        .iter()
        .filter_map(|s| match s {
            Sak::Fakturautkast(u) => Some(u),
            _ => None,
        })
        .collect()
}

fn nya_parter(saker: &[Sak]) -> Vec<&NyPartsak> {
    saker
        .iter()
        .filter_map(|s| match s {
            Sak::NyPart(p) => Some(p),
            _ => None,
        })
        .collect()
}

fn utskick(till: &str) -> NyttUtskick {
    NyttUtskick {
        till: till.into(),
        meddelande: "Hej! Här kommer fakturan.".into(),
    }
}

#[tokio::test]
async fn ett_misslyckat_eller_uteblivet_utskick_följs_upp_tills_fakturan_skickats_eller_lämnats() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::fakturaföretag(&db).await;
    // Driftvarningarna i testkonfigurationen räknas i alla företag.
    let bas = antal(&db, &anna, id).await;
    // Inte skickad: utfärdad utan utskick.
    let ej = testhjalp::utfärdad_kundfaktura(&db, &anna, id, "Ej Skickad AB", "2026-10-01").await;
    // Misslyckades: servern avvisade mejlet.
    let studs = testhjalp::utfärdad_kundfaktura(&db, &anna, id, "Studs AB", "2026-10-02").await;
    let utfall = utskick::skicka(
        &db,
        &i_dag(),
        &anna,
        id,
        studs.id,
        &utskick("fel@studs.se"),
        &testhjalp::Brevlåda::som_avvisar("550 mailbox unavailable"),
    )
    .await
    .unwrap();
    assert!(matches!(utfall, Utfall::Misslyckat(_)));
    // Skickad och lämnad på annat sätt: inget att följa upp.
    let skickad = testhjalp::utfärdad_kundfaktura(&db, &anna, id, "Skickad AB", "2026-10-03").await;
    utskick::skicka(
        &db,
        &i_dag(),
        &anna,
        id,
        skickad.id,
        &utskick("a@skickad.se"),
        &testhjalp::Brevlåda::ny(),
    )
    .await
    .unwrap();
    let lämnad = testhjalp::utfärdad_kundfaktura(&db, &anna, id, "Lämnad AB", "2026-10-04").await;
    utskick::lämnad_på_annat_sätt(&db, &i_dag(), &anna, id, lämnad.id, Lämningssätt::Post)
        .await
        .unwrap();
    // En extern faktura skickas av det andra systemet och följs inte upp.
    let kund = testhjalp::kund(&db, &anna, id, "Extern AB").await;
    testhjalp::registrerad_extern_kundfaktura(&db, &anna, id, kund, "X-1").await;

    let s = saker(&db, &anna, id).await;
    let följ: Vec<(String, Option<(String, String)>)> = uppföljningar(&s)
        .iter()
        .map(|u| {
            (
                u.kund.clone(),
                u.misslyckat
                    .as_ref()
                    .map(|m| (m.till.clone(), m.fel.clone())),
            )
        })
        .collect();
    assert_eq!(
        följ,
        vec![
            ("Ej Skickad AB".to_string(), None),
            (
                "Studs AB".to_string(),
                Some((
                    "fel@studs.se".to_string(),
                    "550 mailbox unavailable".to_string()
                ))
            ),
        ]
    );
    let u = uppföljningar(&s)[0];
    assert_eq!(
        (u.faktura, u.nummer, u.total),
        (ej.id, ej.nummer, 1_187_500)
    );
    // Två att följa upp och den externa fakturan, som har förfallit.
    assert_eq!(antal(&db, &anna, id).await, bas + 3);

    // Ett nytt utskick som går fram, eller en markering, tar bort fakturan ur Följ upp.
    utskick::skicka(
        &db,
        &i_dag(),
        &anna,
        id,
        studs.id,
        &utskick("ekonomi@studs.se"),
        &testhjalp::Brevlåda::ny(),
    )
    .await
    .unwrap();
    utskick::lämnad_på_annat_sätt(&db, &i_dag(), &anna, id, ej.id, Lämningssätt::Portal)
        .await
        .unwrap();
    assert!(uppföljningar(&saker(&db, &anna, id).await).is_empty());
    assert_eq!(antal(&db, &anna, id).await, bas + 1);

    // Ett nytt utskick som misslyckas efter ett som gick fram följs upp igen.
    utskick::skicka(
        &db,
        &i_dag(),
        &anna,
        id,
        skickad.id,
        &utskick("ny@skickad.se"),
        &testhjalp::Brevlåda::som_avvisar("timeout"),
    )
    .await
    .unwrap();
    let s = saker(&db, &anna, id).await;
    let u = uppföljningar(&s);
    assert_eq!(u.len(), 1);
    assert_eq!(u[0].faktura, skickad.id);
    assert_eq!(antal(&db, &anna, id).await, bas + 2);
}

#[tokio::test]
async fn alla_sorters_fakturautkast_visas_med_fakturadatum_och_motpart() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::fakturaföretag(&db).await;
    // Driftvarningarna i testkonfigurationen räknas i alla företag.
    let bas = antal(&db, &anna, id).await;
    let kund = testhjalp::kund(&db, &anna, id, "Kund AB").await;
    let egen = testhjalp::kundfakturautkast(&db, &anna, id, kund, "2026-09-30").await;
    let (externt, _) = extern_kundfaktura::ladda_upp(
        &db,
        &i_dag(),
        &anna,
        id,
        ("X-9.pdf", "application/pdf", b"%PDF-1.4 extern"),
    )
    .await
    .unwrap();
    let (lev, _) = leverantorsfaktura::ladda_upp(
        &db,
        &i_dag(),
        &anna,
        id,
        ("hyra.pdf", "application/pdf", b"%PDF-1.4 hyra"),
    )
    .await
    .unwrap();
    let leverantör = testhjalp::leverantör(&db, &anna, id, "Hyresvärden AB").await;
    leverantorsfaktura::spara_utkast(
        &db,
        &i_dag(),
        &anna,
        id,
        lev,
        &NyLeverantörsfaktura {
            leverantör: leverantör.to_string(),
            fakturanummer: "H-10".into(),
            fakturadatum: "2026-08-15".into(),
            totalbelopp: "1 250".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    // En utfärdad faktura är inget utkast.
    testhjalp::utfärdad_kundfaktura(&db, &anna, id, "Utfärdad AB", "2026-10-01").await;

    let s = saker(&db, &anna, id).await;
    let utkast: Vec<(Fakturautkast, String, Option<String>, Option<i64>)> = fakturautkast(&s)
        .iter()
        .map(|u| {
            (
                u.utkast,
                u.motpart.clone(),
                u.fakturadatum.map(|d| d.to_string()),
                u.total,
            )
        })
        .collect();
    assert_eq!(
        utkast,
        vec![
            (
                Fakturautkast::Kundfaktura(egen),
                "Kund AB".to_string(),
                Some("2026-09-30".to_string()),
                Some(1_187_500)
            ),
            (
                Fakturautkast::ExternKundfaktura(externt),
                String::new(),
                None,
                Some(0)
            ),
            (
                Fakturautkast::Leverantörsfaktura(lev),
                "Hyresvärden AB".to_string(),
                Some("2026-08-15".to_string()),
                Some(125_000)
            ),
        ]
    );
    // Tre utkast och Följ upp för den utfärdade fakturan.
    assert_eq!(antal(&db, &anna, id).await, bas + 4);

    // Ett raderat utkast försvinner.
    kundfaktura::radera_utkast(&db, &i_dag(), &anna, id, egen)
        .await
        .unwrap();
    assert_eq!(fakturautkast(&saker(&db, &anna, id).await).len(), 2);
    assert_eq!(antal(&db, &anna, id).await, bas + 3);
}

#[tokio::test]
async fn en_upplåst_faktura_står_bland_utkasten_tills_den_registreras_om() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::fakturaföretag(&db).await;
    let kund = testhjalp::kund(&db, &anna, id, "Kund AB").await;
    let f = testhjalp::registrerad_extern_kundfaktura(&db, &anna, id, kund, "X-1").await;
    assert!(fakturautkast(&saker(&db, &anna, id).await).is_empty());
    extern_kundfaktura::rätta_registrering(&db, &i_dag(), &anna, id, f, "fel belopp")
        .await
        .unwrap();
    let s = saker(&db, &anna, id).await;
    let u = fakturautkast(&s);
    assert_eq!(u.len(), 1);
    assert_eq!(u[0].utkast, Fakturautkast::ExternKundfaktura(f));
    assert!(u[0].upplåst);
    assert_eq!(u[0].fakturanummer, "X-1");
}

#[tokio::test]
async fn nya_kunder_och_leverantörer_från_utkasten_står_under_att_granska() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::fakturaföretag(&db).await;
    // Driftvarningarna i testkonfigurationen räknas i alla företag.
    let bas = antal(&db, &anna, id).await;
    let k = testhjalp::kund(&db, &anna, id, "Kund AB").await;
    let utkast = kundfaktura::spara_utkast(
        &db,
        &i_dag(),
        &anna,
        id,
        None,
        &maya_domain::kundfaktura::NyKundfaktura {
            kund: maya_domain::kundfaktura::NY.into(),
            ny_kund: NyKund {
                namn: "Nya Kunden AB".into(),
                epost: "ekonomi@nya.se".into(),
                betalningsvillkor: "10".into(),
                ..Default::default()
            },
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let (lev, _) = leverantorsfaktura::ladda_upp(
        &db,
        &i_dag(),
        &anna,
        id,
        ("el.pdf", "application/pdf", b"%PDF-1.4 el"),
    )
    .await
    .unwrap();
    leverantorsfaktura::spara_utkast(
        &db,
        &i_dag(),
        &anna,
        id,
        lev,
        &NyLeverantörsfaktura {
            leverantör: maya_domain::leverantorsfaktura::NY.into(),
            ny_leverantör: NyLeverantör {
                namn: "Elbolaget AB".into(),
                bankgiro: "5050-1055".into(),
                land: "SE".into(),
                ..Default::default()
            },
            ..Default::default()
        },
    )
    .await
    .unwrap();
    // En kund i registret är ingen ny kund.
    testhjalp::kundfakturautkast(&db, &anna, id, k, "2026-10-01").await;

    let s = saker(&db, &anna, id).await;
    let nya: Vec<(&str, Fakturautkast)> = nya_parter(&s)
        .iter()
        .map(|p| (p.namn.as_str(), p.utkast))
        .collect();
    assert_eq!(
        nya,
        vec![
            ("Nya Kunden AB", Fakturautkast::Kundfaktura(utkast)),
            ("Elbolaget AB", Fakturautkast::Leverantörsfaktura(lev)),
        ]
    );
    assert!(matches!(nya_parter(&s)[0].part, NyPart::Kund(_)));
    assert!(matches!(nya_parter(&s)[1].part, NyPart::Leverantör(_)));
    assert!(
        nya_parter(&s)[0]
            .uppgifter
            .contains(&("E-post", "ekonomi@nya.se".to_string()))
    );
    assert!(
        nya_parter(&s)[1]
            .uppgifter
            .contains(&("Bankgiro", "5050-1055".to_string()))
    );
    // Utkasten märks med sin nya motpart.
    assert!(fakturautkast(&s).iter().filter(|u| u.ny_motpart).count() == 2);
    // Tre utkast och två nya parter.
    assert_eq!(antal(&db, &anna, id).await, bas + 5);

    // Den nya kunden försvinner med sitt utkast.
    kundfaktura::radera_utkast(&db, &i_dag(), &anna, id, utkast)
        .await
        .unwrap();
    let s = saker(&db, &anna, id).await;
    assert_eq!(nya_parter(&s).len(), 1);
    assert_eq!(antal(&db, &anna, id).await, bas + 3);
}
