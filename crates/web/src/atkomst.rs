//! Inställningar → Åtkomst: användarna med åtkomst till företaget, öppna inbjudningar, att
//! bjuda in och att ge och ta åtkomst. Ändringarna kräver en färsk passkey; utan en ber sidan
//! om passkeyn med JS-shimmens bekräftelse, som på sidan Passkeys.
//!
//! Datan kommer från sidodata, och ändringarna är vanliga formulär som svarar med en
//! omdirigering hit. En ny inbjudningslänk visas i stället i svaret på formuläret, så att den
//! aldrig står i en adress.

use leptos::prelude::*;
use leptos_router::hooks::{use_params_map, use_query_map};
use serde::{Deserialize, Serialize};

use crate::app::{LoggaIn, Läge};
use crate::foretag::{Företagsval, IngenÅtkomst};
use crate::installningar::Inställningsmeny;
use crate::sidodata::sida;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MedlemVy {
    pub id: String,
    pub epost: String,
    pub visningsnamn: String,
    pub avaktiverad: bool,
    /// Den inloggade själv.
    pub du: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InbjudanVy {
    pub epost: String,
    pub inbjuden_av: String,
    pub giltig_till: String,
}

/// Det som sidan visar.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Åtkomstdata {
    /// Om sessionen har en färsk passkey, så att åtkomsten får ändras.
    pub färsk: bool,
    pub användare: Vec<MedlemVy>,
    pub inbjudningar: Vec<InbjudanVy>,
    pub kandidater: Vec<MedlemVy>,
}

/// Datan för `/foretag/<id>/installningar/atkomst`, eller `None` utan åtkomst.
#[server(prefix = "/api", endpoint = "atkomstdata", input = server_fn::codec::GetUrl)]
pub async fn hämta_åtkomstdata(adress: String) -> Result<Option<Åtkomstdata>, ServerFnError> {
    match crate::sidodata::Sidbegäran::för(&adress)? {
        Some(begäran) => server::åtkomstdata(&begäran).await,
        None => Ok(None),
    }
}

#[cfg(feature = "ssr")]
pub(crate) mod server {
    use std::sync::Arc;

    use axum::extract::{Path, State};
    use axum::http::{StatusCode, header};
    use axum::response::{IntoResponse, Redirect, Response};
    use axum::routing::post;
    use axum::{Extension, Form, Router};
    use leptos::prelude::ServerFnError;
    use maya_core::aktor::AnvändarId;
    use maya_core::atkomst::{self, DelningsFel, Inbjudan, Medlem, ÅtkomstFel};
    use maya_core::foretag::FöretagsId;
    use maya_core::inloggning::Inloggad;
    use maya_core::{Databas, Klocka};
    use serde::Deserialize;

    use super::{InbjudanVy, MedlemVy, Åtkomstdata};
    use crate::sidodata::{self, Sidbegäran};

    #[derive(Clone)]
    pub(crate) struct ÅtkomstState {
        pub db: Databas,
        pub klocka: Klocka,
        /// Mayas publika URL utan avslutande snedstreck, för inbjudningslänken.
        pub publik_url: String,
    }

    pub(crate) fn routes(state: ÅtkomstState) -> Router {
        Router::new()
            .route("/foretag/{id}/atkomst/inbjudan", post(bjud_in))
            .route("/foretag/{id}/atkomst/ge", post(ge))
            .route("/foretag/{id}/atkomst/ta", post(ta))
            .with_state(Arc::new(state))
    }

    /// Hämtar det som `/foretag/<id>/installningar/atkomst` visar. Utan åtkomst blir den
    /// `None`; sidan visar det ur företagsvalet.
    pub(crate) async fn åtkomstdata(
        b: &Sidbegäran,
    ) -> Result<Option<Åtkomstdata>, ServerFnError> {
        let delar = b.delar();
        let ["foretag", id, "installningar", "atkomst"] = delar.as_slice() else {
            return Ok(None);
        };
        let Ok(företag) = id.parse::<FöretagsId>() else {
            return Ok(None);
        };
        let inloggad = &b.inloggad;
        match atkomst::åtkomst(&b.db, &b.klocka, &inloggad.aktör, företag).await {
            Ok(lista) => {
                let vy = |m: Medlem| MedlemVy {
                    id: m.id.to_string(),
                    du: m.id == inloggad.användare.id,
                    epost: m.epost,
                    visningsnamn: m.visningsnamn,
                    avaktiverad: m.avaktiverad,
                };
                Ok(Some(Åtkomstdata {
                    färsk: inloggad.har_färsk_passkey(&b.klocka),
                    användare: lista.användare.into_iter().map(vy).collect(),
                    inbjudningar: lista
                        .inbjudningar
                        .into_iter()
                        .map(|i| InbjudanVy {
                            epost: i.epost,
                            inbjuden_av: i.inbjuden_av,
                            giltig_till: tid(i.giltig_till),
                        })
                        .collect(),
                    kandidater: lista.kandidater.into_iter().map(vy).collect(),
                }))
            }
            Err(ÅtkomstFel::IngenÅtkomst | ÅtkomstFel::KanalenFårInte) => Ok(None),
            Err(e) => Err(sidodata::serverfel(e)),
        }
    }

