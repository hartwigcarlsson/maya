//! Kreditfaktura till kund genom `core` mot en tillfällig databas (#140): hel och delvis kredit
//! enligt tabellen över automatkonteringar, nästa nummer i den gemensamma serien, spärrarna,
//! tillgodohavandet och återbetalningen, och triggrarna som skyddar kreditfakturorna.

use jiff::civil::date;
use maya_core::betalning::{self, BetalningFel, Faktura};
use maya_core::kundfaktura::{self, KundfakturaFel, KundfakturaId, Status, Utfärdad};
use maya_core::verifikat::{self, Flöde, VerifikatId};
use maya_core::{Databas, Klocka, behandlingshistorik, kontering, testhjalp};
use maya_core::{aktor::Aktör, foretag::FöretagsId};
use maya_domain::betalning::NyBetalning;
use maya_domain::kontering::NyKontering;
use maya_domain::kreditfaktura::NyKreditering;
use maya_domain::reskontra::{Märke, kundfakturans_märke};
use maya_pdf::testhjalp::{innehåller, sidor, är_pdfa_2b};

const NU: &str = "2026-10-18T08:00:00Z";

fn klocka() -> Klocka {
    Klocka::fast(NU.parse().unwrap())
}

/// 10 timmar à 950 kr med 25 % moms: 11 875,00 kr, fakturerad 2026-03-10.
const TOTAL: i64 = 1_187_500;

async fn faktura(db: &Databas) -> (Aktör, FöretagsId, Utfärdad) {
    let (anna, id) = testhjalp::fakturaföretag(db).await;
    let f = testhjalp::utfärdad_kundfaktura(db, &anna, id, "Kund AB", "2026-03-10").await;
    (anna, id, f)
}

fn hela() -> NyKreditering {
    NyKreditering {
        hela: true,
        antal: Vec::new(),
    }
}

fn antal(a: &str) -> NyKreditering {
    NyKreditering {
        hela: false,
        antal: vec![a.into()],
    }
}

async fn kreditera(
    db: &Databas,
    anna: &Aktör,
    id: FöretagsId,
    f: KundfakturaId,
    k: NyKreditering,
) -> Result<Utfärdad, KundfakturaFel> {
    let utkast = kundfaktura::kreditera(db, &klocka(), anna, id, f, &k).await?;
    kundfaktura::utfärda(db, &klocka(), anna, id, utkast).await
}

