//! Kontoplanen under Inställningar → Kontoplan (variant A i `prototype/82-kontoplan`): en tät
//! tabell per kontoklass med sökning, flikar och filter, namnbyte och brytare på plats, nytt
//! konto och kontots historik i en sidopanel.
//!
//! Sidan fungerar utan WASM: urvalet står i adressen och servern filtrerar, och ändringarna är
//! vanliga formulär som `core` tar emot och som leder tillbaka hit med urvalet kvar. Servern
//! bygger hela sidans innehåll ([`Kontoplansida`]), som sidan läser med `sidodata::sida`.

use leptos::prelude::*;
use serde::{Deserialize, Serialize};

use crate::app::{LoggaIn, Läge};
use crate::foretag::{FöretagVy, IngenÅtkomst};
use crate::installningar::Inställningsmeny;
use crate::sidodata::sida;
use crate::utskrift::Utskrift;

/// Fliken: aktiva (standard), alla eller inaktiva konton.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Visa {
    #[default]
    Aktiva,
    Alla,
    Inaktiva,
}

impl Visa {
    const ALLA: [Visa; 3] = [Visa::Aktiva, Visa::Alla, Visa::Inaktiva];

    fn kod(self) -> &'static str {
        match self {
            Visa::Aktiva => "aktiva",
            Visa::Alla => "alla",
            Visa::Inaktiva => "inaktiva",
        }
    }

    fn namn(self) -> &'static str {
        match self {
            Visa::Aktiva => "Aktiva",
            Visa::Alla => "Alla",
            Visa::Inaktiva => "Inaktiva",
        }
    }
}

/// Urvalet i tabellen, som står i adressen.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Urval {
    pub q: String,
    pub visa: Visa,
    pub egna: bool,
    pub nya: bool,
}

impl Urval {
    /// Urvalet ur adressens parametrar; okända värden ger standardvalet.
    pub fn tolka<'a>(par: impl IntoIterator<Item = (&'a str, &'a str)>) -> Self {
        let mut urval = Urval::default();
        let mut sok = false;
        let mut visa = false;
        for (nyckel, värde) in par {
            match nyckel {
                "q" => urval.q = värde.trim().to_string(),
                // Länken "Aktivera" från verifikatformuläret: sökningen förifylld och alla
                // konton, så att ett inaktivt konto syns med sin brytare.
                "sok" => {
                    urval.q = värde.trim().to_string();
                    sok = true;
                }
                "visa" => {
                    visa = true;
                    urval.visa = Visa::ALLA
                        .into_iter()
                        .find(|v| v.kod() == värde)
                        .unwrap_or_default()
                }
                "egna" => urval.egna = värde == "1",
                "nya" => urval.nya = värde == "1",
                _ => {}
            }
        }
        if sok && !visa {
            urval.visa = Visa::Alla;
        }
        urval
    }

    /// Urvalet som parametrar i en adress, utan standardvärden.
    pub fn query(&self) -> String {
        let mut par: Vec<(&str, &str)> = Vec::new();
        if !self.q.is_empty() {
            par.push(("q", &self.q));
        }
        if self.visa != Visa::Aktiva {
            par.push(("visa", self.visa.kod()));
        }
        if self.egna {
            par.push(("egna", "1"));
        }
        if self.nya {
            par.push(("nya", "1"));
        }
        par.iter()
            .map(|(k, v)| format!("{k}={}", koda(v)))
            .collect::<Vec<_>>()
            .join("&")
    }

    /// Adressen till kontoplanen med urvalet och `extra` parametrar.
    fn href(&self, sida: &str, extra: &[(&str, &str)]) -> String {
        let mut query = self.query();
        for (k, v) in extra {
            if !query.is_empty() {
                query.push('&');
            }
            query.push_str(&format!("{k}={}", koda(v)));
        }
        if query.is_empty() {
            sida.to_string()
        } else {
            format!("{sida}?{query}")
        }
    }

    fn med(&self, ändra: impl FnOnce(&mut Urval)) -> Urval {
        let mut nytt = self.clone();
        ändra(&mut nytt);
        nytt
    }

    /// Om kontot matchar sökningen och filtren. `med_status` avgör om fliken räknas.
    pub fn matchar(&self, rad: &Kontorad, med_status: bool) -> bool {
        let q = self.q.to_lowercase();
        let sökt = q.is_empty()
            || rad.nummer.to_string().starts_with(&q)
            || rad.namn.to_lowercase().contains(&q)
            || rad
                .bas_namn
                .as_ref()
                .is_some_and(|n| n.to_lowercase().contains(&q));
        let status = !med_status
            || match self.visa {
                Visa::Aktiva => rad.aktiv,
                Visa::Alla => true,
                Visa::Inaktiva => !rad.aktiv,
            };
        sökt && status && (!self.egna || rad.eget) && (!self.nya || rad.ny_i_bas.is_some())
    }
}

/// Procentkodning för ett värde i en adress.
fn koda(värde: &str) -> String {
    let mut ut = String::new();
    for b in värde.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                ut.push(b as char)
            }
            _ => ut.push_str(&format!("%{b:02X}")),
        }
    }
    ut
}

