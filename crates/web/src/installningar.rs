//! Inställningar bakom kugghjulet. Menyn till vänster är gemensam för inställningssidorna;
//! här finns sidan Företag: uppgifterna, byte av bokföringsmetod och momsperiod, första
//! räkenskapsårets slut och radering av företaget.
//!
//! Formulären prövas med `maya_domain::foretag` i webbläsaren och av `core` på servern, med
//! samma koder. Utan JavaScript skickas de till servern, som skickar tillbaka hit med det
//! användaren skrev; sidan prövar det igen och visar felen vid fälten.

use jiff::civil::Date;
use leptos::prelude::*;
use leptos_router::hooks::{use_params_map, use_query_map};
use maya_domain::foretag::{
    Bokföringsmetod, Byte, Fält, Fältfel, Momsperiod, Uppgifter, kontrollera_första_årets_slut,
    Ändringsläge,
};
use maya_domain::rakenskapsar::Räkenskapsår;
use serde::{Deserialize, Serialize};

use crate::app::{LoggaIn, Läge};
use crate::foretag::{Företagsval, IngenÅtkomst};

/// Koden när bekräftelsen av raderingen inte är företagets organisationsnummer.
pub const BEKRÄFTELSE_FEL: &str = "bekraftelse_fel";
/// Koden när företaget har räkenskapsinformation och inte kan raderas. Bara servern ser det
/// säkert.
pub const HAR_RÄKENSKAPSINFORMATION: &str = "har_rakenskapsinformation";

/// Inställningssidorna i menyn, i ordning: sökvägen under `/foretag/<id>/installningar` och
/// namnet. Åtkomst, Behandlingshistorik och Systemdokumentation läggs till med sina tickets.
const SIDOR: [(&str, &str); 2] = [("", "Företag"), ("/kontoplan", "Kontoplan")];

/// Menyn till vänster på inställningssidorna. `aktiv` är sidans sökväg i [`SIDOR`].
#[component]
pub fn Inställningsmeny(bas: String, aktiv: &'static str) -> impl IntoView {
    view! {
        <nav class="installningsmeny" aria-label="Inställningar">
            {SIDOR
                .into_iter()
                .map(|(sökväg, namn)| {
                    let aktuell = (sökväg == aktiv).then_some("page");
                    view! {
                        <a href=format!("{bas}/installningar{sökväg}") aria-current=aktuell>
                            {namn}
                        </a>
                    }
                })
                .collect_view()}
        </nav>
    }
}

/// Företagets uppgifter som sidan visar.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UppgifterVy {
    pub namn: String,
    pub gatuadress: String,
    pub postnummer: String,
    pub postort: String,
    pub epost: String,
    pub telefon: String,
    pub säte: String,
    pub momsregnr: String,
    pub f_skatt: bool,
}

impl From<&UppgifterVy> for Uppgifter {
    fn from(u: &UppgifterVy) -> Self {
        Uppgifter {
            namn: u.namn.clone(),
            gatuadress: u.gatuadress.clone(),
            postnummer: u.postnummer.clone(),
            postort: u.postort.clone(),
            epost: u.epost.clone(),
            telefon: u.telefon.clone(),
            säte: u.säte.clone(),
            momsregnr: u.momsregnr.clone(),
            f_skatt: u.f_skatt,
        }
    }
}

/// Det som sidan Företag visar. Middleware hämtar det ur `core` och lägger det på begäran, och
/// det följer med till webbläsaren vid hydreringen.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Inställningsdata {
    pub orgnr: String,
    pub föreslaget_momsregnr: String,
    pub uppgifter: UppgifterVy,
    /// Start och slut, `ÅÅÅÅ-MM-DD`.
    pub räkenskapsår: Vec<(String, String)>,
    /// Gäller från och kod, äldst först.
    pub bokföringsmetoder: Vec<(String, String)>,
    /// Gäller från och kod, äldst först.
    pub momsperioder: Vec<(String, String)>,
    pub senast_bokförda: Option<String>,
    pub senast_daterade: Option<String>,
    pub kan_raderas: bool,
}

