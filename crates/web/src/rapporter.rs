//! Rapporter: fliken med rapporterna och SIE4-exporten, och resultat- och balansräkningen som
//! dokument (variant A i `prototype/69-rapporter`).
//!
//! SIE4-filen hämtas med `GET /foretag/<id>/sie4?ar=<start>`. `core` läser tillbaka och stämmer
//! av filen innan den lämnas ut; stoppas exporten skickas användaren tillbaka till fliken med
//! `fel=sie4`.
//!
//! I resultat- och balansräkningen
//! står urvalet i adressen: `ar` (räkenskapsårets start), `tom` (periodens sista månad,
//! `ÅÅÅÅ-MM`), `visa` (resultaträkningen: `manad|ackumulerat|bada`), `jmf=1` (jämförelse med
//! föregående år), `niva` (`rubriker|ink2|konton`) och `konto` (panelen med kontots verifikat).
//! Datan kommer från middleware vid serverrenderingen, så länkar som ändrar urvalet har
//! `rel="external"`: klientens router skulle annars rita sidan utan ny data. Exporten (PDF och
//! CSV) tar samma urval och ger samma kolumner och nivå som sidan.

use leptos::prelude::*;
use leptos_router::hooks::{use_params_map, use_query_map};
use serde::{Deserialize, Serialize};

use crate::app::{LoggaIn, Läge};
use crate::foretag::{FöretagVy, Företagsval, IngenÅtkomst};
use crate::utskrift::Utskrift;

/// Koden i `fel=` när SIE4-exporten stoppades vid återläsningen.
pub const SIE4_STOPPAD: &str = "sie4";

/// Det som fliken visar. Middleware hämtar det ur `core` och lägger det på begäran, och det
/// följer med till webbläsaren vid hydreringen.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rapportdata {
    /// Räkenskapsåren i ordning, som start och slut `ÅÅÅÅ-MM-DD`.
    pub år: Vec<(String, String)>,
}

fn data() -> Rapportdata {
    SharedValue::new(|| {
        #[cfg(feature = "ssr")]
        if let Some(parts) = use_context::<axum::http::request::Parts>()
            && let Some(data) = parts.extensions.get::<Rapportdata>()
        {
            return data.clone();
        }
        Rapportdata::default()
    })
    .into_inner()
}

/// Rapporterna i fliken, i grupper: (grupp, namn, sökväg under företaget). Nya rapporter läggs
/// till här.
pub const RAPPORTER: &[(&str, &str, &str)] = &[
    ("Bokslut", "Resultaträkning", "/rapporter/resultatrakning"),
    ("Bokslut", "Balansräkning", "/rapporter/balansrakning"),
    ("Bokföring", "Kontoplan", "/installningar/kontoplan"),
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KolumnVy {
    pub rubrik: String,
    pub underrubrik: String,
    pub föregående: bool,
}

/// En rad i dokumentet. `slag` är CSS-klassen; beloppen är formaterade, `None` är en
/// jämförelse som saknas och en tom lista en överskrift utan belopp.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RadVy {
    pub slag: String,
    pub text: String,
    pub ink2: Option<String>,
    pub konto: Option<u16>,
    pub värden: Vec<Option<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PanelradVy {
    pub verifikat: String,
    pub nummer: String,
    pub datum: String,
    pub text: String,
    pub debet: String,
    pub kredit: String,
    pub saldo: String,
}

/// Panelen med ett kontos verifikat i perioden.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PanelVy {
    pub konto: u16,
    pub namn: String,
    pub period: String,
    /// Datum och saldo, när perioden börjar med den ingående balansen.
    pub ingående: Option<(String, String)>,
    pub rader: Vec<PanelradVy>,
    pub utgående: String,
}

/// Det som rapportsidan visar. Middleware hämtar det ur `core` och lägger det på begäran, och
/// det följer med till webbläsaren vid hydreringen.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rapportvy {
    /// `resultatrakning` eller `balansrakning`.
    pub typ: String,
    pub titel: String,
    /// Räkenskapsåren som start och slut `ÅÅÅÅ-MM-DD`.
    pub år: Vec<(String, String)>,
    pub valt: String,
    /// Årets månader som `ÅÅÅÅ-MM` och namn.
    pub månader: Vec<(String, String)>,
    pub tom: String,
    pub visning: String,
    pub jämförelse: bool,
    pub nivå: String,
    pub period: String,
    /// Den ingående balansen är preliminär (balansräkningen); föregående år som start och slut.
    pub preliminär: Option<(String, String)>,
    pub första_året: bool,
    pub kolumner: Vec<KolumnVy>,
    pub rader: Vec<RadVy>,
    pub panel: Option<PanelVy>,
    /// Exportens filnamn utan ändelse.
    pub filnamn: String,
}