/// En rad i tabellen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Kontorad {
    pub nummer: u16,
    pub namn: String,
    /// Namnet i BAS; `None` för ett eget konto.
    pub bas_namn: Option<String>,
    pub eget: bool,
    pub aktiv: bool,
    /// BAS-versionen som lade till kontot efter uppläggningen ("Nytt i BAS 2027").
    pub ny_i_bas: Option<String>,
    /// Flödet som bokför på kontot, så att det inte kan inaktiveras.
    pub flöde: Option<String>,
    /// INK2-raden ur numret, t.ex. "2.26 Kassa och bank".
    pub ink2: String,
}

/// En kontoklass i tabellen med sina rader.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Klass {
    pub klass: u8,
    pub namn: String,
    pub konton: Vec<Kontorad>,
}

/// Antalen i flikarna och filtren, över hela kontoplanen.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Antal {
    pub aktiva: usize,
    pub alla: usize,
    pub inaktiva: usize,
    pub egna: usize,
    pub nya: usize,
}

/// En bekräftelse eller ett avvisat val, överst på sidan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Meddelande {
    pub fel: bool,
    pub text: String,
}

/// Kontots sidopanel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Panel {
    pub nummer: u16,
    pub namn: String,
    pub typ: String,
    pub bas_namn: Option<String>,
    pub ink2: String,
    /// Saldot per räkenskapsår som inte är stängt, som text.
    pub saldon: Vec<String>,
    pub flöde: Option<String>,
    pub poster: Vec<Historikpost>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Historikpost {
    pub datum: String,
    pub aktör: String,
    pub maya: bool,
    pub typ: String,
    pub text: String,
}

/// Dialogen Nytt konto.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NyttFormulär {
    pub nummer: String,
    pub namn: String,
    /// (fält, kod, text)
    pub fel: Vec<(String, String, String)>,
    /// Numret på ett befintligt inaktivt konto som kan aktiveras i stället.
    pub aktivera: Option<u16>,
    /// Klass och kontogrupp ur numret.
    pub typ: Option<String>,
    pub ink2: Option<String>,
}

/// Allt som sidan visar, byggt på servern.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Kontoplansida {
    pub företag: FöretagVy,
    pub urval: Urval,
    pub antal: Antal,
    pub klasser: Vec<Klass>,
    /// Inaktiva konton som matchar sökningen när fliken Aktiva är vald.
    pub dolda_inaktiva: usize,
    pub meddelande: Option<Meddelande>,
    pub panel: Option<Panel>,
    pub nytt: Option<NyttFormulär>,
    /// INK2-tabellens utgåva.
    pub ink2_utgåva: String,
}

impl Kontoplansida {
    fn sida(&self) -> String {
        format!("{}/installningar/kontoplan", self.företag.href())
    }

    fn konto_href(&self, nummer: u16) -> String {
        format!("{}/kontoplan/{nummer}", self.företag.href())
    }
}

/// Raderna i tabellen per klass och antalen, ur hela kontoplanen och urvalet.
pub fn ordna(alla: &[Kontorad], urval: &Urval) -> (Vec<Klass>, Antal, usize) {
    let antal = Antal {
        aktiva: alla.iter().filter(|k| k.aktiv).count(),
        alla: alla.len(),
        inaktiva: alla.iter().filter(|k| !k.aktiv).count(),
        egna: alla.iter().filter(|k| k.eget).count(),
        nya: alla.iter().filter(|k| k.ny_i_bas.is_some()).count(),
    };
    let mut klasser: Vec<Klass> = Vec::new();
    for rad in alla.iter().filter(|k| urval.matchar(k, true)) {
        let klass = (rad.nummer / 1000) as u8;
        if klasser.last().is_none_or(|k| k.klass != klass) {
            klasser.push(Klass {
                klass,
                namn: maya_domain::kontoplan::klassnamn(klass).to_string(),
                konton: Vec::new(),
            });
        }
        klasser.last_mut().unwrap().konton.push(rad.clone());
    }
    let dolda = if !urval.q.is_empty() && urval.visa == Visa::Aktiva {
        alla.iter()
            .filter(|k| !k.aktiv && urval.matchar(k, false))
            .count()
    } else {
        0
    };
    (klasser, antal, dolda)
}

/// Sidan för `/foretag/<id>/installningar/kontoplan`, eller `None` utan åtkomst.
#[server(prefix = "/api", endpoint = "kontoplansida", input = server_fn::codec::GetUrl)]
pub async fn hämta_kontoplansida(adress: String) -> Result<Option<Kontoplansida>, ServerFnError> {
    match crate::sidodata::Sidbegäran::för(&adress)? {
        Some(begäran) => server::kontoplansida(&begäran).await,
        None => Ok(None),
    }
}

/// Inställningar → Kontoplan.
#[component]
pub fn KontoplanSida() -> impl IntoView {
    sida(hämta_kontoplansida, kontoplan)
}

