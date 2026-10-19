//! Utskick av en utfärdad kundfaktura (#138): avsnittet Utskick i fakturans dokument och
//! panelen Skicka (`?panel=skicka`), med mottagare, svarsadress, meddelande och den arkiverade
//! PDF:en som bilaga, och Lämnad på annat sätt. Utan SMTP visar panelen bara det senare, så att
//! fakturan markeras som skickad för hand.

use leptos::prelude::*;
use leptos_router::hooks::use_query_map;
use maya_domain::utskick::Lämningssätt;
use serde::{Deserialize, Serialize};

use crate::kontoplan::koda;

/// Ett utskick på fakturan.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UtskickVy {
    pub tid: String,
    /// Mottagaren, eller hur fakturan lämnades.
    pub vad: String,
    /// Vem som skickade.
    pub namn: String,
    /// Felet när utskicket misslyckades.
    pub fel: Option<String>,
    /// Med e-post från Maya.
    pub epost: bool,
}

/// Det som panelen Skicka visar.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkickaVy {
    /// Om Maya har SMTP och kan skicka med e-post.
    pub epost: bool,
    pub till: String,
    pub kundregistret: String,
    pub svarsadress: String,
    pub meddelande: String,
    pub bilaga: String,
    /// Det senaste utskicket, när det var ett mejl som misslyckades.
    pub misslyckat: Option<UtskickVy>,
}

/// Avsnittet Utskick: tid, mottagare eller sätt, vem och resultat.
#[component]
pub fn Utskicken(utskick: Vec<UtskickVy>) -> impl IntoView {
    let senaste_fel = utskick
        .last()
        .and_then(|u| u.fel.clone())
        .map(|fel| view! { <p class="fel">"Senaste utskicket misslyckades: " {fel}</p> });
    let tomt = utskick
        .is_empty()
        .then(|| view! { <p class="diskret">"Inte skickad än."</p> });
    let rader = utskick
        .into_iter()
        .map(|u| {
            let resultat = match (&u.fel, u.epost) {
                (Some(_), _) => {
                    view! { <span class="utskick-fel">"misslyckades"</span> }.into_any()
                }
                (None, true) => view! { <span class="utskick-ok">"skickad"</span> }.into_any(),
                (None, false) => view! { <span class="utskick-ok">"markerad"</span> }.into_any(),
            };
            view! {
                <li>
                    <span class="diskret">{u.tid}</span>
                    " "
                    <span>{u.vad}</span>
                    " "
                    {resultat}
                    " "
                    <span class="diskret">"· " {u.namn}</span>
                </li>
            }
        })
        .collect_view();
    view! {
        <section class="utskick">
            <h3>"Utskick"</h3>
            {tomt}
            <ul class="historik">{rader}</ul>
            {senaste_fel}
        </section>
    }
}

