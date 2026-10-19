//! Röktest med riktig server och en webbläsare med virtuella autentiserare, specens flöde från
//! början till slut: första användaren via CLI-länken, en passkey till, inloggning med e-post
//! och passkey, borttagning av passkeys utom den sista, ett nytt företag, kontoplanen som PDF,
//! ett uppladdat underlag, ett verifikat bokfört från Att göra, en rättelse med registrera om och
//! exporten av balansräkningen och SIE4.
//!
//! Kräver Chrome eller Chromium (sökvägen kan anges i `CHROME`) och ett `cargo leptos build`,
//! så att sidorna hydreras. Utan WASM:en hoppar testet över sig självt, utom i CI.

use crate::stod;

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use headless_chrome::protocol::cdp::{Browser as Webbläsaren, Page, WebAuthn};
use headless_chrome::{Browser, LaunchOptions, Tab};

const MAYA: &str = env!("CARGO_BIN_EXE_maya");
const VÄNTA: Duration = Duration::from_secs(20);

/// Samlar sidans fel (panik vid hydreringen, oväntade undantag) i `window.__fel`.
const SAMLA_FEL: &str = "window.__fel=[];\
const e=console.error.bind(console);\
console.error=(...a)=>{window.__fel.push(a.map(String).join(' '));e(...a)};\
addEventListener('error',h=>window.__fel.push(String(h.message)));\
addEventListener('unhandledrejection',h=>window.__fel.push(String(h.reason)));";