fn kontoplan(sida: Option<Kontoplansida>) -> AnyView {
    if !matches!(use_context::<Läge>(), Some(Läge::Inloggad { .. })) {
        return view! { <LoggaIn /> }.into_any();
    }
    let Some(sida) = sida else {
        return view! { <IngenÅtkomst /> }.into_any();
    };
    let href = sida.sida();
    let aktiva = sida.antal.aktiva;
    let alla = sida.antal.alla;
    let bas = sida.företag.href();
    view! {
        <section class="sida kontoplan">
            <Inställningsmeny bas=bas aktiv="/kontoplan" />
            <div class="sidhuvud">
                <div>
                    <h1>"Kontoplan"</h1>
                    <p class="diskret">
                        {sida.företag.namn.clone()} " · " {aktiva} " aktiva av " {alla}
                        " konton · rapportrader enligt " {sida.ink2_utgåva.clone()}
                        " · ändras bara i webben"
                    </p>
                    <Utskrift href=format!("{href}/pdf") filnamn="kontoplan.pdf".to_string() />
                </div>
                <a
                    class="knapp"
                    href=sida.urval.href(&href, &[("nytt", "1")])
                >
                    "Nytt konto"
                </a>
            </div>
            {sida.meddelande.clone().map(|m| view! { <Meddelanderad m /> })}
            {sida.nytt.clone().map(|f| view! { <NyttKontoDialog sida=sida.clone() f /> })}
            <Filterrad sida=sida.clone() />
            <Tabell sida=sida.clone() />
            {(sida.dolda_inaktiva > 0)
                .then(|| {
                    let visa_alla = sida.urval.med(|u| u.visa = Visa::Alla).href(&href, &[]);
                    view! {
                        <p class="fler">
                            {format!("{} inaktiva konton matchar också. ", sida.dolda_inaktiva)}
                            <a href=visa_alla>
                                "Visa alla"
                            </a>
                        </p>
                    }
                })}
            {sida.klasser.is_empty().then(|| view! { <p class="tom">"Inga konton matchar."</p> })}
            <p class="diskret">
                "Namn och aktiv ändras direkt i tabellen och loggas i behandlingshistoriken. "
                "INK2-raden följer kontonumret. Kontonummer ändras aldrig och konton raderas aldrig."
            </p>
        </section>
        {sida.panel.clone().map(|p| view! { <Sidopanel sida=sida.clone() p /> })}
    }
    .into_any()
}

#[component]
fn Meddelanderad(m: Meddelande) -> impl IntoView {
    let klass = if m.fel {
        "meddelande fel"
    } else {
        "meddelande"
    };
    let roll = if m.fel { "alert" } else { "status" };
    view! {
        <p class=klass role=roll>
            {m.text}
        </p>
    }
    .into_any()
}

#[component]
fn Filterrad(sida: Kontoplansida) -> impl IntoView {
    let href = sida.sida();
    let u = sida.urval.clone();
    let antal = sida.antal.clone();
    let flik = {
        let href = href.clone();
        let u = u.clone();
        move |visa: Visa, n: usize| {
            let länk = u.med(|u| u.visa = visa).href(&href, &[]);
            let vald = (u.visa == visa).then_some("page");
            view! {
                <a href=länk aria-current=vald>
                    {visa.namn()}
                    " "
                    <span class="diskret">{n}</span>
                </a>
            }
        }
    };
    let filter = {
        let href = href.clone();
        let u = u.clone();
        move |namn: &'static str, på: bool, n: usize, växla: fn(&mut Urval)| {
            let länk = u.med(växla).href(&href, &[]);
            let klass = if på { "chip pa" } else { "chip" };
            view! {
                <a class=klass href=länk aria-pressed=på.to_string()>
                    {namn}
                    " "
                    <span class="diskret">{n}</span>
                </a>
            }
        }
    };
    view! {
        <div class="filterrad">
            <form class="sok" method="get" action=href.clone()>
                <input
                    id="kontosok"
                    name="q"
                    type="search"
                    placeholder="Sök nummer eller namn"
                    value=u.q.clone()
                    autocomplete="off"
                />
                <kbd>"/"</kbd>
                {(u.visa != Visa::Aktiva)
                    .then(|| view! { <input type="hidden" name="visa" value=u.visa.kod() /> })}
                {u.egna.then(|| view! { <input type="hidden" name="egna" value="1" /> })}
                {u.nya.then(|| view! { <input type="hidden" name="nya" value="1" /> })}
            </form>
            <nav class="tabbar" aria-label="Urval">
                {flik(Visa::Aktiva, antal.aktiva)} {flik(Visa::Alla, antal.alla)}
                {flik(Visa::Inaktiva, antal.inaktiva)}
            </nav>
            {filter("Egna", u.egna, antal.egna, |u| u.egna = !u.egna)}
            {filter("Nya i BAS", u.nya, antal.nya, |u| u.nya = !u.nya)}
        </div>
    }
    .into_any()
}

#[component]
fn Tabell(sida: Kontoplansida) -> impl IntoView {
    let tillbaka = sida.urval.query();
    let klasser = sida.klasser.clone();
    view! {
        <table class="kontotabell">
            <thead>
                <tr>
                    <th>"Konto"</th>
                    <th>"Namn"</th>
                    <th>"Aktiv"</th>
                    <th>"INK2-rad"</th>
                    <th></th>
                    <th></th>
                </tr>
            </thead>
            {klasser
                .into_iter()
                .map(|k| {
                    let antal = k.konton.len();
                    view! {
                        <tbody>
                            <tr class="klassrad">
                                <th colspan="6" scope="rowgroup">
                                    {format!("Klass {} · {}", k.klass, k.namn)}
                                    " "
                                    <span class="diskret">{antal} " konton"</span>
                                </th>
                            </tr>
                            {k
                                .konton
                                .into_iter()
                                .map(|rad| {
                                    view! { <Rad sida=sida.clone() rad tillbaka=tillbaka.clone() /> }
                                })
                                .collect_view()}
                        </tbody>
                    }
                })
                .collect_view()}
        </table>
    }
    .into_any()
}