/// Panelen Skicka fakturan. `lista` är Kundfakturor och `id` fakturan.
#[component]
pub fn SkickaPanel(lista: String, id: String, s: SkickaVy) -> impl IntoView {
    let fråga = |namn: &str| use_query_map().with_untracked(|q| q.get(namn));
    let här = format!("{lista}?f={id}");
    let epost = s.epost.then(|| {
        let till = fråga("till").unwrap_or_else(|| s.till.clone());
        let meddelande = fråga("meddelande").unwrap_or_else(|| s.meddelande.clone());
        let misslyckat = s.misslyckat.clone().map(|m| {
            let använd = (s.kundregistret != till && !s.kundregistret.is_empty()).then(|| {
                let href = format!(
                    "{lista}?f={id}&panel=skicka&till={}",
                    koda(&s.kundregistret)
                );
                view! {
                    <p class="diskret">
                        "Kundregistret har " {s.kundregistret.clone()} ". "
                        <a href=href>"Använd den"</a>
                    </p>
                }
            });
            view! {
                <div class="fel" role="alert">
                    <p><strong>"Förra utskicket misslyckades"</strong></p>
                    <p>{m.vad} ", " {m.tid} ": " {m.fel.unwrap_or_default()}</p>
                </div>
                {använd}
            }
        });
        let svarsadress = if s.svarsadress.is_empty() {
            "Ingen svarsadress (Inställningar → Företag)".to_string()
        } else {
            s.svarsadress.clone()
        };
        let knapp = if s.misslyckat.is_some() { "Skicka igen" } else { "Skicka" };
        view! {
            {misslyckat}
            <form class="formular" method="post" action=format!("{lista}/{id}/skicka")>
                <label>
                    "Till" <input type="email" name="till" value=till required />
                </label>
                <label>
                    "Svar till" <input type="text" value=svarsadress readonly />
                </label>
                <label>
                    "Meddelande" <textarea name="meddelande" rows="6">{meddelande}</textarea>
                    <span class="diskret">"Företagets standardmeddelande, redigerbart per utskick."</span>
                </label>
                <p class="utskick-bilaga">
                    {s.bilaga.clone()}
                    <span class="diskret">" · den arkiverade PDF:en, oförändrad"</span>
                </p>
                <button class="knapp knapp-primar" type="submit">{knapp}</button>
                " "
                <a class="knapp" href=här.clone()>"Avbryt"</a>
            </form>
        }
    });
    let utan_epost = (!s.epost).then(|| {
        view! {
            <p class="diskret">
                "Maya har ingen SMTP-server, så fakturan skickas inte med e-post härifrån. "
                "Markera hur den lämnades till kunden."
            </p>
        }
    });
    let sätt = Lämningssätt::ALLA
        .iter()
        .map(|s| view! { <option value=s.kod()>{s.namn()}</option> })
        .collect_view();
    view! {
        <section class="dialog skicka" aria-labelledby="skicka-rubrik">
            <h2 id="skicka-rubrik">"Skicka fakturan"</h2>
            {epost}
            {utan_epost}
            <form class="lamnad" method="post" action=format!("{lista}/{id}/lamnad")>
                <label>
                    "Lämnad på annat sätt" <select name="satt">{sätt}</select>
                </label>
                " "
                <button class="knapp" type="submit">"Markera som skickad"</button>
                {(!s.epost).then(|| view! { " " <a class="knapp" href=här.clone()>"Avbryt"</a> })}
                <p class="diskret">"Loggas i historiken. Fakturan byter inte tillstånd."</p>
            </form>
        </section>
    }
}

#[cfg(feature = "ssr")]
pub(crate) mod server {
    use std::sync::Arc;

    use axum::extract::{Path, State};
    use axum::http::StatusCode;
    use axum::response::{IntoResponse, Redirect, Response};
    use axum::routing::post;
    use axum::{Extension, Form, Router};
    use maya_core::atkomst::ÅtkomstFel;
    use maya_core::foretag::FöretagsId;
    use maya_core::inloggning::Inloggad;
    use maya_core::kundfaktura::KundfakturaId;
    use maya_core::utskick::{self, NyttUtskick, Sätt, Utfall, Utskick, UtskickFel};
    use maya_core::{Databas, Klocka};
    use maya_domain::utskick::Lämningssätt;

    use super::{SkickaVy, UtskickVy};
    use crate::epost::SmtpEpost;
    use crate::sidodata::Sidbegäran;
    use crate::svar::{ingen_åtkomst, serverfel, text};
    use crate::verifikat::server::tid;

    #[derive(Clone)]
    pub(crate) struct UtskickState {
        pub db: Databas,
        pub klocka: Klocka,
        /// `None` utan SMTP.
        pub epost: Option<SmtpEpost>,
    }

    pub(crate) fn routes(state: UtskickState) -> Router {
        Router::new()
            .route("/foretag/{id}/kundfakturor/{f}/skicka", post(skicka))
            .route("/foretag/{id}/kundfakturor/{f}/lamnad", post(lämnad))
            .with_state(Arc::new(state))
    }

    fn vy(u: &Utskick) -> UtskickVy {
        let (vad, epost) = match &u.sätt {
            Sätt::Epost { till, .. } => (till.clone(), true),
            Sätt::Lämnad(s) => (s.namn().to_string(), false),
        };
        UtskickVy {
            tid: tid(u.signatur.tid),
            vad,
            namn: u.signatur.namn.clone(),
            fel: u.fel.clone(),
            epost,
        }
    }

    /// Fakturans utskick, och panelen Skicka när `panel` är öppen.
    pub(crate) async fn utskicken(
        b: &Sidbegäran,
        företag: FöretagsId,
        id: KundfakturaId,
        panel: bool,
    ) -> Result<(Vec<UtskickVy>, Option<SkickaVy>), UtskickFel> {
        let aktör = &b.inloggad.aktör;
        let utskicken = utskick::utskicken(&b.db, aktör, företag, id)
            .await?
            .iter()
            .map(vy)
            .collect();
        let panelen = if panel {
            let f = utskick::förslag(&b.db, aktör, företag, id).await?;
            Some(SkickaVy {
                epost: b.config.smtp.is_some(),
                till: f.till,
                kundregistret: f.kundregistret,
                svarsadress: f.svarsadress,
                meddelande: f.meddelande,
                bilaga: f.bilaga,
                misslyckat: f.misslyckat.as_ref().map(vy),
            })
        } else {
            None
        };
        Ok((utskicken, panelen))
    }