impl Inställningsdata {
    fn läge(&self) -> Ändringsläge {
        let datum = |d: &str| d.parse::<Date>().ok();
        Ändringsläge {
            räkenskapsår: self
                .räkenskapsår
                .iter()
                .filter_map(|(s, e)| Some(Räkenskapsår::befintligt(datum(s)?, datum(e)?)))
                .collect(),
            bokföringsmetoder: self
                .bokföringsmetoder
                .iter()
                .filter_map(|(d, k)| Some((datum(d)?, Bokföringsmetod::från_kod(k)?)))
                .collect(),
            momsperioder: self
                .momsperioder
                .iter()
                .filter_map(|(d, k)| Some((datum(d)?, Momsperiod::från_kod(k)?)))
                .collect(),
            senast_bokförda: self.senast_bokförda.as_deref().and_then(datum),
            senast_daterade: self.senast_daterade.as_deref().and_then(datum),
        }
    }
}

fn inställningsdata() -> Option<Inställningsdata> {
    SharedValue::new(|| {
        #[cfg(feature = "ssr")]
        if let Some(parts) = use_context::<axum::http::request::Parts>() {
            return parts.extensions.get::<Inställningsdata>().cloned();
        }
        None
    })
    .into_inner()
}

#[cfg(feature = "ssr")]
pub(crate) mod server {
    use std::sync::Arc;

    use axum::extract::{Path, Request, State};
    use axum::http::{StatusCode, header};
    use axum::middleware::Next;
    use axum::response::{IntoResponse, Redirect, Response};
    use axum::routing::post;
    use axum::{Extension, Form, Router};
    use maya_core::atkomst::ÅtkomstFel;
    use maya_core::foretag::{self, FöretagsFel, FöretagsId};
    use maya_core::inloggning::Inloggad;
    use maya_core::{Databas, Klocka};
    use maya_domain::foretag::{Byte, Uppgifter, föreslaget_momsregnr};

    use super::{BEKRÄFTELSE_FEL, HAR_RÄKENSKAPSINFORMATION, Inställningsdata, UppgifterVy};

    #[derive(Clone)]
    pub(crate) struct InställningsState {
        pub db: Databas,
        pub klocka: Klocka,
    }

    pub(crate) fn routes(state: InställningsState) -> Router {
        Router::new()
            .route("/foretag/{id}/installningar/uppgifter", post(uppgifter))
            .route("/foretag/{id}/installningar/bokforingsmetod", post(metod))
            .route("/foretag/{id}/installningar/momsperiod", post(moms))
            .route("/foretag/{id}/installningar/rakenskapsar", post(år))
            .route("/foretag/{id}/radera", post(radera))
            .with_state(Arc::new(state))
    }

    /// Hämtar det som sidan Företag under Inställningar visar. Körs efter
    /// `inloggning::identifiera`.
    pub(crate) async fn lägg_på_inställningar(
        State(state): State<InställningsState>,
        mut req: Request,
        next: Next,
    ) -> Response {
        let Some(inloggad) = req.extensions().get::<Inloggad>().cloned() else {
            return next.run(req).await;
        };
        let delar: Vec<&str> = req.uri().path().trim_matches('/').split('/').collect();
        let ["foretag", id, "installningar"] = delar.as_slice() else {
            return next.run(req).await;
        };
        let Ok(id) = id.parse::<FöretagsId>() else {
            return next.run(req).await;
        };
        match hämta(&state.db, &inloggad, id).await {
            Ok(data) => {
                req.extensions_mut().insert(data);
            }
            // Sidan visar att användaren saknar åtkomst, ur företagsvalet.
            Err(ÅtkomstFel::IngenÅtkomst | ÅtkomstFel::KanalenFårInte) => {}
            Err(e) => return serverfel(e),
        }
        next.run(req).await
    }