async fn rader(
    db: &Databas, anna: &Aktör, id: FöretagsId, v: VerifikatId
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

async fn återstår(db: &Databas, anna: &Aktör, id: FöretagsId, f: KundfakturaId) -> i64 {
    betalning::reskontrapost(db, anna, id, Faktura::Kund(f))
        .await
        .unwrap()
        .återstår
}

fn betalning(belopp: &str) -> NyBetalning {
    NyBetalning {
        betaldag: "2026-10-18".into(),
        belopp: belopp.into(),
        konto: "1930".into(),
        ..Default::default()
    }
}

#[tokio::test]
async fn hel_kredit_får_nästa_nummer_och_vänder_utfärdandets_kontering() {
    let db = Databas::tillfällig().await;
    let (anna, id, f) = faktura(&db).await;
    let k = kreditera(&db, &anna, id, f.id, hela()).await.unwrap();
    assert_eq!(k.nummer, f.nummer + 1);

    let v = verifikat::verifikatet(&db, &anna, id, k.verifikat.as_ref().unwrap().id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(v.datum, Some(date(2026, 10, 18)));
    assert_eq!(
        v.beskrivning,
        format!(
            "Kreditfaktura {}, Kund AB, krediterar faktura {}",
            k.nummer, f.nummer
        )
    );
    assert_eq!(
        rader(&db, &anna, id, k.verifikat.as_ref().unwrap().id).await,
        [(1510, 0, TOTAL), (3001, 950_000, 0), (2611, 237_500, 0)]
    );
    let flöde = v.flöde.expect("länken till kreditfakturan");
    assert_eq!(flöde.flöde, Flöde::Kundfaktura);
    assert_eq!(flöde.id.to_string(), k.id.to_string());
    assert_eq!(
        flöde.text,
        format!("kreditfaktura {} till Kund AB", k.nummer)
    );

    // Originalet är krediterat: inget återstår, och dokumenten länkar åt båda hållen.
    let post = betalning::reskontrapost(&db, &anna, id, Faktura::Kund(f.id))
        .await
        .unwrap();
    assert_eq!(post.återstår, 0);
    assert_eq!(post.krediterat, TOTAL);
    assert_eq!(
        kundfakturans_märke(
            post.total,
            post.krediterat,
            post.återstår,
            post.kundförlust,
            post.förfallodag,
            date(2026, 10, 18)
        ),
        Märke::Krediterad
    );
    let original = kundfaktura::kundfakturan(&db, &anna, id, f.id)
        .await
        .unwrap();
    assert_eq!(original.krediteras_av.len(), 1);
    assert_eq!(original.krediteras_av[0].nummer, k.nummer);
    assert_eq!(original.krediteras_av[0].total, -TOTAL);
    let historik: Vec<&str> = original
        .historik
        .iter()
        .map(|h| h.post.typ.as_str())
        .collect();
    assert_eq!(historik, ["kundfaktura utfärdad", "kreditfaktura utfärdad"]);
    let kredit = kundfaktura::kundfakturan(&db, &anna, id, k.id)
        .await
        .unwrap();
    let u = kredit.utfärdad.unwrap();
    let h = u.krediterar.expect("hänvisningen");
    assert_eq!(
        (h.nummer, h.fakturadatum.as_str()),
        (f.nummer, "2026-03-10")
    );
    assert_eq!(u.total, -TOTAL);
    assert_eq!(u.rader[0].antal.to_string(), "-10");

    // Listan: kreditfakturan räknas av mot originalet och har inget kvar att betala.
    let lista = kundfaktura::lista(&db, &anna, id).await.unwrap();
    let rad = |i: KundfakturaId| lista.iter().find(|r| r.id == i).unwrap();
    assert_eq!(rad(k.id).krediterar, Some(f.nummer.to_string()));
    assert_eq!((rad(k.id).total, rad(k.id).återstår), (-TOTAL, 0));
    assert_eq!((rad(f.id).krediterat, rad(f.id).återstår), (TOTAL, 0));
}

#[tokio::test]
async fn kreditfakturans_pdf_har_hänvisningen_till_originalet() {
    let db = Databas::tillfällig().await;
    let (anna, id, f) = faktura(&db).await;
    let k = kreditera(&db, &anna, id, f.id, hela()).await.unwrap();
    let fil = maya_core::underlag::filen(&db, &anna, id, k.underlag)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fil.filnamn, format!("Kreditfaktura_{}.pdf", k.nummer));
    är_pdfa_2b(&fil.innehåll);
    let text = sidor(&fil.innehåll).join("");
    for uppgift in [
        &format!("Kreditfaktura {}", k.nummer),
        &format!("Krediterar faktura {} daterad 2026-03-10", f.nummer),
        "Kund AB",
        "Org.nr 556036-0793",
        "Momsreg.nr SE556036079301",
        "Moms 25 % på -9 500,00",
        "Att få tillbaka -11 875,00 kr",
    ] {
        assert!(innehåller(&text, uppgift), "{uppgift} saknas i {text}");
    }
}

#[tokio::test]
async fn delvis_kredit_krediterar_antalet_och_högst_det_som_finns_kvar() {
    let db = Databas::tillfällig().await;
    let (anna, id, f) = faktura(&db).await;
    let k = kreditera(&db, &anna, id, f.id, antal("4")).await.unwrap();
    assert_eq!(
        rader(&db, &anna, id, k.verifikat.as_ref().unwrap().id).await,
        [(1510, 0, 475_000), (3001, 380_000, 0), (2611, 95_000, 0)]
    );
    assert_eq!(återstår(&db, &anna, id, f.id).await, TOTAL - 475_000);

    // Hela fakturan går inte att kreditera igen: bara 6 timmar finns kvar.
    let fel = kundfaktura::kreditera(&db, &klocka(), &anna, id, f.id, &hela())
        .await
        .unwrap_err();
    assert!(
        matches!(&fel, KundfakturaFel::Kreditering(s) if s[0].kod == "kredit_overstiger"),
        "{fel:?}"
    );
    // Ett utkast som skapades innan den andra krediteringen spärras när det ska utfärdas.
    let för_mycket = kundfaktura::kreditera(&db, &klocka(), &anna, id, f.id, &antal("6"))
        .await
        .unwrap();
    kreditera(&db, &anna, id, f.id, antal("6")).await.unwrap();
    let fel = kundfaktura::utfärda(&db, &klocka(), &anna, id, för_mycket)
        .await
        .unwrap_err();
    assert!(
        matches!(&fel, KundfakturaFel::Spärrat(s) if s.iter().any(|s| s.kod == "kredit_overstiger")),
        "{fel:?}"
    );
    assert_eq!(återstår(&db, &anna, id, f.id).await, 0);
    let fel = kundfaktura::kreditera(&db, &klocka(), &anna, id, f.id, &antal("1"))
        .await
        .unwrap_err();
    assert!(
        matches!(&fel, KundfakturaFel::Kreditering(s) if s[0].kod == "redan_krediterad"),
        "{fel:?}"
    );
}

#[tokio::test]
async fn fakturor_och_kreditfakturor_delar_en_obruten_serie() {
    let db = Databas::tillfällig().await;
    let (anna, id, f) = faktura(&db).await;
    let k = kreditera(&db, &anna, id, f.id, antal("1")).await.unwrap();
    let nästa = testhjalp::utfärdad_kundfaktura(&db, &anna, id, "Kund 2 AB", "2026-10-18").await;
    assert_eq!(
        [f.nummer, k.nummer, nästa.nummer],
        [f.nummer, f.nummer + 1, f.nummer + 2]
    );
}

#[tokio::test]
async fn kreditfakturan_följer_en_rättad_kontering_och_kan_inte_krediteras_själv() {
    let db = Databas::tillfällig().await;
    let (anna, id, f) = faktura(&db).await;
    kontering::rätta(
        &db,
        &klocka(),
        &anna,
        id,
        f.id,
        &NyKontering {
            konton: vec!["3002".into()],
            orsak: "fel konto".into(),
        },
    )
    .await
    .unwrap();
    let k = kreditera(&db, &anna, id, f.id, hela()).await.unwrap();
    assert_eq!(
        rader(&db, &anna, id, k.verifikat.as_ref().unwrap().id).await,
        [(1510, 0, TOTAL), (3002, 950_000, 0), (2611, 237_500, 0)]
    );
    for original in [k.id, f.id] {
        let fel = kundfaktura::kreditera(&db, &klocka(), &anna, id, original, &hela())
            .await
            .unwrap_err();
        assert!(
            matches!(
                fel,
                KundfakturaFel::FinnsInte | KundfakturaFel::Kreditering(_)
            ),
            "{fel:?}"
        );
    }
    // En kreditfaktura är ingen fordran att betala.
    let fel = betalning::registrera(
        &db,
        &klocka(),
        &anna,
        id,
        Faktura::Kund(k.id),
        &betalning("1"),
    )
    .await
    .unwrap_err();
    assert!(matches!(fel, BetalningFel::FinnsInte), "{fel:?}");
}

#[tokio::test]
async fn kreditutkastet_behåller_originalets_kund_och_hänvisning_när_det_sparas() {
    let db = Databas::tillfällig().await;
    let (anna, id, f) = faktura(&db).await;
    let annan = testhjalp::kund(&db, &anna, id, "Annan AB").await;
    let utkast = kundfaktura::kreditera(&db, &klocka(), &anna, id, f.id, &antal("2"))
        .await
        .unwrap();
    let mut formulär = kundfaktura::kundfakturan(&db, &anna, id, utkast)
        .await
        .unwrap()
        .formulär;
    assert_eq!(formulär.krediterar.as_ref().unwrap().nummer, f.nummer);
    // Formuläret som webbläsaren skickar har ingen hänvisning, och kunden är bytt.
    formulär.krediterar = None;
    formulär.kund = annan.to_string();
    formulär.rader[0].beskrivning = "Konsulttimmar, fel antal".into();
    kundfaktura::spara_utkast(&db, &klocka(), &anna, id, Some(utkast), &formulär)
        .await
        .unwrap();
    let sparat = kundfaktura::kundfakturan(&db, &anna, id, utkast)
        .await
        .unwrap();
    assert_eq!(sparat.status, Status::Utkast);
    assert_eq!(sparat.kund, "Kund AB");
    assert_eq!(sparat.formulär.krediterar.unwrap().nummer, f.nummer);
    let k = kundfaktura::utfärda(&db, &klocka(), &anna, id, utkast)
        .await
        .unwrap();
    let u = kundfaktura::kundfakturan(&db, &anna, id, k.id)
        .await
        .unwrap()
        .utfärdad
        .unwrap();
    assert_eq!(u.kund.namn, "Kund AB");
    assert_eq!(u.rader[0].beskrivning, "Konsulttimmar, fel antal");
}

#[tokio::test]
async fn en_kreditfaktura_utan_negativ_summa_utfärdas_inte() {
    let db = Databas::tillfällig().await;
    let (anna, id, f) = faktura(&db).await;
    let utkast = kundfaktura::kreditera(&db, &klocka(), &anna, id, f.id, &hela())
        .await
        .unwrap();
    let mut formulär = kundfaktura::kundfakturan(&db, &anna, id, utkast)
        .await
        .unwrap()
        .formulär;
    formulär.rader[0].à_pris = "0".into();
    kundfaktura::spara_utkast(&db, &klocka(), &anna, id, Some(utkast), &formulär)
        .await
        .unwrap();
    let fel = kundfaktura::utfärda(&db, &klocka(), &anna, id, utkast)
        .await
        .unwrap_err();
    assert!(
        matches!(&fel, KundfakturaFel::Spärrat(s) if s.iter().map(|s| s.kod).eq(["summa_inte_negativ"])),
        "{fel:?}"
    );
    assert_eq!(återstår(&db, &anna, id, f.id).await, TOTAL);
}

#[tokio::test]
async fn kredit_av_en_betald_faktura_ger_ett_tillgodohavande_som_återbetalas() {
    let db = Databas::tillfällig().await;
    let (anna, id, f) = faktura(&db).await;
    betalning::registrera(
        &db,
        &klocka(),
        &anna,
        id,
        Faktura::Kund(f.id),
        &betalning("11 875"),
    )
    .await
    .unwrap();
    kreditera(&db, &anna, id, f.id, antal("4")).await.unwrap();
    let post = betalning::reskontrapost(&db, &anna, id, Faktura::Kund(f.id))
        .await
        .unwrap();
    assert_eq!(post.återstår, -475_000);
    assert_eq!(
        kundfakturans_märke(
            post.total,
            post.krediterat,
            post.återstår,
            post.kundförlust,
            post.förfallodag,
            date(2026, 10, 18)
        ),
        Märke::Tillgodo
    );
    let kunden = maya_core::kund::kunder(&db, &anna, id).await.unwrap();
    let kund = kunden.iter().find(|k| k.namn == "Kund AB").unwrap().id;
    let fakturor = kundfaktura::kundens_fakturor(&db, &anna, id, kund)
        .await
        .unwrap();
    assert_eq!(fakturor.iter().map(|f| f.återstår).sum::<i64>(), -475_000);

    // Mer än tillgodohavandet kan inte återbetalas.
    let fel = betalning::registrera(
        &db,
        &klocka(),
        &anna,
        id,
        Faktura::Kund(f.id),
        &betalning("4 750,01"),
    )
    .await
    .unwrap_err();
    assert!(
        matches!(&fel, BetalningFel::Spärrat(s) if s[0].kod == "overaterbetalning"),
        "{fel:?}"
    );

    let r = betalning::registrera(
        &db,
        &klocka(),
        &anna,
        id,
        Faktura::Kund(f.id),
        &betalning("4 750"),
    )
    .await
    .unwrap();
    assert_eq!(r.återstår, 0);
    assert_eq!(
        rader(&db, &anna, id, r.verifikat.id).await,
        [(1930, 0, 475_000), (1510, 475_000, 0)]
    );
    let v = verifikat::verifikatet(&db, &anna, id, r.verifikat.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        v.beskrivning,
        format!(
            "Återbetalning av tillgodohavande på kundfaktura {}, Kund AB",
            f.nummer
        )
    );
    let post = betalning::reskontrapost(&db, &anna, id, Faktura::Kund(f.id))
        .await
        .unwrap();
    assert_eq!(post.betalningar.last().unwrap().belopp, -475_000);
    let typer: Vec<String> = behandlingshistorik::läs(&db, &anna, id)
        .await
        .unwrap()
        .into_iter()
        .map(|p| p.typ)
        .collect();
    assert!(typer.iter().any(|t| t == "återbetalning"), "{typer:?}");

    // Rätta betalning på återbetalningen ger tillgodohavandet tillbaka.
    let rättad = betalning::rätta(&db, &klocka(), &anna, id, r.id, "2026-10-18", "fel konto")
        .await
        .unwrap();
    assert_eq!(rättad.återstår, -475_000);
}

#[tokio::test]
async fn schemat_avvisar_en_kreditfaktura_utan_utfärdat_original_och_en_som_krediterar_för_mycket()
{
    let db = Databas::tillfällig().await;
    let (anna, id, f) = faktura(&db).await;
    let k = kreditera(&db, &anna, id, f.id, antal("4")).await.unwrap();
    // Hänvisningen pekar på ett utfärdat original som inte själv är en kreditfaktura.
    for sql in [
        "INSERT INTO kundfaktura (foretag_id, status, krediterar, skapad, skapad_kanal)
         SELECT foretag_id, 'utkast', id, 'x', 'webb' FROM kundfaktura WHERE krediterar IS NOT NULL",
        "INSERT INTO kundfaktura (foretag_id, status, krediterar, skapad, skapad_kanal)
         VALUES (1, 'utkast', 999, 'x', 'webb')",
    ] {
        let fel = testhjalp::kör_sql(&db, sql).await.unwrap_err();
        assert!(
            fel.to_string()
                .contains("en kreditfaktura krediterar en utfärdad faktura"),
            "{fel}"
        );
    }
    // Den utfärdade kreditfakturan och dess hänvisning ändras inte.
    let fel = testhjalp::kör_sql(
        &db,
        "UPDATE kundfaktura SET krediterar = NULL WHERE krediterar IS NOT NULL",
    )
    .await
    .unwrap_err();
    assert!(fel.to_string().contains("går inte att ändra"), "{fel}");
    // Ett utkast som krediterar mer än som finns kvar kan inte bli utfärdat.
    let utkast = kundfaktura::kreditera(&db, &klocka(), &anna, id, f.id, &antal("6"))
        .await
        .unwrap();
    let _ = (k, utkast);
    let fel = testhjalp::kör_sql(
        &db,
        "UPDATE kundfaktura SET status = 'utfardad', nummer = 3, referens = '3',
             kunduppgifter = '{}', saljaruppgifter = '{}', netto = -1000000, moms = 0,
             total = -1000000, underlag_id = 1, utfardad = 'x', utfardad_kanal = 'webb'
         WHERE status = 'utkast'",
    )
    .await
    .unwrap_err();
    assert!(
        fel.to_string()
            .contains("kreditfakturan krediterar mer än som finns kvar"),
        "{fel}"
    );
}

#[tokio::test]
async fn kreditfakturan_skickas_med_eget_ämne() {
    let db = Databas::tillfällig().await;
    let (anna, id, f) = faktura(&db).await;
    let k = kreditera(&db, &anna, id, f.id, hela()).await.unwrap();
    let förslag = maya_core::utskick::förslag(&db, &anna, id, k.id)
        .await
        .unwrap();
    assert!(
        förslag
            .meddelande
            .contains(&format!("kreditfaktura {}", k.nummer)),
        "{}",
        förslag.meddelande
    );
    let brevlåda = testhjalp::Brevlåda::ny();
    maya_core::utskick::skicka(
        &db,
        &klocka(),
        &anna,
        id,
        k.id,
        &maya_core::utskick::NyttUtskick {
            till: "ekonomi@kund.se".into(),
            meddelande: förslag.meddelande,
        },
        &brevlåda,
    )
    .await
    .unwrap();
    assert_eq!(
        brevlåda.mejl()[0].ämne,
        format!("Kreditfaktura {} från Exempel AB", k.nummer)
    );
}

// #140 och #141: en kundförlust på en delkrediterad faktura skriver av det som återstår efter
// krediten, och återkallelsen ger tillbaka just det.
#[tokio::test]
async fn en_kundförlust_på_en_delkrediterad_faktura_gäller_det_som_återstår_efter_krediten() {
    let db = Databas::tillfällig().await;
    let (anna, id, f) = faktura(&db).await;
    kreditera(&db, &anna, id, f.id, antal("4")).await.unwrap();
    let kvar = TOTAL - 475_000;
    assert_eq!(återstår(&db, &anna, id, f.id).await, kvar);

    let b = maya_core::kundforlust::bokför(
        &db,
        &klocka(),
        &anna,
        id,
        f.id,
        &maya_domain::kundforlust::NyKundförlust {
            skäl: "konkurs".into(),
            annat: String::new(),
            datum: "2026-10-18".into(),
        },
    )
    .await
    .unwrap();
    // 7 125 kr återstår: 5 700 netto och 1 425 moms, i proportion till 9 500 och 2 375.
    let v = b.verifikat.expect("fordran är bokförd");
    assert_eq!(
        rader(&db, &anna, id, v.id).await,
        [(6351, 570_000, 0), (2611, 142_500, 0), (1510, 0, kvar)]
    );
    let post = betalning::reskontrapost(&db, &anna, id, Faktura::Kund(f.id))
        .await
        .unwrap();
    assert_eq!((post.återstår, post.krediterat), (0, 475_000));
    assert!(post.kundförlust);
    assert_eq!(
        kundfakturans_märke(
            post.total,
            post.krediterat,
            post.återstår,
            post.kundförlust,
            post.förfallodag,
            date(2026, 10, 18)
        ),
        Märke::Kundförlust
    );
    let listan = kundfaktura::lista(&db, &anna, id).await.unwrap();
    let rad = listan.iter().find(|r| r.id == f.id).unwrap();
    assert_eq!((rad.återstår, rad.krediterat), (0, 475_000));
    assert!(rad.kundförlust);

    maya_core::kundforlust::återkalla(&db, &klocka(), &anna, id, b.id)
        .await
        .unwrap();
    assert_eq!(återstår(&db, &anna, id, f.id).await, kvar);
    let listan = kundfaktura::lista(&db, &anna, id).await.unwrap();
    let rad = listan.iter().find(|r| r.id == f.id).unwrap();
    assert_eq!(rad.återstår, kvar);
    assert!(!rad.kundförlust);
}

#[tokio::test]
async fn fakturaserien_är_obruten_över_två_räkenskapsår_med_fakturor_och_kreditfakturor() {
    // Fakturaserien är en enda serie (VF3): numren fortsätter in i nästa räkenskapsår, och
    // kreditfakturorna tar nästa nummer i samma serie.
    let db = Databas::tillfällig().await;
    let (anna, id, f1) = faktura(&db).await;
    let kreditera_den = |f: KundfakturaId, idag: &'static str| {
        let (db, anna) = (&db, &anna);
        async move {
            let klocka = Klocka::fast(idag.parse().unwrap());
            let utkast = kundfaktura::kreditera(db, &klocka, anna, id, f, &antal("1"))
                .await
                .unwrap();
            kundfaktura::utfärda(db, &klocka, anna, id, utkast)
                .await
                .unwrap()
        }
    };
    let k1 = kreditera_den(f1.id, "2026-10-18T08:00:00Z").await;
    let f2 = testhjalp::utfärdad_kundfaktura(&db, &anna, id, "Kund 2 AB", "2027-01-10").await;
    let k2 = kreditera_den(f2.id, "2027-02-01T08:00:00Z").await;
    let f3 = testhjalp::utfärdad_kundfaktura(&db, &anna, id, "Kund 3 AB", "2027-03-01").await;

    let år: Vec<String> = maya_core::foretag::företaget(&db, &anna, id)
        .await
        .unwrap()
        .räkenskapsår
        .iter()
        .map(|å| å.start.to_string())
        .collect();
    assert_eq!(år, ["2026-01-01", "2027-01-01"]);
    let n = f1.nummer;
    assert_eq!(
        [f1.nummer, k1.nummer, f2.nummer, k2.nummer, f3.nummer],
        [n, n + 1, n + 2, n + 3, n + 4]
    );
    let mut listan: Vec<String> = kundfaktura::lista(&db, &anna, id)
        .await
        .unwrap()
        .into_iter()
        .map(|f| f.fakturanummer)
        .collect();
    listan.sort();
    assert_eq!(
        listan,
        (n..n + 5).map(|n| n.to_string()).collect::<Vec<_>>()
    );
}