impl Rapportvy {
    /// Adressen till rapporten med urvalet, ändrat enligt `ändra` (ett tomt värde tar bort
    /// parametern). Panelen följer inte med om den inte anges.
    fn adress(&self, sökväg: &str, ändra: &[(&str, &str)]) -> String {
        let mut fält: Vec<(&str, String)> = vec![
            ("ar", self.valt.clone()),
            ("tom", self.tom.clone()),
            ("visa", self.visning.clone()),
            ("jmf", if self.jämförelse { "1" } else { "" }.into()),
            ("niva", self.nivå.clone()),
        ];
        if self.typ != "resultatrakning" {
            fält.retain(|(k, _)| *k != "visa");
        }
        for &(nyckel, värde) in ändra {
            fält.retain(|(k, _)| *k != nyckel);
            fält.push((nyckel, värde.to_string()));
        }
        let query: Vec<String> = fält
            .into_iter()
            .filter(|(_, v)| !v.is_empty())
            .map(|(k, v)| format!("{k}={v}"))
            .collect();
        format!("{sökväg}?{}", query.join("&"))
    }
}

fn rapportvy() -> Rapportvy {
    SharedValue::new(|| {
        #[cfg(feature = "ssr")]
        if let Some(parts) = use_context::<axum::http::request::Parts>()
            && let Some(data) = parts.extensions.get::<Rapportvy>()
        {
            return data.clone();
        }
        Rapportvy::default()
    })
    .into_inner()
}

/// Företaget i sökvägen, om den inloggade har åtkomst till det.
fn företaget() -> Result<FöretagVy, AnyView> {
    if !matches!(use_context::<Läge>(), Some(Läge::Inloggad { .. })) {
        return Err(view! { <LoggaIn /> }.into_any());
    }
    let id = use_params_map().with_untracked(|p| p.get("id").unwrap_or_default());
    let val = use_context::<Företagsval>().unwrap_or_default();
    val.0
        .into_iter()
        .find(|f| f.id == id)
        .ok_or_else(|| view! { <IngenÅtkomst /> }.into_any())
}

/// Rapporterna i grupper, med den valda markerad.
#[component]
fn Rapportmeny(bas: String, aktiv: &'static str) -> impl IntoView {
    let mut grupper: Vec<(&str, Vec<(&str, &str)>)> = Vec::new();
    for &(grupp, namn, sökväg) in RAPPORTER {
        match grupper.iter_mut().find(|(g, _)| *g == grupp) {
            Some((_, rader)) => rader.push((namn, sökväg)),
            None => grupper.push((grupp, vec![(namn, sökväg)])),
        }
    }
    view! {
        <nav class="rapportmeny" aria-label="Rapporter">
            {grupper
                .into_iter()
                .map(|(grupp, rader)| {
                    view! {
                        <h2>{grupp}</h2>
                        <ul>
                            {rader
                                .into_iter()
                                .map(|(namn, sökväg)| {
                                    let aktuell = (sökväg == aktiv).then_some("page");
                                    view! {
                                        <li>
                                            <a
                                                rel="external"
                                                href=format!("{bas}{sökväg}")
                                                aria-current=aktuell
                                            >
                                                {namn}
                                            </a>
                                        </li>
                                    }
                                })
                                .collect_view()}
                        </ul>
                    }
                })
                .collect_view()}
        </nav>
    }
}

/// Fliken Rapporter: rapporterna att välja bland och SIE4-exporten per räkenskapsår.
#[component]
pub fn Rapporter() -> impl IntoView {
    let f = match företaget() {
        Ok(f) => f,
        Err(vy) => return vy,
    };
    let stoppad = use_query_map().with_untracked(|q| q.get("fel").as_deref() == Some(SIE4_STOPPAD));
    let bas = f.href();
    let år = data()
        .år
        .into_iter()
        .map(|(start, slut)| {
            view! {
                <li>
                    <a rel="external" href=format!("{bas}/sie4?ar={start}")>
                        {format!("{start} – {slut}")}
                    </a>
                </li>
            }
        })
        .collect_view();
    view! {
        <section class="sida rapporter">
            <p class="diskret">{f.namn.clone()} " · " {f.orgnr.clone()}</p>
            <h1>"Rapporter"</h1>
            <Rapportmeny bas=bas.clone() aktiv="" />
            <h2>"SIE4"</h2>
            <p>
                "En SIE 4E-fil per räkenskapsår, för revisorn eller ett annat program. Maya läser "
                "tillbaka filen och stämmer av den mot bokföringen innan den laddas ned."
            </p>
            {stoppad
                .then(|| {
                    view! {
                        <p class="fel" role="alert">
                            "Exporten stoppades: filen stämmer inte med bokföringen. Ingen fil "
                            "laddades ned. Försök igen, och kontakta den som driver Maya om det "
                            "händer igen."
                        </p>
                    }
                })}
            <ul class="lista">{år}</ul>
        </section>
    }
    .into_any()
}