#[component]
fn Rad(sida: Kontoplansida, rad: Kontorad, tillbaka: String) -> impl IntoView {
    let konto = sida.konto_href(rad.nummer);
    let historik = sida
        .urval
        .href(&sida.sida(), &[("konto", &rad.nummer.to_string())]);
    let klass = if rad.aktiv { "" } else { "inaktiv" };
    let titel = rad
        .bas_namn
        .as_ref()
        .filter(|b| **b != rad.namn)
        .map(|b| format!("Namn i BAS: {b}"));
    let brytare = match (&rad.flöde, rad.aktiv) {
        (Some(flöde), true) => {
            let skäl = format!("Används av flödet {flöde}");
            view! {
                <button
                    class="brytare pa"
                    type="button"
                    role="switch"
                    aria-checked="true"
                    disabled
                    title=skäl.clone()
                    aria-label=skäl
                ></button>
            }
            .into_any()
        }
        (_, aktiv) => {
            let etikett = if aktiv {
                format!("Inaktivera {}", rad.nummer)
            } else {
                format!("Aktivera {}", rad.nummer)
            };
            view! {
                <form method="post" action=format!("{konto}/aktiv")>
                    <input type="hidden" name="aktiv" value=if aktiv { "0" } else { "1" } />
                    <input type="hidden" name="tillbaka" value=tillbaka.clone() />
                    <button
                        class=if aktiv { "brytare pa" } else { "brytare" }
                        type="submit"
                        role="switch"
                        aria-checked=aktiv.to_string()
                        title=etikett.clone()
                        aria-label=etikett
                    ></button>
                </form>
            }
            .into_any()
        }
    };
    view! {
        <tr class=klass data-konto=rad.nummer.to_string()>
            <td class="nr">{rad.nummer}</td>
            <td>
                <div class="namn">
                <form class="namnform" method="post" action=format!("{konto}/namn")>
                    <input type="hidden" name="tillbaka" value=tillbaka.clone() />
                    <input
                        class="cell"
                        name="namn"
                        value=rad.namn.clone()
                        title=titel
                        aria-label=format!("Namn på {}", rad.nummer)
                        required
                    />
                </form>
                {rad.eget.then(|| view! { <span class="etikett">"Eget"</span> })}
                {rad
                    .ny_i_bas
                    .clone()
                    .map(|v| view! { <span class="etikett ny">{format!("Nytt i {v}")}</span> })}
                {rad
                    .flöde
                    .clone()
                    .map(|f| {
                        view! {
                            <span class="etikett las" title=format!("Används av flödet {f}")>
                                "Flöde"
                            </span>
                        }
                    })}
                </div>
            </td>
            <td>{brytare}</td>
            <td class="ink2" title=rad.ink2.clone()>
                {rad.ink2.clone()}
            </td>
            <td class="ikon">
                {(rad.ny_i_bas.is_some() && !rad.aktiv)
                    .then(|| {
                        view! {
                            <span class="varning" title="Nytt i BAS. Inaktivt tills någon aktiverar det.">
                                <TriangelVarning />
                            </span>
                        }
                    })}
            </td>
            <td class="ikon">
                <a
                    class="knapp knapp-diskret"
                    href=historik
                    title="Historik"
                    aria-label=format!("Historik för {}", rad.nummer)
                >
                    "↺"
                </a>
            </td>
        </tr>
    }
    .into_any()
}

#[component]
fn NyttKontoDialog(sida: Kontoplansida, f: NyttFormulär) -> impl IntoView {
    let stäng = sida.urval.href(&sida.sida(), &[]);
    let fel_vid = |fält: &str| {
        f.fel
            .iter()
            .filter(|(f, _, _)| f == fält)
            .map(|(_, kod, text)| {
                view! {
                    <p class="fel" role="alert" data-kod=kod.clone()>
                        {text.clone()}
                    </p>
                }
            })
            .collect_view()
    };
    let aktivera = f.aktivera.map(|nr| {
        view! {
            <form method="post" action=format!("{}/aktiv", sida.konto_href(nr))>
                <input type="hidden" name="aktiv" value="1" />
                <input type="hidden" name="tillbaka" value=sida.urval.query() />
                <button class="knapp" type="submit">
                    {format!("Aktivera {nr}")}
                </button>
            </form>
        }
    });
    view! {
        <section class="dialog" aria-labelledby="nytt-konto">
            <h2 id="nytt-konto">"Nytt konto"</h2>
            <form class="formular" method="post" action=format!("{}/kontoplan", sida.företag.href())>
                <input type="hidden" name="tillbaka" value=sida.urval.query() />
                <label for="nummer">"Kontonummer"</label>
                <input
                    id="nummer"
                    name="nummer"
                    inputmode="numeric"
                    maxlength="4"
                    placeholder="t.ex. 6542"
                    value=f.nummer.clone()
                />
                {fel_vid("nummer")}
                <label for="kontonamn">"Namn"</label>
                <input id="kontonamn" name="namn" value=f.namn.clone() />
                {fel_vid("namn")}
                {match (f.typ.clone(), f.ink2.clone()) {
                    (Some(typ), ink2) => {
                        view! {
                            <dl class="fakta">
                                <dt>"Typ"</dt>
                                <dd>{typ}</dd>
                                <dt>"INK2-rad"</dt>
                                <dd>
                                    {ink2
                                        .unwrap_or_else(|| {
                                            "Ingen rad, kontot syns inte i rapporterna".into()
                                        })}
                                </dd>
                            </dl>
                        }
                            .into_any()
                    }
                    (None, _) => {
                        view! {
                            <p class="diskret">
                                "Ett eget konto får ett fyrsiffrigt nummer som inte finns i kontoplanen. "
                                "Numret kan inte ändras senare och kontot kan aldrig raderas, bara inaktiveras."
                            </p>
                        }
                            .into_any()
                    }
                }}
                <div class="knappar">
                    <a class="knapp knapp-diskret" href=stäng>
                        "Avbryt"
                    </a>
                    <button
                        class="knapp knapp-diskret"
                        type="submit"
                        formmethod="get"
                        formaction=sida.sida()
                        name="nytt"
                        value="1"
                    >
                        "Visa klass och INK2-rad"
                    </button>
                    <button class="knapp" type="submit">
                        "Skapa konto"
                    </button>
                </div>
            </form>
            {aktivera}
        </section>
    }
    .into_any()
}