    /// En tidpunkt i Stockholm, till minuten.
    fn tid(t: jiff::Timestamp) -> String {
        t.to_zoned(maya_domain::tid::stockholm())
            .strftime("%Y-%m-%d %H:%M")
            .to_string()
    }

    #[derive(Deserialize)]
    struct Epost {
        #[serde(default)]
        epost: String,
    }

    #[derive(Deserialize)]
    struct Användare {
        #[serde(default)]
        anvandare: String,
    }

    fn sidan(företag: FöretagsId, query: &str) -> Response {
        Redirect::to(&format!("/foretag/{företag}/installningar/atkomst?{query}")).into_response()
    }

    /// Svaret på ett fel från `core`: tillbaka till sidan med en kod, eller 403.
    fn fel(företag: FöretagsId, fel: DelningsFel) -> Response {
        let kod = match fel {
            DelningsFel::KräverFärskPasskey => "farsk",
            DelningsFel::OgiltigEpost(_) => "epost",
            DelningsFel::HarRedanÅtkomst => "har_atkomst",
            DelningsFel::OkändAnvändare => "okand",
            DelningsFel::SistaÅtkomsten => "sista",
            DelningsFel::Åtkomst(ÅtkomstFel::IngenÅtkomst | ÅtkomstFel::KanalenFårInte) => {
                return ingen_åtkomst();
            }
            DelningsFel::Åtkomst(e) => return serverfel(e),
        };
        sidan(företag, &format!("fel={kod}"))
    }

    async fn bjud_in(
        State(state): State<Arc<ÅtkomstState>>,
        Path(id): Path<String>,
        inloggad: Option<Extension<Inloggad>>,
        Form(form): Form<Epost>,
    ) -> Response {
        let Some(Extension(inloggad)) = inloggad else {
            return Redirect::to("/").into_response();
        };
        let Ok(företag) = id.parse::<FöretagsId>() else {
            return ingen_åtkomst();
        };
        match atkomst::bjud_in(&state.db, &state.klocka, &inloggad, företag, &form.epost).await {
            // Länken visas en gång, för kopiering; den mejlas inte. Den står bara i svaret på
            // formuläret, aldrig i en adress (historik, loggar, Referer).
            Ok(Inbjudan::Länk(länk)) => inbjudningslänk(
                företag,
                &format!("{}/registrera/{}", state.publik_url, länk.token()),
                &form.epost,
            ),
            Ok(Inbjudan::ÅtkomstGiven) => sidan(företag, "meddelande=atkomst_given"),
            Err(e) => fel(företag, e),
        }
    }

    /// Sidan med inbjudningslänken, som svar på formuläret. Den sparas inte (`no-store`) och
    /// skickar ingen Referer vidare.
    fn inbjudningslänk(företag: FöretagsId, url: &str, epost: &str) -> Response {
        let tillbaka = format!("/foretag/{företag}/installningar/atkomst");
        let epost = html(
            &maya_domain::epost::normalisera(epost).unwrap_or_else(|_| epost.trim().to_string()),
        );
        let url = html(url);
        let sida = format!(
            r#"<!DOCTYPE html>
<html lang="sv">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="referrer" content="no-referrer">
<title>Inbjudningslänk · Maya</title>
<link rel="stylesheet" href="/pkg/maya.css">
<script>{tema}</script>
</head>
<body>
<main class="sida">
<h1>Inbjudan till {epost}</h1>
<section class="notis inbjudningslank" role="status">
<p>Kopiera länken och skicka den till den du bjuder in. Den gäller i 24 timmar och kan användas en gång. Maya visar den inte igen.</p>
<input id="inbjudningslank" type="text" readonly value="{url}" aria-label="Inbjudningslänk" onfocus="this.select()">
</section>
<p><a class="knapp" href="{tillbaka}">Tillbaka till Åtkomst</a></p>
</main>
</body>
</html>
"#,
            tema = crate::app::TEMA_FÖRE_RITNING,
        );
        (
            [
                (header::CONTENT_TYPE, "text/html; charset=utf-8"),
                (header::CACHE_CONTROL, "no-store"),
                (header::REFERRER_POLICY, "no-referrer"),
            ],
            sida,
        )
            .into_response()
    }