    async fn hämta(
        db: &Databas,
        inloggad: &Inloggad,
        id: FöretagsId,
    ) -> Result<Inställningsdata, ÅtkomstFel> {
        let aktör = &inloggad.aktör;
        let f = foretag::företaget(db, aktör, id).await?;
        let läge = foretag::ändringsläge(db, aktör, id).await?;
        let kan_raderas = !foretag::har_räkenskapsinformation(db, aktör, id).await?;
        let u = f.uppgifter;
        Ok(Inställningsdata {
            orgnr: f.orgnr.to_string(),
            föreslaget_momsregnr: föreslaget_momsregnr(&f.orgnr),
            uppgifter: UppgifterVy {
                namn: u.namn,
                gatuadress: u.gatuadress,
                postnummer: u.postnummer,
                postort: u.postort,
                epost: u.epost,
                telefon: u.telefon,
                säte: u.säte,
                momsregnr: u.momsregnr,
                f_skatt: u.f_skatt,
            },
            räkenskapsår: f
                .räkenskapsår
                .iter()
                .map(|å| (å.start.to_string(), å.slut.to_string()))
                .collect(),
            bokföringsmetoder: f
                .bokföringsmetoder
                .iter()
                .map(|(d, m)| (d.to_string(), m.kod().to_string()))
                .collect(),
            momsperioder: f
                .momsperioder
                .iter()
                .map(|(d, m)| (d.to_string(), m.kod().to_string()))
                .collect(),
            senast_bokförda: läge.senast_bokförda.map(|d| d.to_string()),
            senast_daterade: läge.senast_daterade.map(|d| d.to_string()),
            kan_raderas,
        })
    }

    type Fält = Vec<(String, String)>;

    fn värde(fält: &Fält, namn: &str) -> String {
        fält
            .iter()
            .find(|(k, _)| k == namn)
            .map(|(_, v)| v.clone())
            .unwrap_or_default()
    }

    /// Kör ett användningsfall för företaget i sökvägen och svarar med en omdirigering: till
    /// sidan med `sparat=<form>` när det gick, och annars tillbaka med det användaren skrev.
    async fn svara<F>(
        id: &str,
        inloggad: Option<Extension<Inloggad>>,
        form: &str,
        fält: &Fält,
        kör: impl FnOnce(Inloggad, FöretagsId) -> F,
    ) -> Response
    where
        F: Future<Output = Result<(), FöretagsFel>>,
    {
        let Some(Extension(inloggad)) = inloggad else {
            return Redirect::to("/").into_response();
        };
        let Ok(företag) = id.parse::<FöretagsId>() else {
            return ingen_åtkomst();
        };
        match kör(inloggad, företag).await {
            Ok(()) => Redirect::to(&format!("/foretag/{företag}/installningar?sparat={form}"))
                .into_response(),
            Err(FöretagsFel::Ogiltigt(_)) => tillbaka(företag, form, fält, None),
            Err(FöretagsFel::HarRäkenskapsinformation) => {
                tillbaka(företag, form, fält, Some(HAR_RÄKENSKAPSINFORMATION))
            }
            Err(FöretagsFel::Åtkomst(ÅtkomstFel::IngenÅtkomst | ÅtkomstFel::KanalenFårInte)) => {
                ingen_åtkomst()
            }
            Err(e) => serverfel(e),
        }
    }

    async fn uppgifter(
        State(state): State<Arc<InställningsState>>,
        Path(id): Path<String>,
        inloggad: Option<Extension<Inloggad>>,
        Form(fält): Form<Fält>,
    ) -> Response {
        let nya = Uppgifter {
            namn: värde(&fält, "namn"),
            gatuadress: värde(&fält, "gatuadress"),
            postnummer: värde(&fält, "postnummer"),
            postort: värde(&fält, "postort"),
            epost: värde(&fält, "epost"),
            telefon: värde(&fält, "telefon"),
            säte: värde(&fält, "sate"),
            momsregnr: värde(&fält, "momsregnr"),
            f_skatt: fält.iter().any(|(k, _)| k == "f_skatt"),
        };
        svara(
            &id,
            inloggad,
            "uppgifter",
            &fält,
            |inloggad, företag| async move {
                foretag::ändra_uppgifter(&state.db, &state.klocka, &inloggad.aktör, företag, &nya)
                    .await
            },
        )
        .await
    }

    async fn metod(
        State(state): State<Arc<InställningsState>>,
        Path(id): Path<String>,
        inloggad: Option<Extension<Inloggad>>,
        Form(fält): Form<Fält>,
    ) -> Response {
        let byte = Byte {
            gäller_från: värde(&fält, "galler_fran"),
            val: värde(&fält, "bokforingsmetod"),
        };
        svara(
            &id,
            inloggad,
            "bokforingsmetod",
            &fält,
            |inloggad, företag| async move {
                foretag::byt_bokföringsmetod(
                    &state.db,
                    &state.klocka,
                    &inloggad.aktör,
                    företag,
                    &byte,
                )
                .await
            },
        )
        .await
    }