#[component]
pub fn Resultaträkning() -> impl IntoView {
    view! { <RapportSida sökväg="/rapporter/resultatrakning" /> }
}

#[component]
pub fn Balansräkning() -> impl IntoView {
    view! { <RapportSida sökväg="/rapporter/balansrakning" /> }
}

/// Resultat- eller balansräkningen: verktygsraden, banderollerna, dokumentet och panelen.
#[component]
fn RapportSida(sökväg: &'static str) -> impl IntoView {
    let f = match företaget() {
        Ok(f) => f,
        Err(vy) => return vy,
    };
    let bas = f.href();
    let d = rapportvy();
    let sida = format!("{bas}{sökväg}");
    let export = |format: &str| d.adress(&format!("{sida}/{format}"), &[]);
    let banderoller = {
        let preliminär = d.preliminär.clone().map(|(start, slut)| {
            view! {
                <div class="banderoll" role="status" data-banderoll="preliminar">
                    <p>
                        <span class="markering">"Preliminär"</span>
                        " Den ingående balansen räknas ur föregående års utgående balans, eftersom "
                        <a rel="external" href=format!("{sida}?ar={start}")>
                            {format!("{start} – {slut}")}
                        </a>
                        " inte är stängt. Resultatet som inte är ombokat står på 2099."
                    </p>
                </div>
            }
        });
        let första = d.första_året.then(|| {
            view! {
                <div class="banderoll" role="status" data-banderoll="forsta-aret">
                    <p>
                        "Räkenskapsåret är företagets första i Maya, så det finns inga jämförelsesiffror."
                    </p>
                </div>
            }
        });
        view! {
            {preliminär}
            {första}
        }
    };
    view! {
        <section class="sida rapport">
            <div class="rapport-delad">
                <Rapportmeny bas=bas.clone() aktiv=sökväg />
                <div class="rapport-innehall">
                    <p class="diskret">{f.namn.clone()} " · " {f.orgnr.clone()}</p>
                    <h1>{d.titel.clone()}</h1>
                    <Verktygsrad d=d.clone() sida=sida.clone() />
                    {banderoller}
                    <div class="rapport-export">
                        <Utskrift href=export("pdf") filnamn=format!("{}.pdf", d.filnamn) />
                        <p class="utskrift">
                            <a
                                class="exportera-csv"
                                href=export("csv")
                                download=format!("{}.csv", d.filnamn)
                                rel="external"
                            >
                                "Exportera CSV"
                            </a>
                        </p>
                    </div>
                    <Dokument d=d.clone() sida=sida.clone() namn=f.namn.clone() orgnr=f.orgnr.clone() />
                </div>
            </div>
            {d.panel.clone().map(|p| view! { <Panel d=d.clone() sida=sida.clone() bas=bas.clone() p /> })}
        </section>
    }
    .into_any()
}

/// År och period t.o.m. i ett formulär, och visning, jämförelse och nivå som länkar.
#[component]
fn Verktygsrad(d: Rapportvy, sida: String) -> impl IntoView {
    let val =
        |nyckel: &'static str, alternativ: Vec<(&'static str, &'static str)>, aktiv: String| {
            let d = d.clone();
            let sida = sida.clone();
            view! {
                <nav class="filter" data-val=nyckel>
                    {alternativ
                        .into_iter()
                        .map(|(kod, namn)| {
                            let aktuell = (kod == aktiv).then_some("page");
                            view! {
                                <a
                                    rel="external"
                                    href=d.adress(&sida, &[(nyckel, kod)])
                                    aria-current=aktuell
                                >
                                    {namn}
                                </a>
                            }
                        })
                        .collect_view()}
                </nav>
            }
        };
    let visning = (d.typ == "resultatrakning").then(|| {
        val(
            "visa",
            vec![
                ("manad", "Månad"),
                ("ackumulerat", "Ackumulerat"),
                ("bada", "Båda"),
            ],
            d.visning.clone(),
        )
    });
    let jämförelse = val(
        "jmf",
        vec![("", "Inget"), ("1", "Föregående år")],
        if d.jämförelse { "1" } else { "" }.into(),
    );
    let nivå = val(
        "niva",
        vec![
            ("rubriker", "Rubriker"),
            ("ink2", "INK2-rader"),
            ("konton", "Konton"),
        ],
        d.nivå.clone(),
    );
    view! {
        <div class="verktygsrad">
            <form method="get" action=sida.clone() class="rapporturval">
                <label>
                    "År "
                    <select name="ar" onchange="this.form.submit()">
                        {d
                            .år
                            .iter()
                            .map(|(start, slut)| {
                                view! {
                                    <option value=start.clone() selected=*start == d.valt>
                                        {format!("{start} – {slut}")}
                                    </option>
                                }
                            })
                            .collect_view()}
                    </select>
                </label>
                <label>
                    "Period t.o.m. "
                    <select name="tom" onchange="this.form.submit()">
                        {d
                            .månader
                            .iter()
                            .map(|(kod, namn)| {
                                view! {
                                    <option value=kod.clone() selected=*kod == d.tom>
                                        {namn.clone()}
                                    </option>
                                }
                            })
                            .collect_view()}
                    </select>
                </label>
                {(d.typ == "resultatrakning")
                    .then(|| view! { <input type="hidden" name="visa" value=d.visning.clone() /> })}
                {d.jämförelse.then(|| view! { <input type="hidden" name="jmf" value="1" /> })}
                <input type="hidden" name="niva" value=d.nivå.clone() />
                <button class="knapp" type="submit">
                    "Visa"
                </button>
            </form>
            {visning}
            <span class="diskret">"Jämför"</span>
            {jämförelse}
            <span class="diskret">"Nivå"</span>
            {nivå}
        </div>
    }
}