    /// Text som HTML, för text och attributvärden.
    fn html(text: &str) -> String {
        text.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&#39;")
    }

    async fn ge(
        State(state): State<Arc<ÅtkomstState>>,
        Path(id): Path<String>,
        inloggad: Option<Extension<Inloggad>>,
        Form(form): Form<Användare>,
    ) -> Response {
        let Some(Extension(inloggad)) = inloggad else {
            return Redirect::to("/").into_response();
        };
        let Ok(företag) = id.parse::<FöretagsId>() else {
            return ingen_åtkomst();
        };
        let Ok(användare) = form.anvandare.parse::<AnvändarId>() else {
            return fel(företag, DelningsFel::OkändAnvändare);
        };
        match atkomst::ge_åtkomst(&state.db, &state.klocka, &inloggad, företag, användare).await
        {
            Ok(()) => sidan(företag, "meddelande=atkomst_given"),
            Err(e) => fel(företag, e),
        }
    }

    /// Tar en användares åtkomst. Den som tar sin egen åtkomst går till startsidan.
    async fn ta(
        State(state): State<Arc<ÅtkomstState>>,
        Path(id): Path<String>,
        inloggad: Option<Extension<Inloggad>>,
        Form(form): Form<Användare>,
    ) -> Response {
        let Some(Extension(inloggad)) = inloggad else {
            return Redirect::to("/").into_response();
        };
        let Ok(företag) = id.parse::<FöretagsId>() else {
            return ingen_åtkomst();
        };
        let Ok(användare) = form.anvandare.parse::<AnvändarId>() else {
            return fel(företag, DelningsFel::OkändAnvändare);
        };
        match atkomst::ta_åtkomst(&state.db, &state.klocka, &inloggad, företag, användare).await
        {
            Ok(()) if användare == inloggad.användare.id => Redirect::to("/").into_response(),
            Ok(()) => sidan(företag, "meddelande=atkomst_tagen"),
            Err(e) => fel(företag, e),
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
        tracing::error!("åtkomst: {e}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
            "Något gick fel i Maya; försök igen.\n",
        )
            .into_response()
    }
}

/// Felkoderna som formulären skickar tillbaka i `?fel=`.
fn felmeddelande(kod: &str) -> &'static str {
    match kod {
        "farsk" => "Använd din passkey igen; det krävs för att bjuda in och ändra åtkomst.",
        "epost" => "Skriv en giltig e-postadress.",
        "har_atkomst" => "Användaren har redan åtkomst till företaget.",
        "okand" => "Det finns ingen sådan användare.",
        "sista" => "Den sista åtkomsten till företaget går inte att ta bort.",
        _ => "Det gick inte. Försök igen.",
    }
}

fn meddelande(kod: &str) -> &'static str {
    match kod {
        "atkomst_given" => "Användaren har fått åtkomst till företaget.",
        "atkomst_tagen" => "Användarens åtkomst är borttagen.",
        _ => "Sparat.",
    }
}

/// Inställningar → Åtkomst.
#[component]
pub fn ÅtkomstSida() -> impl IntoView {
    sida(hämta_åtkomstdata, åtkomstsida)
}