    async fn moms(
        State(state): State<Arc<InställningsState>>,
        Path(id): Path<String>,
        inloggad: Option<Extension<Inloggad>>,
        Form(fält): Form<Fält>,
    ) -> Response {
        let byte = Byte {
            gäller_från: värde(&fält, "galler_fran"),
            val: värde(&fält, "momsperiod"),
        };
        svara(
            &id,
            inloggad,
            "momsperiod",
            &fält,
            |inloggad, företag| async move {
                foretag::byt_momsperiod(&state.db, &state.klocka, &inloggad.aktör, företag, &byte)
                    .await
            },
        )
        .await
    }

    async fn år(
        State(state): State<Arc<InställningsState>>,
        Path(id): Path<String>,
        inloggad: Option<Extension<Inloggad>>,
        Form(fält): Form<Fält>,
    ) -> Response {
        let slut = värde(&fält, "slut");
        svara(
            &id,
            inloggad,
            "rakenskapsar",
            &fält,
            |inloggad, företag| async move {
                foretag::ändra_första_årets_slut(
                    &state.db,
                    &state.klocka,
                    &inloggad.aktör,
                    företag,
                    &slut,
                )
                .await
            },
        )
        .await
    }

    /// Raderar företaget när bekräftelsen är dess organisationsnummer, och går till
    /// startsidan.
    async fn radera(
        State(state): State<Arc<InställningsState>>,
        Path(id): Path<String>,
        inloggad: Option<Extension<Inloggad>>,
        Form(fält): Form<Fält>,
    ) -> Response {
        let Some(Extension(inloggad)) = inloggad else {
            return Redirect::to("/").into_response();
        };
        let Ok(företag) = id.parse::<FöretagsId>() else {
            return ingen_åtkomst();
        };
        let orgnr = match foretag::företaget(&state.db, &inloggad.aktör, företag).await {
            Ok(f) => f.orgnr,
            Err(ÅtkomstFel::IngenÅtkomst | ÅtkomstFel::KanalenFårInte) => {
                return ingen_åtkomst();
            }
            Err(e) => return serverfel(e),
        };
        if !super::bekräftar(&värde(&fält, "bekrafta"), orgnr.siffror()) {
            return tillbaka(företag, "radera", &fält, Some(BEKRÄFTELSE_FEL));
        }
        match foretag::radera(&state.db, &state.klocka, &inloggad.aktör, företag).await {
            Ok(()) => Redirect::to("/").into_response(),
            Err(FöretagsFel::HarRäkenskapsinformation) => {
                tillbaka(företag, "radera", &fält, Some(HAR_RÄKENSKAPSINFORMATION))
            }
            Err(FöretagsFel::Åtkomst(ÅtkomstFel::IngenÅtkomst | ÅtkomstFel::KanalenFårInte)) => {
                ingen_åtkomst()
            }
            Err(e) => serverfel(e),
        }
    }

    /// Tillbaka till sidan med formulärets fält; sidan prövar dem igen med samma regler.
    /// `fel` bär det som bara servern kan se.
    fn tillbaka(företag: FöretagsId, form: &str, fält: &Fält, fel: Option<&str>) -> Response {
        let mut query = url::form_urlencoded::Serializer::new(String::new());
        query.append_pair("form", form);
        for (k, v) in fält {
            if !matches!(k.as_str(), "form" | "fel" | "sparat") {
                query.append_pair(k, v);
            }
        }
        if let Some(fel) = fel {
            query.append_pair("fel", fel);
        }
        Redirect::to(&format!(
            "/foretag/{företag}/installningar?{}",
            query.finish()
        ))
        .into_response()
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
        tracing::error!("inställningar: {e}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
            "Något gick fel i Maya; försök igen.\n",
        )
            .into_response()
    }
}

/// Om `text` är organisationsnumret med siffrorna `siffror`, med eller utan bindestreck.
fn bekräftar(text: &str, siffror: &str) -> bool {
    let skrivet: String = text.chars().filter(char::is_ascii_digit).collect();
    !siffror.is_empty() && skrivet == siffror
}

/// Ett fält i formuläret som kom tillbaka från servern, eller `None` för en vanlig visning.
fn fråga() -> (Option<String>, impl Fn(&str) -> Option<String> + Clone) {
    let q = use_query_map().get_untracked();
    let form = q.get("form");
    (form, move |namn: &str| q.get(namn))
}