/// Rapporten som dokument. Ett konto öppnar panelen; en rubrik eller INK2-rad visar nivån under.
#[component]
fn Dokument(d: Rapportvy, sida: String, namn: String, orgnr: String) -> impl IntoView {
    let rader = d
        .rader
        .iter()
        .map(|r| {
            let text = match (r.konto, &r.ink2) {
                (Some(konto), _) => view! {
                    <a
                        rel="external"
                        href=d.adress(&sida, &[("konto", &konto.to_string())])
                    >
                        {format!("{konto} {}", r.text)}
                    </a>
                }
                .into_any(),
                (None, Some(ink2)) if r.slag == "rad" => view! {
                    <a rel="external" href=d.adress(&sida, &[("niva", "konton")])>
                        <span class="ink2">{ink2.clone()}</span>
                        {r.text.clone()}
                    </a>
                }
                .into_any(),
                _ if r.slag == "rubrik" && !r.värden.is_empty() => view! {
                    <a rel="external" href=d.adress(&sida, &[("niva", "ink2")])>
                        {r.text.clone()}
                    </a>
                }
                .into_any(),
                _ => r.text.clone().into_any(),
            };
            let värden = if r.värden.is_empty() {
                d.kolumner
                    .iter()
                    .map(|_| view! { <td></td> }.into_any())
                    .collect_view()
            } else {
                r.värden
                    .iter()
                    .zip(&d.kolumner)
                    .map(|(v, k)| {
                        view! {
                            <td class=if k.föregående { "belopp foregaende" } else { "belopp" }>
                                {v.clone().unwrap_or_else(|| "–".into())}
                            </td>
                        }
                        .into_any()
                    })
                    .collect_view()
            };
            view! {
                <tr class=r.slag.clone()>
                    <td>{text}</td>
                    {värden}
                </tr>
            }
        })
        .collect_view();
    view! {
        <div class="rapportdokument">
            <div class="rapportdokument-huvud">
                <div>
                    <div class="rapportdokument-titel">{d.titel.clone()}</div>
                    <div class="diskret">{namn} " · " {orgnr}</div>
                </div>
                <div class="diskret hoger">
                    "Period " {d.period.clone()} <br /> "Belopp i kronor · bara bokfört"
                </div>
            </div>
            <table class="rapporttabell">
                <thead>
                    <tr>
                        <th></th>
                        {d
                            .kolumner
                            .iter()
                            .map(|k| {
                                view! {
                                    <th class=if k.föregående { "belopp foregaende" } else { "belopp" }>
                                        {k.rubrik.clone()}
                                        <span class="underrubrik">{k.underrubrik.clone()}</span>
                                    </th>
                                }
                            })
                            .collect_view()}
                    </tr>
                </thead>
                <tbody>{rader}</tbody>
            </table>
            <p class="diskret">
                "Klicka på ett konto för att se verifikaten. Grupperingen följer INK2-kopplingstabellen."
            </p>
        </div>
    }
}