#[component]
fn Sidopanel(sida: Kontoplansida, p: Panel) -> impl IntoView {
    let stäng = sida.urval.href(&sida.sida(), &[]);
    view! {
        <aside class="sidopanel" aria-labelledby="panelrubrik">
            <div class="panelhuvud">
                <h2 id="panelrubrik">{format!("{} {}", p.nummer, p.namn)}</h2>
                <a class="knapp knapp-diskret" href=stäng aria-label="Stäng">
                    "×"
                </a>
            </div>
            <dl class="fakta">
                <dt>"Typ"</dt>
                <dd>{p.typ}</dd>
                {p
                    .bas_namn
                    .map(|b| {
                        view! {
                            <dt>"Namn i BAS"</dt>
                            <dd>{b}</dd>
                        }
                    })}
                <dt>"INK2-rad"</dt>
                <dd>{p.ink2}</dd>
                <dt>"Saldo"</dt>
                <dd>
                    {if p.saldon.is_empty() {
                        "0,00 kr".to_string()
                    } else {
                        p.saldon.join(", ")
                    }}
                </dd>
                {p
                    .flöde
                    .map(|f| {
                        view! {
                            <dt>"Används av"</dt>
                            <dd>{format!("flödet {f}")}</dd>
                        }
                    })}
            </dl>
            <h3>"Behandlingshistorik"</h3>
            <ol class="historik" reversed>
                {p
                    .poster
                    .into_iter()
                    .rev()
                    .map(|post| {
                        view! {
                            <li class=if post.maya { "maya" } else { "" }>
                                <span class="diskret">{post.datum}</span>
                                " "
                                <span class="typ">{post.typ}</span>
                                <div>{post.text}</div>
                                <div class="diskret">{post.aktör}</div>
                            </li>
                        }
                    })
                    .collect_view()}
            </ol>
        </aside>
    }
    .into_any()
}

#[cfg(feature = "ssr")]
pub(crate) mod server {
    use std::sync::Arc;

    use axum::extract::{Path, State};
    use axum::http::{StatusCode, header};
    use axum::response::{IntoResponse, Redirect, Response};
    use axum::routing::post;
    use axum::{Extension, Form, Router};
    use maya_core::aktor::Kanal;
    use maya_core::atkomst::ÅtkomstFel;
    use maya_core::behandlingshistorik::Post;
    use maya_core::foretag::{self, FöretagsId};
    use maya_core::inloggning::Inloggad;
    use maya_core::kontoplan::{self, Konto, KontoFel, Ursprung};
    use maya_core::{Databas, Klocka};
    use maya_domain::kontoplan::{
        self as regler, Kontonummer, NyttKonto, flöde_som_bokför_på, ink2, om_numret,
    };
    use serde::Deserialize;

    use super::*;
    use crate::foretag::server::vy;
    use crate::sidodata::{self, Sidbegäran};

    #[derive(Clone)]
    pub(crate) struct KontoplansState {
        pub db: Databas,
        pub klocka: Klocka,
    }

    pub(crate) fn routes(state: KontoplansState) -> Router {
        Router::new()
            .route("/foretag/{id}/kontoplan", post(skapa))
            .route("/foretag/{id}/kontoplan/{nummer}/namn", post(byt_namn))
            .route("/foretag/{id}/kontoplan/{nummer}/aktiv", post(aktiv))
            .with_state(Arc::new(state))
    }

    /// Bygger kontoplanens sida när begäran gäller den. Utan åtkomst blir den `None`, och
    /// sidan svarar 403.
    pub(crate) async fn kontoplansida(
        b: &Sidbegäran,
    ) -> Result<Option<Kontoplansida>, ServerFnError> {
        let delar = b.delar();
        let ["foretag", id, "installningar", "kontoplan"] = delar.as_slice() else {
            return Ok(None);
        };
        let Ok(id) = id.parse::<FöretagsId>() else {
            return Ok(None);
        };
        match bygg(&b.db, &b.inloggad, id, b.fråga()).await {
            Ok(sida) => Ok(Some(sida)),
            Err(ÅtkomstFel::IngenÅtkomst | ÅtkomstFel::KanalenFårInte) => Ok(None),
            Err(e) => Err(sidodata::serverfel(e)),
        }
    }