/// Felen vid ett fält, när de ska visas.
fn fel_vid(
    fel: Memo<Vec<Fältfel>>,
    visa: RwSignal<bool>,
    fält: &'static str,
) -> impl Fn() -> Option<AnyView> + Send + Sync + 'static {
    move || {
        if !visa.get() {
            return None;
        }
        fel.with(|fel| {
            fel.iter().find(|f| f.fält.namn() == fält).map(|f| {
                view! {
                    <p class="fel" role="alert" data-kod=f.kod>
                        {f.text.clone()}
                    </p>
                }
                .into_any()
            })
        })
    }
}

fn metodnamn(kod: &str) -> &'static str {
    Bokföringsmetod::från_kod(kod).map_or("", Bokföringsmetod::namn)
}

fn momsnamn(kod: &str) -> &'static str {
    Momsperiod::från_kod(kod).map_or("", Momsperiod::namn)
}

/// Inställningar → Företag.
#[component]
pub fn FöretagsInställningar() -> impl IntoView {
    if !matches!(use_context::<Läge>(), Some(Läge::Inloggad { .. })) {
        return view! { <LoggaIn /> }.into_any();
    }
    let id = use_params_map().with_untracked(|p| p.get("id").unwrap_or_default());
    let val = use_context::<Företagsval>().unwrap_or_default();
    let (Some(f), Some(data)) = (val.0.into_iter().find(|f| f.id == id), inställningsdata())
    else {
        return view! { <IngenÅtkomst /> }.into_any();
    };
    let bas = f.href();
    let sparat = use_query_map()
        .with_untracked(|q| q.get("sparat"))
        .map(|_| view! { <p class="notis" role="status">"Sparat."</p> });
    view! {
        <section class="sida">
            <p class="diskret">{f.namn} " · " {f.orgnr}</p>
            <h1>"Inställningar"</h1>
            <div class="installningar">
                <Inställningsmeny bas=bas.clone() aktiv="" />
                <div class="installningar-innehall">
                    {sparat}
                    <UppgiftsFormulär bas=bas.clone() data=data.clone() />
                    <MetodFormulär bas=bas.clone() data=data.clone() />
                    <MomsFormulär bas=bas.clone() data=data.clone() />
                    <RäkenskapsårsFormulär bas=bas.clone() data=data.clone() />
                    <Radering bas=bas data=data />
                </div>
            </div>
        </section>
    }
    .into_any()
}