/// Kontots verifikat i perioden med löpande saldo, och länk till varje verifikat.
#[component]
fn Panel(d: Rapportvy, sida: String, bas: String, p: PanelVy) -> impl IntoView {
    let stäng = d.adress(&sida, &[]);
    view! {
        <aside class="sidopanel" aria-labelledby="panelrubrik">
            <div class="panelhuvud">
                <h2 id="panelrubrik">{format!("{} {}", p.konto, p.namn)}</h2>
                <a class="knapp knapp-diskret" rel="external" href=stäng aria-label="Stäng">
                    "×"
                </a>
            </div>
            <p class="diskret">{p.period.clone()} " · debet och kredit, löpande saldo"</p>
            <table class="rader kontoutdrag">
                <thead>
                    <tr>
                        <th>"Datum"</th>
                        <th>"Ver"</th>
                        <th>"Text"</th>
                        <th class="belopp">"Debet"</th>
                        <th class="belopp">"Kredit"</th>
                        <th class="belopp">"Saldo"</th>
                    </tr>
                </thead>
                <tbody>
                    {p
                        .ingående
                        .clone()
                        .map(|(datum, saldo)| {
                            view! {
                                <tr class="ingaende">
                                    <td>{datum}</td>
                                    <td></td>
                                    <td>"Ingående balans"</td>
                                    <td></td>
                                    <td></td>
                                    <td class="belopp">{saldo}</td>
                                </tr>
                            }
                        })}
                    {p
                        .rader
                        .iter()
                        .map(|r| {
                            view! {
                                <tr>
                                    <td>{r.datum.clone()}</td>
                                    <td>
                                        <a
                                            rel="external"
                                            href=format!("{bas}/verifikat?v={}", r.verifikat)
                                        >
                                            {r.nummer.clone()}
                                        </a>
                                    </td>
                                    <td>{r.text.clone()}</td>
                                    <td class="belopp">{r.debet.clone()}</td>
                                    <td class="belopp">{r.kredit.clone()}</td>
                                    <td class="belopp">{r.saldo.clone()}</td>
                                </tr>
                            }
                        })
                        .collect_view()}
                </tbody>
                <tfoot>
                    <tr>
                        <td colspan="5">
                            {if p.ingående.is_some() { "Utgående balans" } else { "Summa" }}
                        </td>
                        <td class="belopp">{p.utgående.clone()}</td>
                    </tr>
                </tfoot>
            </table>
            {p.rader.is_empty().then(|| view! { <p class="diskret">"Inga verifikat i perioden."</p> })}
        </aside>
    }
}

#[cfg(feature = "ssr")]
pub(crate) mod server {
    use std::sync::Arc;

    use axum::Router;
    use axum::extract::{Extension, Path, Query, RawQuery, Request, State};
    use axum::http::{StatusCode, header};
    use axum::middleware::Next;
    use axum::response::{IntoResponse, Redirect, Response};
    use axum::routing::get;
    use jiff::civil::Date;
    use maya_core::atkomst::ÅtkomstFel;
    use maya_core::foretag::{self, FöretagsId};
    use maya_core::inloggning::Inloggad;
    use maya_core::rapport::{self, Urval};
    use maya_core::saldo::SaldoFel;
    use maya_core::sie4::{self, Sie4Fel};
    use maya_core::utskrift::{self, UtskriftFel};
    use maya_core::{Databas, Klocka};
    use maya_domain::belopp::{Belopp, Saldo};
    use maya_domain::kontoplan::Kontonummer;
    use maya_domain::rakenskapsar::periodnamn;
    use maya_domain::rapport::{Nivå, Radslag, Rapporttyp, Visning, periodspann};
    use serde::Deserialize;

    use super::{KolumnVy, PanelVy, PanelradVy, RadVy, Rapportdata, Rapportvy, SIE4_STOPPAD};

    #[derive(Clone)]
    pub(crate) struct RapportState {
        pub db: Databas,
        pub klocka: Klocka,
    }

    pub(crate) fn routes(state: RapportState) -> Router {
        let mut router = Router::new().route("/foretag/{id}/sie4", get(exportera_sie4));
        for typ in Rapporttyp::ALLA {
            router = router
                .route(
                    &format!("/foretag/{{id}}/rapporter/{}/pdf", typ.kod()),
                    get(move |s, p, i, q| pdf(s, p, i, q, typ)),
                )
                .route(
                    &format!("/foretag/{{id}}/rapporter/{}/csv", typ.kod()),
                    get(move |s, p, i, q| csv(s, p, i, q, typ)),
                );
        }
        router.with_state(Arc::new(state))
    }