    fn ink2_text(nummer: Kontonummer) -> String {
        om_numret(nummer)
            .ink2
            .iter()
            .map(|r| format!("{} {}", r.rad(), r.benämning()))
            .collect::<Vec<_>>()
            .join(" / ")
    }

    fn typ_text(nummer: Kontonummer) -> String {
        let om = om_numret(nummer);
        let grupp = match om.gruppnamn {
            Some(namn) => format!("grupp {} {namn}", om.kontogrupp),
            None => format!("grupp {}", om.kontogrupp),
        };
        format!("Klass {}: {} · {grupp}", om.klass, om.klassnamn)
    }

    fn rad(k: &Konto) -> Kontorad {
        Kontorad {
            nummer: k.nummer.get(),
            namn: k.namn.clone(),
            bas_namn: k.bas_namn.clone(),
            eget: k.ursprung == Ursprung::Eget,
            aktiv: k.aktiv,
            ny_i_bas: k.ny_i_bas.then(|| k.bas_version.clone()).flatten(),
            flöde: flöde_som_bokför_på(k.nummer).map(str::to_string),
            ink2: ink2_text(k.nummer),
        }
    }

    async fn bygg(
        db: &Databas,
        inloggad: &Inloggad,
        id: FöretagsId,
        query: &str,
    ) -> Result<Kontoplansida, ÅtkomstFel> {
        let aktör = &inloggad.aktör;
        let företaget = foretag::mina_företag(db, aktör)
            .await?
            .into_iter()
            .find(|f| f.id == id)
            .ok_or(ÅtkomstFel::IngenÅtkomst)?;
        let konton = kontoplan::konton(db, aktör, id).await?;
        let par: Vec<(String, String)> = url::form_urlencoded::parse(query.as_bytes())
            .into_owned()
            .collect();
        let hämta = |nyckel: &str| {
            par.iter()
                .find(|(k, _)| k == nyckel)
                .map(|(_, v)| v.as_str())
        };
        // Knappen "Visa klass och INK2-rad" skickar dialogen med GET, och urvalet följer då med
        // i `tillbaka` som när dialogen skickas med POST.
        let urval = match hämta("tillbaka") {
            Some(tillbaka) => {
                let par: Vec<(String, String)> = url::form_urlencoded::parse(tillbaka.as_bytes())
                    .into_owned()
                    .collect();
                Urval::tolka(par.iter().map(|(k, v)| (k.as_str(), v.as_str())))
            }
            None => Urval::tolka(par.iter().map(|(k, v)| (k.as_str(), v.as_str()))),
        };
        let rader: Vec<Kontorad> = konton.iter().map(rad).collect();
        let (klasser, antal, dolda_inaktiva) = ordna(&rader, &urval);
        let konto = |nr: Option<&str>| {
            nr.and_then(|n| n.parse().ok())
                .and_then(Kontonummer::new)
                .and_then(|nr| konton.iter().find(|k| k.nummer == nr))
        };

        let meddelande = match konto(hämta("andrat")) {
            Some(k) => meddelande(db, inloggad, id, k, hämta("meddelande")).await?,
            None => None,
        };
        let panel = match konto(hämta("konto")) {
            Some(k) => Some(panel(db, inloggad, id, k).await?),
            None => None,
        };
        let nytt = (hämta("nytt") == Some("1")).then(|| {
            nytt_formulär(
                &konton,
                hämta("nummer").unwrap_or_default(),
                hämta("namn").unwrap_or_default(),
                hämta("skickat") == Some("1"),
            )
        });
        Ok(Kontoplansida {
            företag: vy(företaget),
            urval,
            antal,
            klasser,
            dolda_inaktiva,
            meddelande,
            panel,
            nytt,
            ink2_utgåva: ink2::UTGÅVA.to_string(),
        })
    }

    /// Bekräftelsen eller skälet efter en ändring, ur kontots läge nu. Adressen bär bara en
    /// kod, aldrig text.
    async fn meddelande(
        db: &Databas,
        inloggad: &Inloggad,
        id: FöretagsId,
        k: &Konto,
        kod: Option<&str>,
    ) -> Result<Option<Meddelande>, ÅtkomstFel> {
        let ok = |text: String| Some(Meddelande { fel: false, text });
        let fel = |text: String| Some(Meddelande { fel: true, text });
        let nr = k.nummer;
        Ok(match kod {
            Some("namnbytt") => ok(format!("{nr} heter nu {}.", k.namn)),
            Some("aktiverat") => ok(format!("{nr} {} är aktivt och kan bokföras på.", k.namn)),
            Some("inaktiverat") => ok(format!(
                "{nr} {} är inaktivt. Utkast får använda det, men det kan inte bokföras på förrän \
                 kontot aktiveras.",
                k.namn
            )),
            Some("skapat") => ok(format!("{nr} {} är skapat och aktivt.", k.namn)),
            Some("namn_tom") => fel(format!("Namnet på {nr} kan inte vara tomt.")),
            Some("inaktivering") => {
                let saldon = kontoplan::saldon(db, &inloggad.aktör, id, nr).await?;
                let skäl = match regler::kan_inaktiveras(nr, &saldon) {
                    Err(spärr) => format!(" {spärr}"),
                    Ok(()) => String::new(),
                };
                fel(format!("{nr} kan inte inaktiveras.{skäl}"))
            }
            _ => None,
        })
    }