#[component]
fn UppgiftsFormulär(bas: String, data: Inställningsdata) -> impl IntoView {
    let (form, q) = fråga();
    let tillbaka = form.as_deref() == Some("uppgifter");
    let start = if tillbaka {
        let v = |n: &str| q(n).unwrap_or_default();
        Uppgifter {
            namn: v("namn"),
            gatuadress: v("gatuadress"),
            postnummer: v("postnummer"),
            postort: v("postort"),
            epost: v("epost"),
            telefon: v("telefon"),
            säte: v("sate"),
            momsregnr: v("momsregnr"),
            f_skatt: q("f_skatt").is_some(),
        }
    } else {
        Uppgifter::from(&data.uppgifter)
    };
    let uppgifter = RwSignal::new(start.clone());
    let visa = RwSignal::new(tillbaka);
    let fel = Memo::new(move |_| uppgifter.with(|u| u.kontrollera().err().unwrap_or_default()));
    let skicka = move |ev: leptos::ev::SubmitEvent| {
        if !fel.with_untracked(Vec::is_empty) {
            ev.prevent_default();
            visa.set(true);
        }
    };
    let fält = move |namn: &'static str,
                     etikett: &'static str,
                     värde: String,
                     sätt: fn(&mut Uppgifter, String)| {
        view! {
            <label for=namn>{etikett}</label>
            <input
                id=namn
                name=namn
                type="text"
                value=värde
                on:input=move |ev| uppgifter.update(|u| sätt(u, event_target_value(&ev)))
            />
            {fel_vid(fel, visa, namn)}
        }
    };
    view! {
        <form
            class="formular"
            method="post"
            action=format!("{bas}/installningar/uppgifter")
            novalidate
            on:submit=skicka
        >
            <h2>"Företaget"</h2>
            <p class="diskret">"Organisationsnummer " {data.orgnr.clone()}</p>
            {fält(Fält::Namn.namn(), "Namn", start.namn.clone(), |u, v| u.namn = v)}
            {fält(Fält::Gatuadress.namn(), "Gatuadress", start.gatuadress.clone(), |u, v| {
                u.gatuadress = v
            })}
            {fält(Fält::Postnummer.namn(), "Postnummer", start.postnummer.clone(), |u, v| {
                u.postnummer = v
            })}
            {fält(Fält::Postort.namn(), "Postort", start.postort.clone(), |u, v| u.postort = v)}
            {fält(Fält::Epost.namn(), "E-post", start.epost.clone(), |u, v| u.epost = v)}
            {fält(Fält::Telefon.namn(), "Telefon", start.telefon.clone(), |u, v| u.telefon = v)}
            <p class="diskret">"Uppgifterna nedan krävs först av senare flöden, som momsdeklarationen."</p>
            {fält(Fält::Säte.namn(), "Säte", start.säte.clone(), |u, v| u.säte = v)}
            <label for="momsregnr">"Momsregistreringsnummer"</label>
            <input
                id="momsregnr"
                name="momsregnr"
                type="text"
                placeholder=data.föreslaget_momsregnr.clone()
                value=start.momsregnr.clone()
                on:input=move |ev| uppgifter.update(|u| u.momsregnr = event_target_value(&ev))
            />
            <p class="diskret">"Förslag för ett aktiebolag: " {data.föreslaget_momsregnr.clone()}</p>
            {fel_vid(fel, visa, Fält::Momsregnr.namn())}
            <label class="kryss">
                <input
                    name="f_skatt"
                    type="checkbox"
                    value="ja"
                    checked=start.f_skatt
                    on:change=move |ev| uppgifter.update(|u| u.f_skatt = event_target_checked(&ev))
                />
                "Godkänd för F-skatt"
            </label>
            <button class="knapp" type="submit">
                "Spara uppgifterna"
            </button>
        </form>
    }
}

/// Historiken med gäller-från-datum, äldst först.
#[component]
fn Historik(id: &'static str, rader: Vec<(String, &'static str)>) -> impl IntoView {
    view! {
        <table class="installningstabell" id=id>
            <thead>
                <tr>
                    <th>"Gäller från"</th>
                    <th>"Val"</th>
                </tr>
            </thead>
            <tbody>
                {rader
                    .into_iter()
                    .map(|(från, namn)| view! {
                        <tr>
                            <td>{från}</td>
                            <td>{namn}</td>
                        </tr>
                    })
                    .collect_view()}
            </tbody>
        </table>
    }
}

/// Startvärdet för ett byte: det som kom tillbaka från servern, eller början av året efter det
/// sista.
fn startbyte(form: &'static str, valfält: &str, läge: &Ändringsläge) -> (Byte, bool) {
    let (kom, q) = fråga();
    if kom.as_deref() == Some(form) {
        let byte = Byte {
            gäller_från: q("galler_fran").unwrap_or_default(),
            val: q(valfält).unwrap_or_default(),
        };
        return (byte, true);
    }
    let nästa = läge
        .räkenskapsår
        .last()
        .map(|å| å.nästa().start().to_string())
        .unwrap_or_default();
    (
        Byte {
            gäller_från: nästa,
            val: String::new(),
        },
        false,
    )
}