#[test]
fn forsta_anvandaren_via_cli_lanken_och_inloggning_med_passkey() {
    if !stod::sajten_finns() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let port = ledig_port();
    let bas = format!("http://localhost:{port}");
    std::fs::write(
        dir.path().join(".env"),
        format!(
            "MAYA_PUBLIC_URL={bas}\nMAYA_LISTEN=127.0.0.1:{port}\nMAYA_BACKUP_DIR={backup}\n\
             MAYA_DATA_DIR={data}\n",
            backup = dir.path().display(),
            data = dir.path().join("data").display(),
        ),
    )
    .unwrap();
    let _server = Server::starta(dir.path(), port);

    let browser = Browser::new(
        LaunchOptions::default_builder()
            .sandbox(false)
            .idle_browser_timeout(Duration::from_secs(120))
            .build()
            .unwrap(),
    )
    .expect("röktestet kräver Chrome eller Chromium; ange sökvägen i CHROME");
    let tab = browser.new_tab().unwrap();
    // Nedladdningar hamnar i testets katalog, inte i ~/Downloads.
    let nedladdningar = dir.path().join("nedladdningar");
    std::fs::create_dir(&nedladdningar).unwrap();
    tab.call_method(Webbläsaren::SetDownloadBehavior {
        behavior: Webbläsaren::SetDownloadBehaviorBehaviorOption::Allow,
        browser_context_id: None,
        download_path: Some(nedladdningar.display().to_string()),
        events_enabled: None,
    })
    .unwrap();
    tab.call_method(Page::AddScriptToEvaluateOnNewDocument {
        source: SAMLA_FEL.to_string(),
        world_name: None,
        include_command_line_api: None,
        run_immediately: None,
    })
    .unwrap();
    tab.call_method(WebAuthn::Enable {
        enable_ui: Some(false),
    })
    .unwrap();
    let första_nyckeln = autentiserare(&tab);

    // Innan någon användare finns: bara hänvisningen till CLI:t.
    tab.navigate_to(&bas).unwrap();
    vänta_på_text(&tab, "maya user create");
    assert!(tab.find_element("form[data-passkey]").is_err());

    // Driften skapar den första användaren och öppnar länken.
    let länk = skapa_användare(dir.path(), &bas);
    tab.navigate_to(&länk).unwrap();
    vänta_på_shimmen(&tab);
    tab.wait_for_element("#namn")
        .unwrap()
        .type_into("Virtuell nyckel")
        .unwrap();
    klicka_till_ny_sida(&tab, "form[data-passkey='registrera'] button");
    vänta_på_text(&tab, "Lägg till en passkey till");

    let kaka = tab
        .get_cookies()
        .unwrap()
        .into_iter()
        .find(|c| c.name == "maya_session")
        .expect("sessionscookien");
    assert!(kaka.http_only, "HttpOnly");
    assert!(kaka.secure, "Secure");
    assert_eq!(format!("{:?}", kaka.same_site), "Some(Lax)", "SameSite=Lax");

    // Länken går inte att använda igen.
    let ny_flik = browser.new_tab().unwrap();
    ny_flik.navigate_to(&länk).unwrap();
    vänta_på_shimmen(&ny_flik);
    ny_flik
        .wait_for_element("form[data-passkey='registrera'] button")
        .unwrap()
        .click()
        .unwrap();
    vänta_på_text(&ny_flik, "länken är ogiltig");
    ny_flik.close(true).unwrap();

    // En passkey till, på en annan enhet. Sessionen är nyss inloggad och har en färsk passkey.
    tab.call_method(WebAuthn::RemoveVirtualAuthenticator {
        authenticator_id: första_nyckeln,
    })
    .unwrap();
    autentiserare(&tab);
    tab.wait_for_element("form[data-passkey='lagg-till'] #namn")
        .unwrap()
        .type_into("Andra nyckeln")
        .unwrap();
    klicka_till_ny_sida(&tab, "form[data-passkey='lagg-till'] button");
    vänta_på_text(&tab, "Andra nyckeln");

    // Logga ut och in igen med den andra nyckeln: e-post först, sedan passkey.
    klicka(&tab, ".anvandarmeny summary");
    klicka_till_ny_sida(&tab, "form[action='/auth/utloggning'] button");
    vänta_på_text(&tab, "Logga in");
    vänta_på_shimmen(&tab);
    tab.wait_for_element("#epost")
        .unwrap()
        .type_into("anna@example.se")
        .unwrap();
    klicka_till_ny_sida(&tab, "form[data-passkey='logga-in'] button");
    vänta_på_text(&tab, "Lägg upp ett företag");

    // Den första nyckeln kan tas bort, men inte den sista. Sidorna når man från användarmenyn.
    klicka(&tab, ".anvandarmeny summary");
    tab.find_element("a[href='/passkeys']")
        .unwrap()
        .click()
        .unwrap();
    vänta_på_text(&tab, "Virtuell nyckel");
    inga_sidfel(&tab);
    ta_bort_passkey(&tab, "Virtuell nyckel");
    vänta_på_text_borta(&tab, "Virtuell nyckel");
    ta_bort_passkey(&tab, "Andra nyckeln");
    vänta_på_text(&tab, "Den sista passkeyn går inte att ta bort");
    vänta_på_text(&tab, "Andra nyckeln");

    // Sessionslistan visar den här sessionen.
    klicka(&tab, ".anvandarmeny summary");
    tab.find_element("a[href='/sessioner']")
        .unwrap()
        .click()
        .unwrap();
    vänta_på_text(&tab, "Den här sessionen");
    inga_sidfel(&tab);
    tab.navigate_to(&bas).unwrap();
    vänta_på_text(&tab, "Lägg upp ett företag");

    // Ett nytt företag: först med fel kontrollsiffra, sedan rätt.
    tab.find_element("a[href='/foretag/nytt']").unwrap();
    tab.navigate_to(&format!("{bas}/foretag/nytt")).unwrap();
    tab.wait_for_element("#orgnr")
        .unwrap()
        .type_into("556036-0794")
        .unwrap();
    tab.find_element("#namn")
        .unwrap()
        .type_into("Exempel AB")
        .unwrap();
    tab.evaluate(
        "document.querySelector('#start').value = '2026-01-01';\
         document.querySelector('#slut').value = '2026-12-31';",
        false,
    )
    .unwrap();
    tab.find_element("form[action='/foretag'] button")
        .unwrap()
        .click()
        .unwrap();
    vänta_på_text(&tab, "Kontrollsiffran stämmer inte");
    tab.evaluate(
        "document.querySelector('#orgnr').value = '556036-0793';\
         document.querySelector('#start').value = '2026-01-01';\
         document.querySelector('#slut').value = '2026-12-31';",
        false,
    )
    .unwrap();
    tab.find_element("form[action='/foretag'] button")
        .unwrap()
        .click()
        .unwrap();
    vänta_på_text(&tab, "Företagskontot");
    vänta_på_text(&tab, "556036-0793");
    assert!(tab.get_url().contains("/foretag/"), "{}", tab.get_url());
    let företaget = tab.get_url();

    // Kontoplanen som PDF: Skriv ut och Exportera är samma PDF, och routern på klienten tar
    // inte över länkarna.
    let kontoplan = format!("{}/installningar/kontoplan", tab.get_url());
    tab.navigate_to(&kontoplan).unwrap();
    vänta_på_text(&tab, "Exportera PDF");
    inga_sidfel(&tab);
    let svar = tab
        .evaluate(
            "(async () => {\
               const skriv = document.querySelector('a.skriv-ut');\
               const exp = document.querySelector('a.exportera-pdf');\
               const r = await fetch(exp.href);\
               const b = new Uint8Array(await r.arrayBuffer());\
               return [skriv.href === exp.href, skriv.target, exp.download, r.status,\
                       r.headers.get('content-type'), String.fromCharCode(...b.slice(0, 5))]\
                       .join(' ');\
             })()",
            true,
        )
        .unwrap()
        .value
        .unwrap();
    assert_eq!(
        svar.as_str().unwrap(),
        "true _blank kontoplan.pdf 200 application/pdf %PDF-"
    );
    tab.find_element("a.exportera-pdf")
        .unwrap()
        .click()
        .unwrap();
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(
        tab.get_url(),
        kontoplan,
        "Exportera laddar ned, sidan står kvar"
    );
    let pdf = nedladdningar.join("kontoplan.pdf");
    for _ in 0..50 {
        if pdf.exists() {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(
        pdf.exists(),
        "Exportera sparar kontoplan.pdf i testets katalog"
    );
    inga_sidfel(&tab);

    // Ett underlag laddas upp på sidan Underlag.
    let kvitto = dir.path().join("kvitto.pdf");
    std::fs::write(&kvitto, KVITTO).unwrap();
    tab.navigate_to(&format!("{företaget}/underlag")).unwrap();
    vänta_på_hydreringen(&tab);
    tab.wait_for_element("form.uppladdning input[type='file']")
        .unwrap()
        .set_input_files(&[kvitto.to_str().unwrap()])
        .unwrap();
    klicka(&tab, "form.uppladdning button[type='submit']");
    vänta_på_text(&tab, "kvitto.pdf");
    inga_sidfel(&tab);

    // Att göra: underlaget blir ett kvitto, och utkastet bokförs därifrån.
    klicka(&tab, "nav.flikar a:first-child");
    vänta_på_text(&tab, "okopplat");
    klicka(&tab, ".acard[id^='underlag-'] a.ah");
    klicka(&tab, "form[action$='/kvitto'] button");
    vänta_på_text(&tab, "Ett verifikatutkast skapades med underlaget kopplat.");
    vänta_på_hydreringen(&tab);
    fyll(&tab, "#verifikatformular #datum", "2026-03-14");
    fyll(
        &tab,
        "#verifikatformular #beskrivning",
        "Kaffe till kontoret",
    );
    fyll(&tab, "#verifikatformular [name='konto_0']", "1930");
    fyll(&tab, "#verifikatformular [name='debet_0']", "125");
    fyll(&tab, "#verifikatformular [name='konto_1']", "3001");
    fyll(&tab, "#verifikatformular [name='kredit_1']", "125");
    klicka(
        &tab,
        "#verifikatformular button[value='bokfor']:not([disabled])",
    );
    vänta_på_text(&tab, "Bokförd som V1.");
    inga_sidfel(&tab);

    // Rätta V1 med registrera om: rättelsen blir V2 och kopian bokförs med rätt belopp som V3.
    klicka(&tab, "nav.flikar a[href$='/verifikat']");
    klicka(&tab, "a[href*='/verifikat?v=']");
    vänta_på_text(&tab, "kvitto.pdf");
    klicka(&tab, "a[href*='ratta=1']");
    vänta_på_text(&tab, "Rättelse av V1");
    fyll(&tab, ".rattelse-formular [name='datum']", "2026-03-20");
    fyll(&tab, ".rattelse-formular [name='orsak']", "fel belopp");
    klicka(
        &tab,
        ".rattelse-formular button[value='registrera_om']:not([disabled])",
    );
    vänta_på_text(&tab, "Rättelsen bokfördes som V2.");
    vänta_på_hydreringen(&tab);
    fyll(&tab, "#verifikatformular [name='debet_0']", "150");
    fyll(&tab, "#verifikatformular [name='kredit_1']", "150");
    klicka(
        &tab,
        "#verifikatformular button[value='bokfor']:not([disabled])",
    );
    vänta_på_text(&tab, "V1 (kopia av dess rader)");
    vänta_på_text(&tab, "Bokfört 150,00 kr");
    // Originalet står kvar och visar att det är rättat.
    klicka(&tab, "a[href$='/verifikat?v=1']");
    vänta_på_text(&tab, "Rättad av V2");
    inga_sidfel(&tab);

    // Balansräkningen visar det rättade beloppet och exporteras som PDF och CSV.
    klicka(&tab, "nav.flikar a[href$='/rapporter']");
    klicka(&tab, ".rapportmeny a[href$='/rapporter/balansrakning']");
    vänta_på_text(&tab, "Balansräkning");
    vänta_på_text(&tab, "150,00");
    inga_sidfel(&tab);
    let pdf = hämta(&tab, "a.exportera-pdf");
    assert_eq!(
        (pdf.status, pdf.typ.as_str(), &pdf.text[..5]),
        (200, "application/pdf", "%PDF-")
    );
    let csv = hämta(&tab, "a.exportera-csv");
    assert_eq!(csv.status, 200);
    assert!(csv.typ.starts_with("text/csv"), "{}", csv.typ);
    assert!(
        csv.text.contains("1930") && csv.text.contains("150,00"),
        "{}",
        csv.text
    );

    // SIE4 för året: tre verifikat, och filen avslutas med sin kontrollsumma.
    klicka(&tab, "nav.flikar a[href$='/rapporter']");
    let sie = hämta(&tab, "a[href*='/sie4?ar=2026-01-01']");
    assert_eq!(sie.status, 200, "{}", sie.text);
    assert!(sie.text.starts_with("#FLAGGA 0"), "{}", sie.text);
    assert_eq!(sie.text.matches("#VER ").count(), 3, "{}", sie.text);
    assert!(sie.text.contains("#KSUMMA "), "{}", sie.text);
    inga_sidfel(&tab);
}

/// #145: ett underlag laddas upp, blir en leverantörsfaktura under "Vad är det här?" och
/// registreras med originalet bredvid formuläret.
#[test]
fn leverantorsfaktura_laddas_upp_och_registreras() {
    if !stod::sajten_finns() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let port = ledig_port();
    let bas = format!("http://localhost:{port}");
    std::fs::write(
        dir.path().join(".env"),
        format!(
            "MAYA_PUBLIC_URL={bas}\nMAYA_LISTEN=127.0.0.1:{port}\nMAYA_BACKUP_DIR={backup}\n\
             MAYA_DATA_DIR={data}\n",
            backup = dir.path().display(),
            data = dir.path().join("data").display(),
        ),
    )
    .unwrap();
    let _server = Server::starta(dir.path(), port);
    let browser = Browser::new(
        LaunchOptions::default_builder()
            .sandbox(false)
            .idle_browser_timeout(Duration::from_secs(120))
            .build()
            .unwrap(),
    )
    .expect("röktestet kräver Chrome eller Chromium; ange sökvägen i CHROME");
    let tab = browser.new_tab().unwrap();
    tab.call_method(Page::AddScriptToEvaluateOnNewDocument {
        source: SAMLA_FEL.to_string(),
        world_name: None,
        include_command_line_api: None,
        run_immediately: None,
    })
    .unwrap();
    tab.call_method(WebAuthn::Enable {
        enable_ui: Some(false),
    })
    .unwrap();
    autentiserare(&tab);
    tab.navigate_to(&skapa_användare(dir.path(), &bas)).unwrap();
    vänta_på_shimmen(&tab);
    tab.wait_for_element("#namn")
        .unwrap()
        .type_into("Virtuell nyckel")
        .unwrap();
    klicka(&tab, "form[data-passkey='registrera'] button");
    vänta_på_text(&tab, "Lägg till en passkey till");

    // Ett företag och leverantören Kontorsvaror AB med standardkontot 5410.
    let företaget = posta(
        &tab,
        "/foretag",
        "orgnr=556036-0793&namn=Exempel+AB&start=2026-01-01&slut=2026-12-31\
         &bokforingsmetod=faktura&momsperiod=kvartal",
    );
    let sökväg = företaget.strip_prefix(&bas).unwrap().to_string();
    posta(
        &tab,
        &format!("{sökväg}/leverantorer"),
        "namn=Kontorsvaror+AB&land=SE&bankgiro=5050-1055&standardkonto=5410",
    );

    // Originalet laddas upp som ett okopplat underlag.
    let original = dir.path().join("faktura-8812.pdf");
    std::fs::write(&original, KVITTO).unwrap();
    tab.navigate_to(&format!("{företaget}/underlag")).unwrap();
    vänta_på_hydreringen(&tab);
    tab.wait_for_element("form.uppladdning input[type='file']")
        .unwrap()
        .set_input_files(&[original.to_str().unwrap()])
        .unwrap();
    klicka(&tab, "form.uppladdning button[type='submit']");
    vänta_på_text(&tab, "faktura-8812.pdf");

    // Att göra: Vad är det här? → Leverantörsfaktura öppnar utkastet med originalet bredvid.
    klicka(&tab, "nav.flikar a:first-child");
    vänta_på_text(&tab, "okopplat");
    klicka(&tab, ".acard[id^='underlag-'] a.ah");
    klicka(&tab, "form[action$='/leverantorsfaktura'] button");
    vänta_på_text(&tab, "Ett utkast skapades med originalet kopplat.");
    vänta_på_hydreringen(&tab);
    tab.wait_for_element(".med-underlag .underlagsspalt")
        .unwrap();
    välj(&tab, "#leverantorsfakturaformular #leverantor", "1");
    fyll(&tab, "#leverantorsfakturaformular #fakturanummer", "8812");
    fyll(
        &tab,
        "#leverantorsfakturaformular #fakturadatum",
        "2026-03-14",
    );
    fyll(
        &tab,
        "#leverantorsfakturaformular #forfallodag",
        "2026-04-13",
    );
    fyll(&tab, "#leverantorsfakturaformular #totalbelopp", "1 250");
    fyll(&tab, "#leverantorsfakturaformular [name='netto_0']", "1000");
    // Rutan "Bokförs som" visar konteringen live, med standardkontot på raden.
    vänta_på_text(&tab, "Bokförs som V1 · 2026-03-14");
    vänta_på_text(&tab, "Leverantörsskulder");
    klicka(
        &tab,
        "#leverantorsfakturaformular button[value='registrera']:not([disabled])",
    );
    vänta_på_text(&tab, "Registrerad och bokförd som V1.");
    vänta_på_text(&tab, "Leverantörsfaktura 8812");
    inga_sidfel(&tab);

    // Verifikatet länkar tillbaka till fakturan.
    klicka(&tab, ".dokument a[href*='/verifikat?v=']");
    vänta_på_text(&tab, "leverantörsfaktura 8812 från Kontorsvaror AB");
    vänta_på_text(&tab, "faktura-8812.pdf");
    inga_sidfel(&tab);
}

/// Specens flöde för kundfakturor (#135): användaren lägger upp en kund i kundregistret,
/// skapar en faktura i formuläret, fäller ut förhandsvisningen med P, granskar och utfärdar,
/// och den utfärdade fakturan visar sin arkiverade PDF. Betalningen är nästa röktest.
#[test]
fn kundfaktura_skapas_granskas_och_utfärdas() {
    if !stod::sajten_finns() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let port = ledig_port();
    let bas = format!("http://localhost:{port}");
    std::fs::write(
        dir.path().join(".env"),
        format!(
            "MAYA_PUBLIC_URL={bas}\nMAYA_LISTEN=127.0.0.1:{port}\nMAYA_BACKUP_DIR={backup}\n\
             MAYA_DATA_DIR={data}\n",
            backup = dir.path().display(),
            data = dir.path().join("data").display(),
        ),
    )
    .unwrap();
    let _server = Server::starta(dir.path(), port);
    let browser = Browser::new(
        LaunchOptions::default_builder()
            .sandbox(false)
            .idle_browser_timeout(Duration::from_secs(120))
            .build()
            .unwrap(),
    )
    .expect("röktestet kräver Chrome eller Chromium; ange sökvägen i CHROME");
    let tab = browser.new_tab().unwrap();
    tab.call_method(Page::AddScriptToEvaluateOnNewDocument {
        source: SAMLA_FEL.to_string(),
        world_name: None,
        include_command_line_api: None,
        run_immediately: None,
    })
    .unwrap();
    tab.call_method(WebAuthn::Enable {
        enable_ui: Some(false),
    })
    .unwrap();
    autentiserare(&tab);
    tab.navigate_to(&skapa_användare(dir.path(), &bas)).unwrap();
    vänta_på_shimmen(&tab);
    tab.wait_for_element("#namn")
        .unwrap()
        .type_into("Virtuell nyckel")
        .unwrap();
    klicka(&tab, "form[data-passkey='registrera'] button");
    vänta_på_text(&tab, "Lägg till en passkey till");

    // Företaget med fakturauppgifterna och kunden, med vanliga formulärposter.
    let företaget = posta(
        &tab,
        "/foretag",
        "orgnr=556036-0793&namn=Exempel+AB&start=2026-01-01&slut=2026-12-31\
         &bokforingsmetod=faktura&momsperiod=kvartal",
    );
    let id = företaget.rsplit('/').next().unwrap().to_string();
    posta(
        &tab,
        &format!("/foretag/{id}/installningar/uppgifter"),
        "namn=Exempel+AB&gatuadress=Exempelgatan+1&postnummer=12345&postort=Exempelstad\
         &epost=&telefon=&sate=Stockholm&momsregnr=SE556036079301&f_skatt=ja",
    );
    posta(
        &tab,
        &format!("/foretag/{id}/installningar/fakturauppgifter"),
        "bankgiro=50501055&ocr=ja",
    );

    // Kunden läggs upp i kundregistret, som nås från fliken Kundfakturor.
    tab.navigate_to(&format!("{bas}/foretag/{id}/kundfakturor"))
        .unwrap();
    klicka(&tab, ".verifikat-spalt a[href$='/kunder']");
    vänta_på_text(&tab, "Kundregistret");
    klicka(&tab, "a[href$='/kunder/ny']");
    vänta_på_text(&tab, "Lägg upp kunden");
    vänta_på_hydreringen(&tab);
    fyll(&tab, "#namn", "Kund AB");
    fyll(&tab, "#adress", "Storgatan 1\n111 22 Stockholm");
    fyll(&tab, "#betalningsvillkor", "30");
    klicka(&tab, "form.formular button[type='submit']");
    vänta_på_text(&tab, "Kunden lades upp och loggades.");
    let kund = tab.get_url();
    let k = kund
        .split("k=")
        .nth(1)
        .unwrap()
        .split('&')
        .next()
        .unwrap()
        .to_string();

    // Formuläret: kund, datum och en rad. Spärrarna räknas medan användaren skriver.
    tab.navigate_to(&format!("{bas}/foretag/{id}/kundfakturor/ny"))
        .unwrap();
    vänta_på_hydreringen(&tab);
    vänta_på_text(&tab, "Granska och utfärda · ");
    välj(&tab, "#kund", &k);
    fyll(&tab, "#fakturadatum", "2026-03-10");
    fyll(&tab, "input[name='beskrivning_0']", "Konsulttimmar");
    fyll(&tab, "input[name='antal_0']", "10");
    fyll(&tab, "input[name='a_pris_0']", "950");
    vänta_på_text(&tab, "11 875,00");
    // Rutan "Bokförs som" med konteringen (rubriken visas i versaler).
    tab.wait_for_element(".kont").unwrap();
    vänta_på_text(&tab, "2 375,00");
    // P fäller ut förhandsvisningen, som visar fakturan medan den skrivs.
    tab.evaluate(
        "document.body.dispatchEvent(new KeyboardEvent('keydown', {key: 'p', bubbles: true}))",
        false,
    )
    .unwrap();
    tab.wait_for_element(".papper").unwrap();
    vänta_på_text(&tab, "Uppdateras medan du skriver.");
    let knapp = tab
        .evaluate(
            "document.querySelector(\"button[value='granska']\").textContent",
            false,
        )
        .unwrap()
        .value;
    assert_eq!(knapp, Some("Granska och utfärda".into()));
    klicka(&tab, "button[value='granska']");

    // Granska och utfärda: PDF:en, numret och konteringen, sedan Utfärda.
    vänta_på_text(&tab, "Så här ser PDF:en ut");
    tab.wait_for_element("iframe.granska-pdf").unwrap();
    klicka(&tab, "form[action$='/utfarda'] button");
    vänta_på_text(&tab, "Fakturan utfärdades");
    vänta_på_text(&tab, "Kundfaktura 1");
    vänta_på_text(&tab, "SHA-256");
    vänta_på_text(&tab, "Kundfaktura_1.pdf");
    let pdf = hämta(&tab, ".arkiverad-pdf a");
    assert_eq!(
        (pdf.status, pdf.typ.as_str(), &pdf.text[..5]),
        (200, "application/pdf", "%PDF-")
    );
    inga_sidfel(&tab);
}

/// Specens flöde för betalningar (#139): en utfärdad faktura får en delbetalning i panelen
/// Registrera betalning, som visar resten och verifikatet medan användaren skriver, och
/// dokumentet visar betalningen och det som återstår.
#[test]
fn kundfaktura_betalas_i_dokumentet() {
    if !stod::sajten_finns() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let port = ledig_port();
    let bas = format!("http://localhost:{port}");
    std::fs::write(
        dir.path().join(".env"),
        format!(
            "MAYA_PUBLIC_URL={bas}\nMAYA_LISTEN=127.0.0.1:{port}\nMAYA_BACKUP_DIR={backup}\n\
             MAYA_DATA_DIR={data}\n",
            backup = dir.path().display(),
            data = dir.path().join("data").display(),
        ),
    )
    .unwrap();
    let _server = Server::starta(dir.path(), port);
    let browser = Browser::new(
        LaunchOptions::default_builder()
            .sandbox(false)
            .idle_browser_timeout(Duration::from_secs(120))
            .build()
            .unwrap(),
    )
    .expect("röktestet kräver Chrome eller Chromium; ange sökvägen i CHROME");
    let tab = browser.new_tab().unwrap();
    tab.call_method(Page::AddScriptToEvaluateOnNewDocument {
        source: SAMLA_FEL.to_string(),
        world_name: None,
        include_command_line_api: None,
        run_immediately: None,
    })
    .unwrap();
    tab.call_method(WebAuthn::Enable {
        enable_ui: Some(false),
    })
    .unwrap();
    autentiserare(&tab);
    tab.navigate_to(&skapa_användare(dir.path(), &bas)).unwrap();
    vänta_på_shimmen(&tab);
    tab.wait_for_element("#namn")
        .unwrap()
        .type_into("Virtuell nyckel")
        .unwrap();
    klicka(&tab, "form[data-passkey='registrera'] button");
    vänta_på_text(&tab, "Lägg till en passkey till");

    // Företaget, kunden och en utfärdad faktura på 11 875,00 kr, med vanliga formulärposter.
    let företaget = posta(
        &tab,
        "/foretag",
        "orgnr=556036-0793&namn=Exempel+AB&start=2026-01-01&slut=2026-12-31\
         &bokforingsmetod=faktura&momsperiod=kvartal",
    );
    let id = företaget.rsplit('/').next().unwrap().to_string();
    posta(
        &tab,
        &format!("/foretag/{id}/installningar/uppgifter"),
        "namn=Exempel+AB&gatuadress=Exempelgatan+1&postnummer=12345&postort=Exempelstad\
         &epost=&telefon=&sate=Stockholm&momsregnr=SE556036079301&f_skatt=ja",
    );
    posta(
        &tab,
        &format!("/foretag/{id}/installningar/fakturauppgifter"),
        "bankgiro=50501055",
    );
    let kund = posta(
        &tab,
        &format!("/foretag/{id}/kunder"),
        "namn=Kund+AB&identitet=&adress=Storgatan+1%0A111+22+Stockholm&epost=\
         &momsregnr=&betalningsvillkor=30&omvand_skattskyldighet=ingen",
    );
    let k = kund
        .split("k=")
        .nth(1)
        .unwrap()
        .split('&')
        .next()
        .unwrap()
        .to_string();
    let granska = posta(
        &tab,
        &format!("/foretag/{id}/kundfakturor"),
        &format!(
            "kund={k}&fakturadatum=2026-03-10&leveransdatum=&forfallodag=&er_referens=\
             &startnummer=&beskrivning_0=Konsulttimmar&antal_0=10&enhet_0=tim&a_pris_0=950\
             &rabatt_0=&momssats_0=25&konto_0=3001&handling=granska"
        ),
    );
    let f = granska
        .split("f=")
        .nth(1)
        .unwrap()
        .split('&')
        .next()
        .unwrap()
        .to_string();
    posta(&tab, &format!("/foretag/{id}/kundfakturor/{f}/utfarda"), "");

    // Dokumentet: förfallen och obetald. Registrera betalning öppnar panelen.
    tab.navigate_to(&format!("{bas}/foretag/{id}/kundfakturor?f={f}"))
        .unwrap();
    vänta_på_hydreringen(&tab);
    vänta_på_text(&tab, "Förfallen");
    vänta_på_text(&tab, "Inga betalningar än.");
    klicka(&tab, "a[href$='panel=betalning']");
    vänta_på_text(&tab, "Återstår 11 875,00 kr");
    vänta_på_hydreringen(&tab);
    // En delbetalning: resten och verifikatet visas medan användaren skriver.
    fyll(&tab, "#betaldag", "2026-04-01");
    fyll(&tab, "#belopp", "5000");
    vänta_på_text(&tab, "Delbetalning, 6 875,00 kr återstår.");
    vänta_på_text(&tab, "5 000,00");
    // En överbetalning spärras vid fältet.
    fyll(&tab, "#belopp", "20000");
    tab.wait_for_element("p[data-kod='overbetalning']").unwrap();
    fyll(&tab, "#belopp", "5000");
    klicka(&tab, ".betalningspanel button[type='submit']");
    vänta_på_text(&tab, "Betalningen registrerades");
    vänta_på_text(&tab, "Rätta betalning…");
    vänta_på_text(&tab, "6 875,00 kvar");
    inga_sidfel(&tab);
}

/// Specens röktest för kreditfakturor (#140): Kreditfaktura… på en utfärdad faktura krediterar
/// ett antal på raden, utkastet visar hänvisningen och verifikatet, och kreditfakturan utfärdas
/// med nästa nummer och minskar det som återstår på originalet.
#[test]
fn kundfaktura_krediteras() {
    if !stod::sajten_finns() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let port = ledig_port();
    let bas = format!("http://localhost:{port}");
    std::fs::write(
        dir.path().join(".env"),
        format!(
            "MAYA_PUBLIC_URL={bas}\nMAYA_LISTEN=127.0.0.1:{port}\nMAYA_BACKUP_DIR={backup}\n\
             MAYA_DATA_DIR={data}\n",
            backup = dir.path().display(),
            data = dir.path().join("data").display(),
        ),
    )
    .unwrap();
    let _server = Server::starta(dir.path(), port);
    let browser = Browser::new(
        LaunchOptions::default_builder()
            .sandbox(false)
            .idle_browser_timeout(Duration::from_secs(120))
            .build()
            .unwrap(),
    )
    .expect("röktestet kräver Chrome eller Chromium; ange sökvägen i CHROME");
    let tab = browser.new_tab().unwrap();
    tab.call_method(Page::AddScriptToEvaluateOnNewDocument {
        source: SAMLA_FEL.to_string(),
        world_name: None,
        include_command_line_api: None,
        run_immediately: None,
    })
    .unwrap();
    tab.call_method(WebAuthn::Enable {
        enable_ui: Some(false),
    })
    .unwrap();
    autentiserare(&tab);
    tab.navigate_to(&skapa_användare(dir.path(), &bas)).unwrap();
    vänta_på_shimmen(&tab);
    tab.wait_for_element("#namn")
        .unwrap()
        .type_into("Virtuell nyckel")
        .unwrap();
    klicka(&tab, "form[data-passkey='registrera'] button");
    vänta_på_text(&tab, "Lägg till en passkey till");

    // Företaget, kunden och en utfärdad faktura 1 på 11 875,00 kr.
    let företaget = posta(
        &tab,
        "/foretag",
        "orgnr=556036-0793&namn=Exempel+AB&start=2026-01-01&slut=2026-12-31\
         &bokforingsmetod=faktura&momsperiod=kvartal",
    );
    let id = företaget.rsplit('/').next().unwrap().to_string();
    posta(
        &tab,
        &format!("/foretag/{id}/installningar/uppgifter"),
        "namn=Exempel+AB&gatuadress=Exempelgatan+1&postnummer=12345&postort=Exempelstad\
         &epost=&telefon=&sate=Stockholm&momsregnr=SE556036079301&f_skatt=ja",
    );
    posta(
        &tab,
        &format!("/foretag/{id}/installningar/fakturauppgifter"),
        "bankgiro=50501055",
    );
    let kund = posta(
        &tab,
        &format!("/foretag/{id}/kunder"),
        "namn=Kund+AB&identitet=&adress=Storgatan+1%0A111+22+Stockholm&epost=\
         &momsregnr=&betalningsvillkor=30&omvand_skattskyldighet=ingen",
    );
    let k = kund
        .split("k=")
        .nth(1)
        .unwrap()
        .split('&')
        .next()
        .unwrap()
        .to_string();
    let granska = posta(
        &tab,
        &format!("/foretag/{id}/kundfakturor"),
        &format!(
            "kund={k}&fakturadatum=2026-03-10&leveransdatum=&forfallodag=&er_referens=\
             &startnummer=&beskrivning_0=Konsulttimmar&antal_0=10&enhet_0=tim&a_pris_0=950\
             &rabatt_0=&momssats_0=25&konto_0=3001&handling=granska"
        ),
    );
    let f = granska
        .split("f=")
        .nth(1)
        .unwrap()
        .split('&')
        .next()
        .unwrap()
        .to_string();
    posta(&tab, &format!("/foretag/{id}/kundfakturor/{f}/utfarda"), "");

    // Kreditfaktura… på dokumentet: 4 av 10 timmar.
    tab.navigate_to(&format!("{bas}/foretag/{id}/kundfakturor?f={f}"))
        .unwrap();
    vänta_på_hydreringen(&tab);
    klicka(&tab, "a[href$='panel=kreditera']");
    vänta_på_text(&tab, "Kvar att kreditera 11 875,00 kr");
    vänta_på_hydreringen(&tab);
    klicka(&tab, ".panel.kreditera input[name='hela'][value='']");
    fyll(&tab, ".panel.kreditera input[name='antal_0']", "4");
    klicka(&tab, ".panel.kreditera button[type='submit']");
    // Utkastet: hänvisningen, raden med negativt antal och verifikatet som speglar originalet.
    vänta_på_text(&tab, "Kreditfakturans utkast skapades");
    vänta_på_text(&tab, "Krediterar faktura 1 daterad 2026-03-10");
    vänta_på_hydreringen(&tab);
    vänta_på_text(&tab, "Att få tillbaka");
    vänta_på_text(&tab, "3 800,00");
    klicka(&tab, "button[value='granska']");
    vänta_på_text(&tab, "När du utfärdar får fakturan nummer");
    klicka(&tab, ".granska button[type='submit']");
    vänta_på_text(&tab, "Fakturan utfärdades");
    vänta_på_text(&tab, "Kreditfaktura 2");
    // Originalet har det krediterade avdraget och länken till kreditfakturan.
    tab.navigate_to(&format!("{bas}/foretag/{id}/kundfakturor?f={f}"))
        .unwrap();
    vänta_på_text(&tab, "Krediteras av");
    vänta_på_text(&tab, "7 125,00 kvar");
    inga_sidfel(&tab);
}

/// Specens flöde för leverantörsfakturor (#147): en registrerad faktura betalas i sitt dokument
/// med betaluppgifterna synliga, och delbetalningen och verifikatet visas medan användaren skriver.
#[test]
fn leverantorsfaktura_betalas_i_dokumentet() {
    if !stod::sajten_finns() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let port = ledig_port();
    let bas = format!("http://localhost:{port}");
    std::fs::write(
        dir.path().join(".env"),
        format!(
            "MAYA_PUBLIC_URL={bas}\nMAYA_LISTEN=127.0.0.1:{port}\nMAYA_BACKUP_DIR={backup}\n\
             MAYA_DATA_DIR={data}\n",
            backup = dir.path().display(),
            data = dir.path().join("data").display(),
        ),
    )
    .unwrap();
    let _server = Server::starta(dir.path(), port);
    let browser = Browser::new(
        LaunchOptions::default_builder()
            .sandbox(false)
            .idle_browser_timeout(Duration::from_secs(120))
            .build()
            .unwrap(),
    )
    .expect("röktestet kräver Chrome eller Chromium; ange sökvägen i CHROME");
    let tab = browser.new_tab().unwrap();
    tab.call_method(Page::AddScriptToEvaluateOnNewDocument {
        source: SAMLA_FEL.to_string(),
        world_name: None,
        include_command_line_api: None,
        run_immediately: None,
    })
    .unwrap();
    tab.call_method(WebAuthn::Enable {
        enable_ui: Some(false),
    })
    .unwrap();
    autentiserare(&tab);
    tab.navigate_to(&skapa_användare(dir.path(), &bas)).unwrap();
    vänta_på_shimmen(&tab);
    tab.wait_for_element("#namn")
        .unwrap()
        .type_into("Virtuell nyckel")
        .unwrap();
    klicka(&tab, "form[data-passkey='registrera'] button");
    vänta_på_text(&tab, "Lägg till en passkey till");

    // Företaget, leverantören med bankgiro och originalet, uppladdat i fliken.
    let företaget = posta(
        &tab,
        "/foretag",
        "orgnr=556036-0793&namn=Exempel+AB&start=2026-01-01&slut=2026-12-31\
         &bokforingsmetod=faktura&momsperiod=kvartal",
    );
    let sökväg = företaget.strip_prefix(&bas).unwrap().to_string();
    posta(
        &tab,
        &format!("{sökväg}/leverantorer"),
        "namn=Kontorsvaror+AB&land=SE&bankgiro=5050-1055&standardkonto=5410",
    );
    let original = dir.path().join("faktura-8812.pdf");
    std::fs::write(&original, KVITTO).unwrap();
    tab.navigate_to(&format!("{företaget}/leverantorsfakturor/ny"))
        .unwrap();
    vänta_på_hydreringen(&tab);
    tab.wait_for_element("form.uppladdning input[type='file']")
        .unwrap()
        .set_input_files(&[original.to_str().unwrap()])
        .unwrap();
    klicka(&tab, "form.uppladdning button[type='submit']");
    vänta_på_text(&tab, "Ett utkast skapades med originalet kopplat.");
    let utkast = tab.get_url();
    let f = utkast
        .split("f=")
        .nth(1)
        .unwrap()
        .split('&')
        .next()
        .unwrap()
        .to_string();
    posta(
        &tab,
        &format!("{sökväg}/leverantorsfakturor/{f}"),
        "handling=registrera&leverantor=1&fakturanummer=8812&fakturadatum=2026-03-14\
         &forfallodag=2026-04-13&referens=4455667788&totalbelopp=1250&originalvaluta=\
         &konto_0=&netto_0=1000&momssats_0=25&moms_0=",
    );

    // Dokumentet: förfallen, med bankgirot och OCR-numret. Registrera betalning öppnar panelen.
    tab.navigate_to(&format!("{företaget}/leverantorsfakturor?f={f}"))
        .unwrap();
    vänta_på_hydreringen(&tab);
    vänta_på_text(&tab, "Förfallen");
    vänta_på_text(&tab, "5050-1055");
    vänta_på_text(&tab, "4455667788");
    klicka(&tab, "a[href$='panel=betalning']");
    vänta_på_text(&tab, "Återstår 1 250,00 kr");
    vänta_på_hydreringen(&tab);
    fyll(&tab, "#betaldag", "2026-04-10");
    fyll(&tab, "#belopp", "500");
    vänta_på_text(&tab, "Delbetalning, 750,00 kr återstår.");
    // Rutan "Bokförs som" med verifikatet (rubriken visas i versaler).
    tab.wait_for_element(".betalningspanel .kont").unwrap();
    vänta_på_text(&tab, "VERIFIKAT · 2026-04-10");
    klicka(&tab, ".betalningspanel button[type='submit']");
    vänta_på_text(&tab, "Betalningen registrerades");
    vänta_på_text(&tab, "750,00 kvar");
    vänta_på_text(&tab, "Rätta betalning…");
    inga_sidfel(&tab);
}

/// #152 Att göra med fakturorna (specens röktest): en uppladdad leverantörsfaktura registreras
/// från Att göra, står sedan som förfallen i tidslinjen med kopieringschipsen och betalas i sitt
/// kort, och en förfallen kundfaktura betalas i sitt kort utan att fakturan öppnas.
#[test]
fn fakturor_registreras_och_betalas_från_att_göra() {
    if !stod::sajten_finns() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let port = ledig_port();
    let bas = format!("http://localhost:{port}");
    std::fs::write(
        dir.path().join(".env"),
        format!(
            "MAYA_PUBLIC_URL={bas}\nMAYA_LISTEN=127.0.0.1:{port}\nMAYA_BACKUP_DIR={backup}\n\
             MAYA_DATA_DIR={data}\n",
            backup = dir.path().display(),
            data = dir.path().join("data").display(),
        ),
    )
    .unwrap();
    let _server = Server::starta(dir.path(), port);
    let browser = Browser::new(
        LaunchOptions::default_builder()
            .sandbox(false)
            .idle_browser_timeout(Duration::from_secs(120))
            .build()
            .unwrap(),
    )
    .expect("röktestet kräver Chrome eller Chromium; ange sökvägen i CHROME");
    let tab = browser.new_tab().unwrap();
    tab.call_method(Page::AddScriptToEvaluateOnNewDocument {
        source: SAMLA_FEL.to_string(),
        world_name: None,
        include_command_line_api: None,
        run_immediately: None,
    })
    .unwrap();
    tab.call_method(WebAuthn::Enable {
        enable_ui: Some(false),
    })
    .unwrap();
    autentiserare(&tab);
    tab.navigate_to(&skapa_användare(dir.path(), &bas)).unwrap();
    vänta_på_shimmen(&tab);
    tab.wait_for_element("#namn")
        .unwrap()
        .type_into("Virtuell nyckel")
        .unwrap();
    klicka(&tab, "form[data-passkey='registrera'] button");
    vänta_på_text(&tab, "Lägg till en passkey till");

    // Företaget med fakturauppgifterna, leverantören och kunden, och en utfärdad kundfaktura
    // på 11 875,00 kr som förföll 2026-04-09, med vanliga formulärposter.
    let företaget = posta(
        &tab,
        "/foretag",
        "orgnr=556036-0793&namn=Exempel+AB&start=2026-01-01&slut=2026-12-31\
         &bokforingsmetod=faktura&momsperiod=kvartal",
    );
    let id = företaget.rsplit('/').next().unwrap().to_string();
    posta(
        &tab,
        &format!("/foretag/{id}/installningar/uppgifter"),
        "namn=Exempel+AB&gatuadress=Exempelgatan+1&postnummer=12345&postort=Exempelstad\
         &epost=&telefon=&sate=Stockholm&momsregnr=SE556036079301&f_skatt=ja",
    );
    posta(
        &tab,
        &format!("/foretag/{id}/installningar/fakturauppgifter"),
        "bankgiro=50501055",
    );
    posta(
        &tab,
        &format!("/foretag/{id}/leverantorer"),
        "namn=Kontorsvaror+AB&land=SE&bankgiro=5050-1055&standardkonto=5410",
    );
    let kund = posta(
        &tab,
        &format!("/foretag/{id}/kunder"),
        "namn=Kund+AB&identitet=&adress=Storgatan+1%0A111+22+Stockholm&epost=\
         &momsregnr=&betalningsvillkor=30&omvand_skattskyldighet=ingen",
    );
    let k = kund
        .split("k=")
        .nth(1)
        .unwrap()
        .split('&')
        .next()
        .unwrap()
        .to_string();
    let granska = posta(
        &tab,
        &format!("/foretag/{id}/kundfakturor"),
        &format!(
            "kund={k}&fakturadatum=2026-03-10&leveransdatum=&forfallodag=&er_referens=\
             &startnummer=&beskrivning_0=Konsulttimmar&antal_0=10&enhet_0=tim&a_pris_0=950\
             &rabatt_0=&momssats_0=25&konto_0=3001&handling=granska"
        ),
    );
    let f = granska
        .split("f=")
        .nth(1)
        .unwrap()
        .split('&')
        .next()
        .unwrap()
        .to_string();
    posta(&tab, &format!("/foretag/{id}/kundfakturor/{f}/utfarda"), "");

    // Leverantörsfakturan laddas upp och registreras från Att göra.
    let original = dir.path().join("faktura-8812.pdf");
    std::fs::write(&original, KVITTO).unwrap();
    tab.navigate_to(&format!("{företaget}/underlag")).unwrap();
    vänta_på_hydreringen(&tab);
    tab.wait_for_element("form.uppladdning input[type='file']")
        .unwrap()
        .set_input_files(&[original.to_str().unwrap()])
        .unwrap();
    klicka(&tab, "form.uppladdning button[type='submit']");
    vänta_på_text(&tab, "faktura-8812.pdf");
    klicka(&tab, "nav.flikar a:first-child");
    vänta_på_text(&tab, "okopplat");
    klicka(&tab, ".acard[id^='underlag-'] a.ah");
    klicka(&tab, "form[action$='/leverantorsfaktura'] button");
    vänta_på_text(&tab, "Ett utkast skapades med originalet kopplat.");
    vänta_på_hydreringen(&tab);
    välj(&tab, "#leverantorsfakturaformular #leverantor", "1");
    fyll(&tab, "#leverantorsfakturaformular #fakturanummer", "8812");
    fyll(
        &tab,
        "#leverantorsfakturaformular #fakturadatum",
        "2026-03-14",
    );
    fyll(
        &tab,
        "#leverantorsfakturaformular #forfallodag",
        "2026-04-13",
    );
    fyll(&tab, "#leverantorsfakturaformular #referens", "88120044711");
    fyll(&tab, "#leverantorsfakturaformular #totalbelopp", "1 250");
    fyll(&tab, "#leverantorsfakturaformular [name='netto_0']", "1000");
    vänta_på_text(&tab, "Bokförs som V2 · 2026-03-14");
    klicka(
        &tab,
        "#leverantorsfakturaformular button[value='registrera']:not([disabled])",
    );
    vänta_på_text(&tab, "Registrerad och bokförd som V2.");

    // Att göra: båda fakturorna under Försenat och nyckeltalen för reskontrorna.
    klicka(&tab, "nav.flikar a:first-child");
    vänta_på_text(&tab, "Försenat");
    vänta_på_text(&tab, "Kontorsvaror AB · 8812");
    vänta_på_text(&tab, "Kundfordringar");
    vänta_på_text(&tab, "varav förfallet 1 250,00 kr · 2440 ur bokföringen");
    // Leverantörsfakturans kort fälls ut med kopieringschipsen.
    klicka(&tab, ".acard[id^='leverantorsfaktura-'] a.ah");
    tab.wait_for_element(".acard.open .chips button.copy")
        .unwrap();
    vänta_på_text(&tab, "88120044711");
    inga_sidfel(&tab);
    // Betalningspanelen fälls ut i kortet och betalningen bokförs utan att fakturan öppnas.
    klicka(
        &tab,
        ".acard.open[id^='leverantorsfaktura-'] a[href$='panel=betalning']",
    );
    vänta_på_text(&tab, "Återstår 1 250,00 kr");
    vänta_på_hydreringen(&tab);
    fyll(&tab, "#betaldag", "2026-04-13");
    klicka(&tab, ".betalningspanel button[type='submit']");
    vänta_på_text(&tab, "Betalningen är bokförd som V3.");
    vänta_på_text_borta(&tab, "Kontorsvaror AB · 8812");
    inga_sidfel(&tab);

    // Kundfakturans kort: betalningspanelen fälls ut i kortet och betalningen bokförs.
    klicka(&tab, &format!("#kundfaktura-{f} a.ah"));
    vänta_på_text(&tab, "Kontakta kunden själv.");
    klicka(
        &tab,
        &format!("#kundfaktura-{f} a[href$='panel=betalning']"),
    );
    vänta_på_text(&tab, "Återstår 11 875,00 kr");
    vänta_på_hydreringen(&tab);
    fyll(&tab, "#betaldag", "2026-04-20");
    klicka(&tab, ".betalningspanel button[type='submit']");
    vänta_på_text(&tab, "Betalningen är bokförd som V4.");
    // Kortet lämnar Försenat, men fakturan står kvar under Följ upp (#153) eftersom den inte
    // är skickad.
    vänta_på_element_borta(&tab, &format!("#kundfaktura-{f}"));
    tab.wait_for_element(&format!("#utskick-{f}")).unwrap();
    inga_sidfel(&tab);

    // Betalningen är bokförd som från fakturan.
    tab.navigate_to(&format!("{bas}/foretag/{id}/kundfakturor?f={f}"))
        .unwrap();
    vänta_på_text(&tab, "Betald");
    inga_sidfel(&tab);
}

/// #148 Kreditfaktura från leverantör och Rätta avskrift: kreditfakturans rader fylls ur
/// originalet i webbläsaren och registreras, och originalets avskrift rättas så att fakturan
/// låses upp.
#[test]
fn leverantorskredit_konteras_som_originalet_och_avskriften_rättas() {
    if !stod::sajten_finns() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let port = ledig_port();
    let bas = format!("http://localhost:{port}");
    std::fs::write(
        dir.path().join(".env"),
        format!(
            "MAYA_PUBLIC_URL={bas}\nMAYA_LISTEN=127.0.0.1:{port}\nMAYA_BACKUP_DIR={backup}\n\
             MAYA_DATA_DIR={data}\n",
            backup = dir.path().display(),
            data = dir.path().join("data").display(),
        ),
    )
    .unwrap();
    let _server = Server::starta(dir.path(), port);
    let browser = Browser::new(
        LaunchOptions::default_builder()
            .sandbox(false)
            .idle_browser_timeout(Duration::from_secs(120))
            .build()
            .unwrap(),
    )
    .expect("röktestet kräver Chrome eller Chromium; ange sökvägen i CHROME");
    let tab = browser.new_tab().unwrap();
    tab.call_method(Page::AddScriptToEvaluateOnNewDocument {
        source: SAMLA_FEL.to_string(),
        world_name: None,
        include_command_line_api: None,
        run_immediately: None,
    })
    .unwrap();
    tab.call_method(WebAuthn::Enable {
        enable_ui: Some(false),
    })
    .unwrap();
    autentiserare(&tab);
    tab.navigate_to(&skapa_användare(dir.path(), &bas)).unwrap();
    vänta_på_shimmen(&tab);
    tab.wait_for_element("#namn")
        .unwrap()
        .type_into("Virtuell nyckel")
        .unwrap();
    klicka(&tab, "form[data-passkey='registrera'] button");
    vänta_på_text(&tab, "Lägg till en passkey till");

    // Företaget, leverantören och originalet 8812 på 1 250 kr, registrerat.
    let företaget = posta(
        &tab,
        "/foretag",
        "orgnr=556036-0793&namn=Exempel+AB&start=2026-01-01&slut=2026-12-31\
         &bokforingsmetod=faktura&momsperiod=kvartal",
    );
    let sökväg = företaget.strip_prefix(&bas).unwrap().to_string();
    posta(
        &tab,
        &format!("{sökväg}/leverantorer"),
        "namn=Kontorsvaror+AB&land=SE&bankgiro=5050-1055&standardkonto=5410",
    );
    let ladda_upp = |namn: &str| {
        let fil = dir.path().join(namn);
        std::fs::write(&fil, [KVITTO, namn.as_bytes()].concat()).unwrap();
        tab.navigate_to(&format!("{företaget}/leverantorsfakturor/ny"))
            .unwrap();
        vänta_på_hydreringen(&tab);
        tab.wait_for_element("form.uppladdning input[type='file']")
            .unwrap()
            .set_input_files(&[fil.to_str().unwrap()])
            .unwrap();
        klicka(&tab, "form.uppladdning button[type='submit']");
        vänta_på_text(&tab, "Ett utkast skapades med originalet kopplat.");
        tab.get_url()
            .split("f=")
            .nth(1)
            .unwrap()
            .split('&')
            .next()
            .unwrap()
            .to_string()
    };
    let f = ladda_upp("faktura-8812.pdf");
    posta(
        &tab,
        &format!("{sökväg}/leverantorsfakturor/{f}"),
        "handling=registrera&leverantor=1&fakturanummer=8812&fakturadatum=2026-03-14\
         &forfallodag=2026-04-13&referens=4455667788&totalbelopp=1250&originalvaluta=\
         &konto_0=&netto_0=1000&momssats_0=25&moms_0=",
    );

    // Kreditfakturan: leverantören, hänvisningen och halva totalen med minus; Kontera som
    // originalet fyller raden med netto och moms.
    let k = ladda_upp("kredit-k17.pdf");
    vänta_på_hydreringen(&tab);
    välj(&tab, "#leverantor", "1");
    tab.wait_for_element("#krediterar").unwrap();
    välj(&tab, "#krediterar", &f);
    fyll(&tab, "#fakturanummer", "K-17");
    fyll(&tab, "#fakturadatum", "2026-03-20");
    fyll(&tab, "#forfallodag", "2026-04-19");
    fyll(&tab, "#totalbelopp", "-625");
    vänta_på_text(&tab, "Kreditfaktura");
    klicka(&tab, ".hanvisning-val button");
    vänta_på_text(&tab, "Summerar till totalen");
    let värde = |selektor: &str| {
        tab.evaluate(
            &format!("document.querySelector({selektor:?}).value"),
            false,
        )
        .unwrap()
        .value
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
    };
    assert_eq!(värde("input[name='netto_0']"), "-500");
    assert_eq!(värde("input[name='moms_0']"), "-125");
    klicka(&tab, "button[value='registrera']");
    vänta_på_text(&tab, "Registrerad och bokförd som");
    vänta_på_text(&tab, "Kreditfaktura K-17");
    vänta_på_text(&tab, "Krediterar");
    assert!(
        tab.get_url().contains(&format!("f={k}")),
        "{}",
        tab.get_url()
    );

    // Originalet: 625 kr kvar. Rätta avskrift… låser upp det.
    tab.navigate_to(&format!("{företaget}/leverantorsfakturor?f={f}"))
        .unwrap();
    vänta_på_hydreringen(&tab);
    vänta_på_text(&tab, "625,00 kvar");
    vänta_på_text(&tab, "Krediteras av");
    klicka(&tab, "a[href$='panel=ratta-avskrift']");
    vänta_på_text(&tab, "Bokförs som rättelse");
    klicka(&tab, ".avskriftspanel button[type='submit']");
    vänta_på_text(&tab, "Avskriften rättades");
    vänta_på_text(&tab, "Upplåst");
    inga_sidfel(&tab);
}

/// Väljer ett värde i en `<select>` som användaren skulle.
fn välj(tab: &Tab, selektor: &str, värde: &str) {
    tab.wait_for_element(selektor)
        .unwrap_or_else(|e| panic!("{selektor} finns inte på {}: {e}", tab.get_url()));
    tab.evaluate(
        &format!(
            "(() => {{ const e = document.querySelector({selektor:?}); e.value = {värde:?}; \
             e.dispatchEvent(new Event('change', {{bubbles: true}})); }})()"
        ),
        false,
    )
    .unwrap();
}

/// Postar ett formulär från sidan (samma origin, sessionscookien följer med) och ger adressen
/// som svaret ledde till.
fn posta(tab: &Tab, sökväg: &str, kropp: &str) -> String {
    let skript = format!(
        "fetch({sökväg:?}, {{method: 'POST', \
         headers: {{'content-type': 'application/x-www-form-urlencoded'}}, \
         body: {kropp:?}}}).then(r => r.ok ? r.url : Promise.reject(r.status + ' ' + r.url))"
    );
    tab.evaluate(&skript, true)
        .unwrap()
        .value
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_else(|| panic!("{sökväg} gav inget svar"))
}

/// Ett litet PDF-dokument som underlag.
const KVITTO: &[u8] = b"%PDF-1.4\n1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n\
2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj\n\
3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 200 200]>>endobj\n\
trailer<</Root 1 0 R>>\n%%EOF\n";

/// Klickar på elementet med ett DOM-klick, som når elementet även om något annat ligger över
/// dess mittpunkt (ett musklick på koordinater kan träffa grannen).
fn klicka(tab: &Tab, selektor: &str) {
    tab.wait_for_element(selektor)
        .unwrap_or_else(|e| panic!("{selektor} finns inte på {}: {e}", tab.get_url()));
    tab.evaluate(
        &format!("document.querySelector({selektor:?}).click()"),
        false,
    )
    .unwrap();
}

/// Skriver ett värde i ett fält som användaren skulle, så att sidans kontroller ser det.
fn fyll(tab: &Tab, selektor: &str, värde: &str) {
    tab.wait_for_element(selektor)
        .unwrap_or_else(|e| panic!("{selektor} finns inte på {}: {e}", tab.get_url()));
    tab.evaluate(
        &format!(
            "(() => {{ const e = document.querySelector({selektor:?}); e.value = {värde:?}; \
             e.dispatchEvent(new Event('input', {{bubbles: true}})); }})()"
        ),
        false,
    )
    .unwrap();
}

struct Svar {
    status: u16,
    typ: String,
    text: String,
}

/// Hämtar länkens adress från sidan, med sessionscookien, som en nedladdning gör.
fn hämta(tab: &Tab, selektor: &str) -> Svar {
    tab.wait_for_element(selektor)
        .unwrap_or_else(|e| panic!("{selektor} finns inte på {}: {e}", tab.get_url()));
    let svar = tab
        .evaluate(
            &format!(
                "(async () => {{ const r = await fetch(document.querySelector({selektor:?}).href); \
                 return [r.status, r.headers.get('content-type'), await r.text()].join('\\n'); \
                 }})()"
            ),
            true,
        )
        .unwrap()
        .value
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_else(|| panic!("{selektor} gav inget svar"));
    let mut delar = svar.splitn(3, '\n');
    let mut del = || delar.next().unwrap_or_default().to_string();
    Svar {
        status: del().parse().unwrap(),
        typ: del(),
        text: del(),
    }
}

/// Väntar tills `hydrate()` har kört klart (den sätter `<html data-hydrerad>`).
fn vänta_på_hydreringen(tab: &Tab) {
    tab.wait_for_element("html[data-hydrerad]").unwrap();
}

fn skapa_användare(dir: &Path, bas: &str) -> String {
    let output = Command::new(MAYA)
        .current_dir(dir)
        .env_clear()
        .args(["user", "create", "anna@example.se"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8(output.stdout).unwrap();
    stdout
        .split_whitespace()
        .find(|ord| ord.starts_with(&format!("{bas}/registrera/")))
        .unwrap_or_else(|| panic!("ingen länk i {stdout}"))
        .to_string()
}

/// Klickar på `väljare`, en knapp som leder till en ny sida (ett formulär, eller JS-shimmen som
/// byter sida när passkeyn är klar), och väntar tills den nya sidan har laddat klart med sina
/// skript. Den gamla sidan märks först, så att det testet sedan letar efter inte kan hittas på
/// den, eller försvinna med den mitt i ett klick.
fn klicka_till_ny_sida(tab: &Tab, väljare: &str) {
    tab.evaluate("document.documentElement.dataset.foraSida = ''", false)
        .unwrap();
    tab.wait_for_element(väljare).unwrap().click().unwrap();
    let start = Instant::now();
    loop {
        let ny = tab
            .evaluate(
                "document.readyState === 'complete' \
                 && !('foraSida' in document.documentElement.dataset)",
                false,
            )
            .ok()
            .and_then(|svar| svar.value);
        if ny == Some(true.into()) {
            return;
        }
        assert!(
            start.elapsed() < VÄNTA,
            "klicket på {väljare} ledde aldrig till en ny sida från {}",
            tab.get_url()
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Väntar tills sidan har laddat och JS-shimmen tar emot formulären.
fn vänta_på_shimmen(tab: &Tab) {
    tab.wait_for_element("html[data-passkey-shim]").unwrap();
}

fn vänta_på_text(tab: &Tab, text: &str) {
    let start = Instant::now();
    loop {
        let innehåll = tab
            .evaluate("document.body.innerText", false)
            .ok()
            .and_then(|svar| svar.value)
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_default();
        if innehåll.contains(text) {
            return;
        }
        assert!(
            start.elapsed() < VÄNTA,
            "sidan {} visade aldrig \"{text}\":\n{innehåll}",
            tab.get_url()
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

struct Server(Child);

impl Server {
    fn starta(dir: &Path, port: u16) -> Self {
        let server = Server(
            Command::new(MAYA)
                .current_dir(dir)
                .env_clear()
                .arg("serve")
                .stdout(Stdio::null())
                .spawn()
                .unwrap(),
        );
        let start = Instant::now();
        loop {
            if let Ok(mut stream) = TcpStream::connect(("127.0.0.1", port)) {
                write!(
                    stream,
                    "GET /healthz HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"
                )
                .unwrap();
                let mut svar = String::new();
                stream.read_to_string(&mut svar).unwrap();
                assert!(svar.starts_with("HTTP/1.1 200"), "{svar}");
                return server;
            }
            assert!(start.elapsed() < VÄNTA, "servern startade inte");
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
    }
}

fn ledig_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// Lägger till en virtuell autentiserare med användarverifiering och ger dess id.
fn autentiserare(tab: &Tab) -> String {
    tab.call_method(WebAuthn::AddVirtualAuthenticator {
        options: WebAuthn::VirtualAuthenticatorOptions {
            protocol: WebAuthn::AuthenticatorProtocol::Ctap2,
            ctap_2_version: Some(WebAuthn::Ctap2Version::Ctap21),
            transport: WebAuthn::AuthenticatorTransport::Internal,
            has_resident_key: Some(true),
            has_user_verification: Some(true),
            is_user_verified: Some(true),
            automatic_presence_simulation: Some(true),
            has_large_blob: None,
            has_cred_blob: None,
            has_min_pin_length: None,
            has_prf: None,
            default_backup_eligibility: None,
            default_backup_state: None,
        },
    })
    .unwrap()
    .authenticator_id
}

/// Skickar Ta bort-formuläret på raden för passkeyn `namn`.
fn ta_bort_passkey(tab: &Tab, namn: &str) {
    let skript = format!(
        "[...document.querySelectorAll('li')]\
         .find(li => li.querySelector('strong')?.textContent === {namn:?})\
         .querySelector(\"form[action='/konto/passkeys/ta-bort'] button\").click()"
    );
    tab.evaluate(&skript, false).unwrap();
}

fn vänta_på_text_borta(tab: &Tab, text: &str) {
    let start = Instant::now();
    loop {
        let innehåll = tab
            .evaluate("document.body.innerText", false)
            .ok()
            .and_then(|svar| svar.value)
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_default();
        if !innehåll.is_empty() && !innehåll.contains(text) {
            return;
        }
        assert!(
            start.elapsed() < VÄNTA,
            "\"{text}\" försvann aldrig:\n{innehåll}"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn vänta_på_element_borta(tab: &Tab, selektor: &str) {
    let skript = format!("document.querySelector({selektor:?}) === null");
    let start = Instant::now();
    loop {
        let borta = tab
            .evaluate(&skript, false)
            .ok()
            .and_then(|svar| svar.value)
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if borta {
            return;
        }
        assert!(start.elapsed() < VÄNTA, "{selektor} försvann aldrig");
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Sidan har inte loggat något fel och har hydrerats.
fn inga_sidfel(tab: &Tab) {
    let start = Instant::now();
    loop {
        let fel = tab
            .evaluate("(window.__fel || []).join('\\n')", false)
            .ok()
            .and_then(|svar| svar.value)
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_default();
        assert!(
            fel.is_empty(),
            "sidan {} loggade fel:\n{fel}",
            tab.get_url()
        );
        if tab.find_element("html[data-hydrerad]").is_ok() {
            return;
        }
        assert!(
            start.elapsed() < VÄNTA,
            "sidan {} hydrerades aldrig",
            tab.get_url()
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}