    fn historikpost(post: Post) -> Historikpost {
        let b = &post.beskrivning;
        let fält = |namn: &str| b[namn].as_str().unwrap_or_default().to_string();
        let text = match post.typ.as_str() {
            "företag skapat" => format!(
                "Kopierat från {} när företaget lades upp",
                fält("kontoplan")
            ),
            "konto namnbytt" => format!(
                "Namn ändrat från \"{}\" till \"{}\"",
                fält("från"),
                fält("till")
            ),
            "konto aktiverat" => "Aktiverat".into(),
            "konto inaktiverat" => "Inaktiverat".into(),
            "konto skapat" => format!("Skapat som eget konto med namnet \"{}\"", fält("namn")),
            "BAS-tillägg" => format!("Tillagt som inaktivt från {}", fält("bas_version")),
            annat => annat.to_string(),
        };
        let maya = post.aktör.kanal == Kanal::Maya;
        let aktör = match (&post.användarnamn, maya) {
            (_, true) => "Maya".to_string(),
            (Some(namn), _) => format!("{namn} ({})", post.aktör.kanal.namn()),
            (None, _) => post.aktör.kanal.namn().to_string(),
        };
        Historikpost {
            datum: maya_domain::tid::datum(post.tid).to_string(),
            aktör,
            maya,
            typ: post.typ,
            text,
        }
    }

    async fn panel(
        db: &Databas,
        inloggad: &Inloggad,
        id: FöretagsId,
        k: &Konto,
    ) -> Result<Panel, ÅtkomstFel> {
        let poster = kontoplan::historik(db, &inloggad.aktör, id, k.nummer).await?;
        let saldon = kontoplan::saldon(db, &inloggad.aktör, id, k.nummer).await?;
        let ink2 = ink2_text(k.nummer);
        Ok(Panel {
            nummer: k.nummer.get(),
            namn: k.namn.clone(),
            typ: typ_text(k.nummer),
            bas_namn: k.bas_namn.clone(),
            ink2: if ink2.is_empty() { "–".into() } else { ink2 },
            saldon: saldon
                .iter()
                .map(|s| format!("{} kr ({}–{})", s.saldo, s.start, s.slut))
                .collect(),
            flöde: flöde_som_bokför_på(k.nummer).map(str::to_string),
            poster: poster.into_iter().map(historikpost).collect(),
        })
    }

    /// Dialogen med det användaren skrev. Felen räknas med samma regler som `core`, och om
    /// numret finns ur kontoplanen.
    fn nytt_formulär(konton: &[Konto], nummer: &str, namn: &str, skickat: bool) -> NyttFormulär {
        let formulär = NyttKonto {
            nummer: nummer.into(),
            namn: namn.into(),
        };
        let mut f = NyttFormulär {
            nummer: nummer.into(),
            namn: namn.into(),
            ..NyttFormulär::default()
        };
        let giltigt = nummer
            .trim()
            .parse()
            .ok()
            .filter(|_| nummer.trim().len() == 4)
            .and_then(Kontonummer::new);
        if let Some(nr) = giltigt {
            f.typ = Some(typ_text(nr));
            let ink2 = ink2_text(nr);
            f.ink2 = (!ink2.is_empty()).then_some(ink2);
            if let Some(finns) = konton.iter().find(|k| k.nummer == nr) {
                let inaktivt = if finns.aktiv { "" } else { " (inaktivt)" };
                f.fel.push((
                    "nummer".into(),
                    "nummer_finns".into(),
                    format!("{nr} finns redan: {}{inaktivt}.", finns.namn),
                ));
                f.aktivera = (!finns.aktiv).then_some(nr.get());
            }
        }
        if let Err(fel) = formulär.kontrollera() {
            for fel in fel {
                let tomt = match fel.fält {
                    regler::KontoFält::Nummer => nummer.trim().is_empty(),
                    regler::KontoFält::Namn => namn.trim().is_empty(),
                };
                // Ett fält som ännu inte är ifyllt får sitt fel först när formuläret skickas.
                if skickat || !tomt {
                    f.fel
                        .push((fel.fält.namn().into(), fel.kod.into(), fel.text));
                }
            }
        }
        f
    }

    #[derive(Deserialize)]
    struct NyttFält {
        #[serde(default)]
        nummer: String,
        #[serde(default)]
        namn: String,
        #[serde(default)]
        tillbaka: String,
    }

    #[derive(Deserialize)]
    struct NamnFält {
        #[serde(default)]
        namn: String,
        #[serde(default)]
        tillbaka: String,
    }

    #[derive(Deserialize)]
    struct AktivFält {
        #[serde(default)]
        aktiv: String,
        #[serde(default)]
        tillbaka: String,
    }

    /// Tillbaka till kontoplanen med urvalet som formuläret bar och `extra` parametrar. Bara
    /// urvalets kända parametrar följer med.
    fn tillbaka(id: FöretagsId, urval_query: &str, extra: &[(&str, &str)]) -> Response {
        let par: Vec<(String, String)> = url::form_urlencoded::parse(urval_query.as_bytes())
            .into_owned()
            .collect();
        let urval = Urval::tolka(par.iter().map(|(k, v)| (k.as_str(), v.as_str())));
        let sida = format!("/foretag/{id}/installningar/kontoplan");
        Redirect::to(&urval.href(&sida, extra)).into_response()
    }