#[component]
fn MetodFormulär(bas: String, data: Inställningsdata) -> impl IntoView {
    let läge = data.läge();
    let (start, tillbaka) = startbyte("bokforingsmetod", "bokforingsmetod", &läge);
    let byte = RwSignal::new(start.clone());
    let visa = RwSignal::new(tillbaka);
    let fel = Memo::new(move |_| {
        byte.with(|b| {
            b.kontrollera_bokföringsmetod(&läge)
                .err()
                .unwrap_or_default()
        })
    });
    let skicka = move |ev: leptos::ev::SubmitEvent| {
        if !fel.with_untracked(Vec::is_empty) {
            ev.prevent_default();
            visa.set(true);
        }
    };
    let historik = data
        .bokföringsmetoder
        .iter()
        .map(|(d, k)| (d.clone(), metodnamn(k)))
        .collect();
    view! {
        <form
            class="formular"
            method="post"
            action=format!("{bas}/installningar/bokforingsmetod")
            novalidate
            on:submit=skicka
        >
            <h2>"Bokföringsmetod"</h2>
            <Historik id="bokforingsmetod-historik" rader=historik />
            <p class="diskret">"Ett byte gäller från början av ett nytt räkenskapsår."</p>
            <label for="metod-galler-fran">"Gäller från"</label>
            <input
                id="metod-galler-fran"
                name="galler_fran"
                type="date"
                value=start.gäller_från.clone()
                on:input=move |ev| byte.update(|b| b.gäller_från = event_target_value(&ev))
            />
            {fel_vid(fel, visa, Fält::GällerFrån.namn())}
            <label for="bokforingsmetod">"Ny bokföringsmetod"</label>
            <select
                id="bokforingsmetod"
                name="bokforingsmetod"
                on:change=move |ev| byte.update(|b| b.val = event_target_value(&ev))
            >
                <option value="">"Välj…"</option>
                {Bokföringsmetod::ALLA
                    .into_iter()
                    .map(|m| {
                        view! {
                            <option value=m.kod() selected=m.kod() == start.val>
                                {m.namn()}
                            </option>
                        }
                    })
                    .collect_view()}
            </select>
            {fel_vid(fel, visa, Fält::Bokföringsmetod.namn())}
            <button class="knapp" type="submit">
                "Byt bokföringsmetod"
            </button>
        </form>
    }
}

#[component]
fn MomsFormulär(bas: String, data: Inställningsdata) -> impl IntoView {
    let läge = data.läge();
    let (start, tillbaka) = startbyte("momsperiod", "momsperiod", &läge);
    let byte = RwSignal::new(start.clone());
    let visa = RwSignal::new(tillbaka);
    let fel = Memo::new(move |_| {
        byte.with(|b| b.kontrollera_momsperiod(&läge).err().unwrap_or_default())
    });
    let skicka = move |ev: leptos::ev::SubmitEvent| {
        if !fel.with_untracked(Vec::is_empty) {
            ev.prevent_default();
            visa.set(true);
        }
    };
    let historik = data
        .momsperioder
        .iter()
        .map(|(d, k)| (d.clone(), momsnamn(k)))
        .collect();
    view! {
        <form
            class="formular"
            method="post"
            action=format!("{bas}/installningar/momsperiod")
            novalidate
            on:submit=skicka
        >
            <h2>"Momsperiod"</h2>
            <Historik id="momsperiod-historik" rader=historik />
            <p class="diskret">
                "Ett byte gäller från början av en momsperiod: en månad, ett kvartal eller ett räkenskapsår."
            </p>
            <label for="moms-galler-fran">"Gäller från"</label>
            <input
                id="moms-galler-fran"
                name="galler_fran"
                type="date"
                value=start.gäller_från.clone()
                on:input=move |ev| byte.update(|b| b.gäller_från = event_target_value(&ev))
            />
            {fel_vid(fel, visa, Fält::GällerFrån.namn())}
            <label for="momsperiod">"Ny momsperiod"</label>
            <select
                id="momsperiod"
                name="momsperiod"
                on:change=move |ev| byte.update(|b| b.val = event_target_value(&ev))
            >
                <option value="">"Välj…"</option>
                {Momsperiod::ALLA
                    .into_iter()
                    .map(|m| {
                        view! {
                            <option value=m.kod() selected=m.kod() == start.val>
                                {m.namn()}
                            </option>
                        }
                    })
                    .collect_view()}
            </select>
            {fel_vid(fel, visa, Fält::Momsperiod.namn())}
            <button class="knapp" type="submit">
                "Byt momsperiod"
            </button>
        </form>
    }
}