    /// Hämtar räkenskapsåren för fliken `/foretag/<id>/rapporter`. Körs efter
    /// `inloggning::identifiera`.
    pub(crate) async fn lägg_på_rapportdata(
        State(state): State<RapportState>,
        mut req: Request,
        next: Next,
    ) -> Response {
        let Some(inloggad) = req.extensions().get::<Inloggad>().cloned() else {
            return next.run(req).await;
        };
        let delar: Vec<&str> = req.uri().path().trim_matches('/').split('/').collect();
        let ["foretag", id, "rapporter"] = delar.as_slice() else {
            return next.run(req).await;
        };
        let Ok(företag) = id.parse::<FöretagsId>() else {
            return next.run(req).await;
        };
        match foretag::företaget(&state.db, &inloggad.aktör, företag).await {
            Ok(f) => {
                req.extensions_mut().insert(Rapportdata {
                    år: f
                        .räkenskapsår
                        .iter()
                        .map(|å| (å.start.to_string(), å.slut.to_string()))
                        .collect(),
                });
            }
            // Sidan visar att användaren saknar åtkomst, ur företagsvalet.
            Err(ÅtkomstFel::IngenÅtkomst | ÅtkomstFel::KanalenFårInte) => {}
            Err(e) => return serverfel(e),
        }
        next.run(req).await
    }

    #[derive(Deserialize)]
    struct Årsval {
        #[serde(default)]
        ar: String,
    }

    /// SIE4-filen för räkenskapsåret som börjar `ar`, som en bilaga.
    async fn exportera_sie4(
        State(state): State<Arc<RapportState>>,
        Path(id): Path<String>,
        Query(urval): Query<Årsval>,
        inloggad: Option<Extension<Inloggad>>,
    ) -> Response {
        let (Some(Extension(inloggad)), Ok(företag)) = (inloggad, id.parse::<FöretagsId>())
        else {
            return ingen_åtkomst();
        };
        let Ok(år) = urval.ar.parse::<Date>() else {
            return okänt_år();
        };
        match sie4::exportera(&state.db, &state.klocka, &inloggad.aktör, företag, år).await {
            Ok(export) => (
                StatusCode::OK,
                [
                    (
                        header::CONTENT_TYPE,
                        "text/plain; charset=IBM437".to_string(),
                    ),
                    (
                        header::CONTENT_DISPOSITION,
                        format!("attachment; filename=\"{}\"", export.filnamn),
                    ),
                    (header::CACHE_CONTROL, "no-store".to_string()),
                ],
                export.innehåll,
            )
                .into_response(),
            Err(e @ (Sie4Fel::Avvikelse(_) | Sie4Fel::Återläsning(_))) => {
                tracing::error!("SIE4-exporten stoppades: {e}");
                Redirect::to(&format!("/foretag/{företag}/rapporter?fel={SIE4_STOPPAD}"))
                    .into_response()
            }
            Err(Sie4Fel::OkäntRäkenskapsår) => okänt_år(),
            Err(Sie4Fel::Åtkomst(ÅtkomstFel::IngenÅtkomst | ÅtkomstFel::KanalenFårInte)) => {
                ingen_åtkomst()
            }
            Err(e) => serverfel(e),
        }
    }

    fn okänt_år() -> Response {
        text(
            StatusCode::NOT_FOUND,
            "Företaget har inget sådant räkenskapsår.\n",
        )
    }

    /// Urvalet i adressen, med räkenskapsåret och kontot för panelen. Saknas eller är något
    /// ogiltigt gäller året som pågår, perioden fram till i dag, båda kolumnerna, ingen
    /// jämförelse och INK2-raderna.
    struct Valt {
        år: Date,
        urval: Urval,
        konto: Option<Kontonummer>,
    }

    fn välj(
        typ: Rapporttyp,
        query: Option<&str>,
        år: &[(Date, Date)],
        idag: Date,
    ) -> Option<Valt> {
        let fält: Vec<(String, String)> =
            url::form_urlencoded::parse(query.unwrap_or_default().as_bytes())
                .into_owned()
                .collect();
        let hämta = |k: &str| fält.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
        let pågående = år
            .iter()
            .find(|(s, e)| *s <= idag && idag <= *e)
            .or_else(|| år.iter().rev().find(|(s, _)| *s <= idag))
            .or(år.first())?;
        let (start, slut) = hämta("ar")
            .and_then(|a| a.parse::<Date>().ok())
            .and_then(|a| år.iter().find(|(s, _)| *s == a))
            .unwrap_or(pågående);
        let till_och_med = hämta("tom")
            .and_then(|m| format!("{m}-01").parse::<Date>().ok())
            .filter(|m| m.last_of_month() >= *start && *m <= *slut)
            .unwrap_or_else(|| idag.clamp(*start, *slut));
        Some(Valt {
            år: *start,
            urval: Urval {
                typ,
                till_och_med,
                visning: hämta("visa")
                    .and_then(Visning::från_kod)
                    .unwrap_or_default(),
                jämförelse: hämta("jmf") == Some("1"),
                nivå: hämta("niva").and_then(Nivå::från_kod).unwrap_or_default(),
            },
            konto: hämta("konto")
                .and_then(|k| k.parse::<u16>().ok())
                .and_then(Kontonummer::new),
        })
    }