    fn konto_fel(id: FöretagsId, urval: &str, nr: Kontonummer, e: KontoFel) -> Response {
        let nr = nr.to_string();
        match e {
            KontoFel::Åtkomst(ÅtkomstFel::IngenÅtkomst | ÅtkomstFel::KanalenFårInte) => {
                ingen_åtkomst()
            }
            KontoFel::KanInteInaktiveras(_) => tillbaka(
                id,
                urval,
                &[("meddelande", "inaktivering"), ("andrat", &nr)],
            ),
            KontoFel::NamnTomt => {
                tillbaka(id, urval, &[("meddelande", "namn_tom"), ("andrat", &nr)])
            }
            KontoFel::FinnsInte => tillbaka(id, urval, &[]),
            e => serverfel(e),
        }
    }

    async fn skapa(
        State(state): State<Arc<KontoplansState>>,
        Path(id): Path<String>,
        inloggad: Option<Extension<Inloggad>>,
        Form(f): Form<NyttFält>,
    ) -> Response {
        let Some(Extension(inloggad)) = inloggad else {
            return Redirect::to("/").into_response();
        };
        let Ok(id) = id.parse::<FöretagsId>() else {
            return ingen_åtkomst();
        };
        let formulär = NyttKonto {
            nummer: f.nummer.clone(),
            namn: f.namn.clone(),
        };
        match kontoplan::skapa_eget(&state.db, &state.klocka, &inloggad.aktör, id, &formulär).await
        {
            Ok(nr) => tillbaka(
                id,
                &f.tillbaka,
                &[("meddelande", "skapat"), ("andrat", &nr.to_string())],
            ),
            Err(KontoFel::Ogiltigt(_) | KontoFel::Finns { .. }) => tillbaka(
                id,
                &f.tillbaka,
                &[
                    ("nytt", "1"),
                    ("skickat", "1"),
                    ("nummer", &f.nummer),
                    ("namn", &f.namn),
                ],
            ),
            Err(KontoFel::Åtkomst(ÅtkomstFel::IngenÅtkomst | ÅtkomstFel::KanalenFårInte)) => {
                ingen_åtkomst()
            }
            Err(e) => serverfel(e),
        }
    }

    fn sökväg(id: &str, nummer: u16) -> Option<(FöretagsId, Kontonummer)> {
        Some((id.parse().ok()?, Kontonummer::new(nummer)?))
    }

    async fn byt_namn(
        State(state): State<Arc<KontoplansState>>,
        Path((id, nummer)): Path<(String, u16)>,
        inloggad: Option<Extension<Inloggad>>,
        Form(f): Form<NamnFält>,
    ) -> Response {
        let Some(Extension(inloggad)) = inloggad else {
            return Redirect::to("/").into_response();
        };
        let Some((id, nr)) = sökväg(&id, nummer) else {
            return ingen_åtkomst();
        };
        match kontoplan::byt_namn(&state.db, &state.klocka, &inloggad.aktör, id, nr, &f.namn).await
        {
            Ok(()) => tillbaka(
                id,
                &f.tillbaka,
                &[("meddelande", "namnbytt"), ("andrat", &nr.to_string())],
            ),
            Err(e) => konto_fel(id, &f.tillbaka, nr, e),
        }
    }

    async fn aktiv(
        State(state): State<Arc<KontoplansState>>,
        Path((id, nummer)): Path<(String, u16)>,
        inloggad: Option<Extension<Inloggad>>,
        Form(f): Form<AktivFält>,
    ) -> Response {
        let Some(Extension(inloggad)) = inloggad else {
            return Redirect::to("/").into_response();
        };
        let Some((id, nr)) = sökväg(&id, nummer) else {
            return ingen_åtkomst();
        };
        let (svar, kod) = if f.aktiv == "1" {
            (
                kontoplan::aktivera(&state.db, &state.klocka, &inloggad.aktör, id, nr).await,
                "aktiverat",
            )
        } else {
            (
                kontoplan::inaktivera(&state.db, &state.klocka, &inloggad.aktör, id, nr).await,
                "inaktiverat",
            )
        };
        match svar {
            Ok(()) => tillbaka(
                id,
                &f.tillbaka,
                &[("meddelande", kod), ("andrat", &nr.to_string())],
            ),
            Err(e) => konto_fel(id, &f.tillbaka, nr, e),
        }
    }

    fn ingen_åtkomst() -> Response {
        (
            StatusCode::FORBIDDEN,
            [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
            "Du har inte åtkomst till företaget.\n",
        )
            .into_response()
    }

    fn serverfel(e: impl std::fmt::Display) -> Response {
        tracing::error!("kontoplanen: {e}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
            "Något gick fel i Maya; försök igen.\n",
        )
            .into_response()
    }
}

/// Lucides varningstriangel (`triangle-alert`).
#[component]
fn TriangelVarning() -> impl IntoView {
    view! {
        <svg
            class="lucide lucide-triangle-alert"
            xmlns="http://www.w3.org/2000/svg"
            width="14"
            height="14"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
            stroke-linecap="round"
            stroke-linejoin="round"
            aria-hidden="true"
        >
            <path d="m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3" />
            <path d="M12 9v4" />
            <path d="M12 17h.01" />
        </svg>
    }
}