#[component]
fn RäkenskapsårsFormulär(bas: String, data: Inställningsdata) -> impl IntoView {
    let läge = data.läge();
    let (form, q) = fråga();
    let tillbaka = form.as_deref() == Some("rakenskapsar");
    let (första_start, första_slut) = data.räkenskapsår.first().cloned().unwrap_or_default();
    let start = if tillbaka {
        q("slut").unwrap_or_default()
    } else {
        första_slut.clone()
    };
    let slut = RwSignal::new(start.clone());
    let visa = RwSignal::new(tillbaka);
    let fel = Memo::new(move |_| {
        slut.with(|s| {
            kontrollera_första_årets_slut(s, &läge)
                .err()
                .unwrap_or_default()
        })
    });
    let skicka = move |ev: leptos::ev::SubmitEvent| {
        if !fel.with_untracked(Vec::is_empty) {
            ev.prevent_default();
            visa.set(true);
        }
    };
    let år = data
        .räkenskapsår
        .iter()
        .map(|(s, e)| view! { <li>{s.clone()} " – " {e.clone()}</li> })
        .collect_view();
    view! {
        <form
            class="formular"
            method="post"
            action=format!("{bas}/installningar/rakenskapsar")
            novalidate
            on:submit=skicka
        >
            <h2>"Räkenskapsår"</h2>
            <ul class="lista">{år}</ul>
            <p class="diskret">
                "Det första räkenskapsåret börjar " {första_start}
                ". Slutet kan flyttas så länge inget senare år finns. Året slutar den sista dagen i en månad och är högst 18 månader."
            </p>
            <label for="slut">"Första årets slut"</label>
            <input
                id="slut"
                name="slut"
                type="date"
                value=start
                on:input=move |ev| slut.set(event_target_value(&ev))
            />
            {fel_vid(fel, visa, Fält::FörstaÅrSlut.namn())}
            <button class="knapp" type="submit">
                "Ändra slutdatum"
            </button>
        </form>
    }
}

#[component]
fn Radering(bas: String, data: Inställningsdata) -> impl IntoView {
    let (form, q) = fråga();
    let tillbaka = form.as_deref() == Some("radera");
    let serverfel = if tillbaka { q("fel") } else { None };
    let bekräftelse = RwSignal::new(if tillbaka {
        q("bekrafta").unwrap_or_default()
    } else {
        String::new()
    });
    let siffror: String = data.orgnr.chars().filter(char::is_ascii_digit).collect();
    let visa = RwSignal::new(tillbaka);
    let fel = Memo::new(move |_| {
        let mut fel = Vec::new();
        let kod = match serverfel.as_deref() {
            Some(HAR_RÄKENSKAPSINFORMATION) => Some((
                HAR_RÄKENSKAPSINFORMATION,
                "Företaget har bokförda verifikat och kan inte raderas.",
            )),
            _ if !bekräftelse.with(|b| bekräftar(b, &siffror)) => Some((
                BEKRÄFTELSE_FEL,
                "Skriv företagets organisationsnummer för att bekräfta.",
            )),
            _ => None,
        };
        if let Some((kod, text)) = kod {
            fel.push((kod, text));
        }
        fel
    });
    let skicka = move |ev: leptos::ev::SubmitEvent| {
        if fel.with_untracked(|f| f.iter().any(|(k, _)| *k == BEKRÄFTELSE_FEL)) {
            ev.prevent_default();
            visa.set(true);
        }
    };
    let felvisning = move || {
        if !visa.get() {
            return None;
        }
        fel.with(|f| {
            f.first().map(|(kod, text)| {
                view! {
                    <p class="fel" role="alert" data-kod=*kod>
                        {*text}
                    </p>
                }
            })
        })
    };
    let innehåll = if data.kan_raderas {
        view! {
            <p>
                "Företaget har inga bokförda verifikat och kan raderas med kontoplan, räkenskapsår, utkast och behandlingshistorik. Raderingen står i installationens logg."
            </p>
            <form
                class="formular"
                method="post"
                action=format!("{bas}/radera")
                novalidate
                on:submit=skicka
            >
                <label for="bekrafta">
                    "Skriv organisationsnumret " {data.orgnr.clone()} " för att bekräfta"
                </label>
                <input
                    id="bekrafta"
                    name="bekrafta"
                    type="text"
                    inputmode="numeric"
                    autocomplete="off"
                    value=bekräftelse.get_untracked()
                    on:input=move |ev| bekräftelse.set(event_target_value(&ev))
                />
                {felvisning}
                <button class="knapp knapp-fara" type="submit">
                    "Radera företaget"
                </button>
            </form>
        }
        .into_any()
    } else {
        view! {
            <p>"Företaget har bokförda verifikat och kan inte raderas."</p>
            {felvisning}
        }
        .into_any()
    };
    view! {
        <section class="radering">
            <h2>"Radera företaget"</h2>
            {innehåll}
        </section>
    }
}