    async fn räkenskapsåren(
        db: &Databas,
        inloggad: &Inloggad,
        företag: FöretagsId,
    ) -> Result<Vec<(Date, Date)>, ÅtkomstFel> {
        Ok(foretag::företaget(db, &inloggad.aktör, företag)
            .await?
            .räkenskapsår
            .iter()
            .map(|å| (å.start, å.slut))
            .collect())
    }

    /// Hämtar rapporten för `/foretag/<id>/rapporter/<resultatrakning|balansrakning>`. Körs
    /// efter `inloggning::identifiera`.
    pub(crate) async fn lägg_på_rapport(
        State(state): State<RapportState>,
        mut req: Request,
        next: Next,
    ) -> Response {
        let Some(inloggad) = req.extensions().get::<Inloggad>().cloned() else {
            return next.run(req).await;
        };
        let delar: Vec<&str> = req.uri().path().trim_matches('/').split('/').collect();
        let ["foretag", id, "rapporter", kod] = delar.as_slice() else {
            return next.run(req).await;
        };
        let (Ok(företag), Some(typ)) = (id.parse::<FöretagsId>(), Rapporttyp::från_kod(kod))
        else {
            return next.run(req).await;
        };
        let query = req.uri().query().map(str::to_string);
        match hämta(&state, &inloggad, företag, typ, query.as_deref()).await {
            Ok(Some(data)) => {
                req.extensions_mut().insert(data);
            }
            Ok(None) => {}
            // Sidan visar att användaren saknar åtkomst, ur företagsvalet.
            Err(SaldoFel::Åtkomst(ÅtkomstFel::IngenÅtkomst | ÅtkomstFel::KanalenFårInte)) => {}
            Err(e) => return serverfel(e),
        }
        next.run(req).await
    }

    fn kronor(öre: i64) -> String {
        Saldo::från_öre(öre).to_string()
    }

    fn belopp(b: Belopp) -> String {
        if b == Belopp::NOLL {
            String::new()
        } else {
            b.to_string()
        }
    }