    type Fält = Vec<(String, String)>;

    fn värde(fält: &Fält, namn: &str) -> String {
        fält
            .iter()
            .find(|(k, _)| k == namn)
            .map(|(_, v)| v.clone())
            .unwrap_or_default()
    }

    fn tolka(
        id: &str,
        f: &str,
        inloggad: Option<Extension<Inloggad>>,
    ) -> Result<(Inloggad, FöretagsId, KundfakturaId), Box<Response>> {
        let Some(Extension(inloggad)) = inloggad else {
            return Err(Box::new(Redirect::to("/").into_response()));
        };
        let Ok(företag) = id.parse::<FöretagsId>() else {
            return Err(Box::new(ingen_åtkomst()));
        };
        let Ok(f) = f.parse::<KundfakturaId>() else {
            return Err(Box::new(finns_inte()));
        };
        Ok((inloggad, företag, f))
    }

    async fn skicka(
        State(state): State<Arc<UtskickState>>,
        Path((id, f)): Path<(String, String)>,
        inloggad: Option<Extension<Inloggad>>,
        Form(fält): Form<Fält>,
    ) -> Response {
        let (inloggad, företag, f) = match tolka(&id, &f, inloggad) {
            Ok(t) => t,
            Err(r) => return *r,
        };
        let Some(epost) = &state.epost else {
            return text(
                StatusCode::CONFLICT,
                "Maya har ingen SMTP-server (MAYA_SMTP_HOST), så fakturor skickas inte med \
                 e-post. Markera fakturan som lämnad på annat sätt.\n",
            );
        };
        let lista = format!("/foretag/{företag}/kundfakturor");
        let ny = NyttUtskick {
            till: värde(&fält, "till"),
            meddelande: värde(&fält, "meddelande"),
        };
        match utskick::skicka(
            &state.db,
            &state.klocka,
            &inloggad.aktör,
            företag,
            f,
            &ny,
            epost,
        )
        .await
        {
            Ok(Utfall::Skickat) => {
                Redirect::to(&format!("{lista}?f={f}&skickat=1")).into_response()
            }
            Ok(Utfall::Misslyckat(_)) => {
                Redirect::to(&format!("{lista}?f={f}&panel=skicka&fel=skickfel")).into_response()
            }
            Err(UtskickFel::Mottagare(_)) => {
                let mut q = url::form_urlencoded::Serializer::new(String::new());
                q.append_pair("f", &f.to_string());
                q.append_pair("panel", "skicka");
                q.append_pair("fel", "mottagare");
                q.append_pair("till", &ny.till);
                q.append_pair("meddelande", &ny.meddelande);
                Redirect::to(&format!("{lista}?{}", q.finish())).into_response()
            }
            Err(e) => utskicksfel(e),
        }
    }

    async fn lämnad(
        State(state): State<Arc<UtskickState>>,
        Path((id, f)): Path<(String, String)>,
        inloggad: Option<Extension<Inloggad>>,
        Form(fält): Form<Fält>,
    ) -> Response {
        let (inloggad, företag, f) = match tolka(&id, &f, inloggad) {
            Ok(t) => t,
            Err(r) => return *r,
        };
        let Some(sätt) = Lämningssätt::från_kod(&värde(&fält, "satt")) else {
            return text(StatusCode::BAD_REQUEST, "Välj hur fakturan lämnades.\n");
        };
        match utskick::lämnad_på_annat_sätt(
            &state.db,
            &state.klocka,
            &inloggad.aktör,
            företag,
            f,
            sätt,
        )
        .await
        {
            Ok(()) => Redirect::to(&format!("/foretag/{företag}/kundfakturor?f={f}&lamnad=1"))
                .into_response(),
            Err(e) => utskicksfel(e),
        }
    }

    fn utskicksfel(e: UtskickFel) -> Response {
        match e {
            UtskickFel::FinnsInte | UtskickFel::InteUtfärdad => finns_inte(),
            UtskickFel::Åtkomst(ÅtkomstFel::IngenÅtkomst | ÅtkomstFel::KanalenFårInte) => {
                ingen_åtkomst()
            }
            e => serverfel(e),
        }
    }

    fn finns_inte() -> Response {
        text(
            StatusCode::NOT_FOUND,
            "Det finns ingen utfärdad kundfaktura att skicka.\n",
        )
    }
}