fn åtkomstsida(data: Option<Åtkomstdata>) -> AnyView {
    if !matches!(use_context::<Läge>(), Some(Läge::Inloggad { .. })) {
        return view! { <LoggaIn /> }.into_any();
    }
    let id = use_params_map().with_untracked(|p| p.get("id").unwrap_or_default());
    let val = use_context::<Företagsval>().unwrap_or_default();
    let (Some(f), Some(data)) = (val.0.into_iter().find(|f| f.id == id), data) else {
        return view! { <IngenÅtkomst /> }.into_any();
    };
    let bas = f.href();
    let sida = format!("{bas}/installningar/atkomst");
    let (besked, fel) = use_query_map().with_untracked(|q| (q.get("meddelande"), q.get("fel")));
    let besked = besked.map(|kod| view! { <p class="notis" role="status">{meddelande(&kod)}</p> });
    let fel = fel.map(|kod| {
        view! {
            <p class="fel" role="alert" data-kod=kod.clone()>
                {felmeddelande(&kod)}
            </p>
        }
    });
    let färsk = data.färsk;
    let användare = data
        .användare
        .into_iter()
        .map(|m| {
            let ta = färsk.then(|| {
                let text = if m.du {
                    "Ta bort min åtkomst"
                } else {
                    "Ta bort åtkomst"
                };
                view! {
                    <form class="rad-formular" method="post" action=format!("{bas}/atkomst/ta")>
                        <input type="hidden" name="anvandare" value=m.id.clone() />
                        <button class="knapp knapp-diskret" type="submit">{text}</button>
                    </form>
                }
            });
            view! {
                <li>
                    <strong>{m.visningsnamn.clone()}</strong>
                    <span class="detalj">
                        " " {m.epost}
                        {m.du.then_some(" (du)")}
                        {m.avaktiverad.then_some(" · avaktiverad")}
                    </span>
                    {ta}
                </li>
            }
        })
        .collect_view();
    let inbjudningar = if data.inbjudningar.is_empty() {
        view! { <p class="diskret">"Inga öppna inbjudningar."</p> }.into_any()
    } else {
        let rader = data
            .inbjudningar
            .into_iter()
            .map(|i| {
                view! {
                    <li>
                        <strong>{i.epost}</strong>
                        <span class="detalj">
                            " Inbjuden av " {i.inbjuden_av} ", gäller till " {i.giltig_till}
                        </span>
                    </li>
                }
            })
            .collect_view();
        view! { <ul class="lista" id="inbjudningar">{rader}</ul> }.into_any()
    };
    let ändra = if färsk {
        let kandidater = (!data.kandidater.is_empty()).then(|| {
            let rader = data
                .kandidater
                .into_iter()
                .map(|m| {
                    view! {
                        <li>
                            <strong>{m.visningsnamn}</strong>
                            <span class="detalj">" " {m.epost}</span>
                            <form
                                class="rad-formular"
                                method="post"
                                action=format!("{bas}/atkomst/ge")
                            >
                                <input type="hidden" name="anvandare" value=m.id />
                                <button class="knapp knapp-diskret" type="submit">
                                    "Ge åtkomst"
                                </button>
                            </form>
                        </li>
                    }
                })
                .collect_view();
            view! {
                <h2>"Ge åtkomst"</h2>
                <p class="diskret">"Användare som du delar ett annat företag med."</p>
                <ul class="lista">{rader}</ul>
            }
        });
        view! {
            <form class="formular" method="post" action=format!("{bas}/atkomst/inbjudan")>
                <h2>"Bjud in"</h2>
                <p class="diskret">
                    "Maya skapar en länk som du kopierar och skickar själv. Har adressen redan ett konto i Maya får det åtkomst direkt."
                </p>
                <label for="epost">"E-post"</label>
                <input id="epost" name="epost" type="email" required autocomplete="off" />
                <button class="knapp" type="submit">"Bjud in"</button>
            </form>
            {kandidater}
        }
        .into_any()
    } else {
        view! {
            <form class="formular" data-passkey="bekrafta" data-efter=sida>
                <p>"Använd din passkey för att bjuda in och ändra åtkomst."</p>
                <button class="knapp" type="submit">"Använd passkey"</button>
                <p class="fel" role="alert" data-fel hidden></p>
            </form>
        }
        .into_any()
    };
    view! {
        <section class="sida">
            <p class="diskret">{f.namn} " · " {f.orgnr}</p>
            <h1>"Inställningar"</h1>
            <Inställningsmeny bas=bas.clone() aktiv="/atkomst" />
            <div class="installningar-innehall">
                {besked}
                {fel}
                <h2>"Användare med åtkomst"</h2>
                <ul class="lista" id="anvandare">{användare}</ul>
                <h2>"Öppna inbjudningar"</h2>
                {inbjudningar}
                {ändra}
            </div>
        </section>
    }
    .into_any()
}