    fn slag(s: Radslag) -> &'static str {
        match s {
            Radslag::Sida => "sida",
            Radslag::Rubrik => "rubrik",
            Radslag::Underrubrik => "underrubrik",
            Radslag::Ink2Rad => "rad",
            Radslag::Konto => "konto",
            Radslag::Summa => "summa",
            Radslag::Delsumma => "delsumma",
            Radslag::Slutsumma => "slutsumma",
            Radslag::ÅretsResultat => "arets-resultat",
            Radslag::Kontroll => "kontroll",
        }
    }

    async fn hämta(
        state: &RapportState,
        inloggad: &Inloggad,
        företag: FöretagsId,
        typ: Rapporttyp,
        query: Option<&str>,
    ) -> Result<Option<Rapportvy>, SaldoFel> {
        let db = &state.db;
        let aktör = &inloggad.aktör;
        let år = räkenskapsåren(db, inloggad, företag).await?;
        let Some(valt) = välj(typ, query, &år, state.klocka.idag()) else {
            return Ok(None);
        };
        let r = rapport::rapport(db, aktör, företag, valt.år, &valt.urval).await?;
        let orgnr = foretag::företaget(db, aktör, företag)
            .await?
            .orgnr
            .to_string();
        let panel = match valt.konto {
            None => None,
            Some(konto) => {
                let u =
                    rapport::kontoutdrag(db, aktör, företag, valt.år, konto, &valt.urval).await?;
                Some(PanelVy {
                    konto: konto.get(),
                    namn: u.kontonamn,
                    period: periodspann(u.från, u.till),
                    ingående: u.ingående.map(|s| (u.från.to_string(), s.to_string())),
                    rader: u
                        .rader
                        .into_iter()
                        .map(|r| PanelradVy {
                            verifikat: r.verifikat.to_string(),
                            nummer: r.nummer.to_string(),
                            datum: r.datum.to_string(),
                            text: r.text.unwrap_or(r.beskrivning),
                            debet: belopp(r.debet),
                            kredit: belopp(r.kredit),
                            saldo: r.saldo.to_string(),
                        })
                        .collect(),
                    utgående: u.utgående.to_string(),
                })
            }
        };
        let (start, slut) = år
            .iter()
            .find(|(s, _)| *s == valt.år)
            .copied()
            .expect("året finns");
        let mut månader = Vec::new();
        let mut m = start.first_of_month();
        while m <= slut {
            månader.push((m.strftime("%Y-%m").to_string(), periodnamn(m)));
            m = m.last_of_month().tomorrow().expect("en månad efter året");
        }
        let föregående = år
            .iter()
            .take_while(|(s, _)| *s != valt.år)
            .last()
            .map(|(s, e)| (s.to_string(), e.to_string()));
        Ok(Some(Rapportvy {
            typ: typ.kod().into(),
            titel: typ.namn().into(),
            år: år
                .iter()
                .map(|(s, e)| (s.to_string(), e.to_string()))
                .collect(),
            valt: valt.år.to_string(),
            månader,
            tom: r.till.strftime("%Y-%m").to_string(),
            visning: valt.urval.visning.kod().into(),
            jämförelse: valt.urval.jämförelse,
            nivå: valt.urval.nivå.kod().into(),
            period: r.period(),
            preliminär: r
                .preliminär_ingående_balans()
                .then_some(föregående)
                .flatten(),
            första_året: r.första_året(),
            kolumner: r
                .kolumner
                .iter()
                .map(|k| KolumnVy {
                    rubrik: k.rubrik.clone(),
                    underrubrik: k.underrubrik.clone(),
                    föregående: k.föregående,
                })
                .collect(),
            rader: r
                .rader
                .iter()
                .map(|rad| RadVy {
                    slag: slag(rad.slag).into(),
                    text: rad.text.clone(),
                    ink2: rad.ink2.clone(),
                    konto: rad.konto.map(Kontonummer::get),
                    värden: rad.värden.iter().map(|v| v.map(kronor)).collect(),
                })
                .collect(),
            panel,
            filnamn: r.filnamn(&orgnr),
        }))
    }

    async fn pdf(
        State(state): State<Arc<RapportState>>,
        Path(id): Path<String>,
        inloggad: Option<Extension<Inloggad>>,
        RawQuery(query): RawQuery,
        typ: Rapporttyp,
    ) -> Response {
        let Some(Extension(inloggad)) = inloggad else {
            return Redirect::to("/").into_response();
        };
        let Ok(företag) = id.parse::<FöretagsId>() else {
            return ingen_åtkomst();
        };
        let år = match räkenskapsåren(&state.db, &inloggad, företag).await {
            Ok(år) => år,
            Err(e) => return åtkomstfel(e),
        };
        let Some(valt) = välj(typ, query.as_deref(), &år, state.klocka.idag()) else {
            return okänt_år();
        };
        match utskrift::rapport(
            &state.db,
            &state.klocka,
            &inloggad.aktör,
            företag,
            valt.år,
            &valt.urval,
        )
        .await
        {
            Ok(pdf) => crate::utskrift::server::svar(pdf),
            Err(UtskriftFel::Åtkomst(e)) => åtkomstfel(e),
            Err(UtskriftFel::OkäntRäkenskapsår) => okänt_år(),
            Err(e) => serverfel(e),
        }
    }

    async fn csv(
        State(state): State<Arc<RapportState>>,
        Path(id): Path<String>,
        inloggad: Option<Extension<Inloggad>>,
        RawQuery(query): RawQuery,
        typ: Rapporttyp,
    ) -> Response {
        let Some(Extension(inloggad)) = inloggad else {
            return Redirect::to("/").into_response();
        };
        let Ok(företag) = id.parse::<FöretagsId>() else {
            return ingen_åtkomst();
        };
        let år = match räkenskapsåren(&state.db, &inloggad, företag).await {
            Ok(år) => år,
            Err(e) => return åtkomstfel(e),
        };
        let Some(valt) = välj(typ, query.as_deref(), &år, state.klocka.idag()) else {
            return okänt_år();
        };
        match rapport::csv(&state.db, &inloggad.aktör, företag, valt.år, &valt.urval).await {
            Ok(csv) => (
                [
                    (header::CONTENT_TYPE, "text/csv; charset=utf-8".to_string()),
                    (
                        header::CONTENT_DISPOSITION,
                        format!(r#"attachment; filename="{}""#, csv.filnamn),
                    ),
                    (header::CACHE_CONTROL, "no-store".to_string()),
                ],
                csv.innehåll,
            )
                .into_response(),
            Err(SaldoFel::Åtkomst(e)) => åtkomstfel(e),
            Err(SaldoFel::OkäntRäkenskapsår) => okänt_år(),
        }
    }

    fn åtkomstfel(e: ÅtkomstFel) -> Response {
        match e {
            ÅtkomstFel::IngenÅtkomst | ÅtkomstFel::KanalenFårInte => ingen_åtkomst(),
            e => serverfel(e),
        }
    }

    fn ingen_åtkomst() -> Response {
        text(
            StatusCode::FORBIDDEN,
            "Du har inte åtkomst till företaget.\n",
        )
    }

    fn serverfel(e: impl std::fmt::Display) -> Response {
        tracing::error!("rapporter: {e}");
        text(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Något gick fel i Maya; försök igen.\n",
        )
    }

    fn text(status: StatusCode, text: &'static str) -> Response {
        (
            status,
            [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
            text,
        )
            .into_response()
    }
}
