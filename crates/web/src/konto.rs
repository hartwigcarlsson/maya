//! Användarmenyns sidor för passkeys och sessioner. Sidorna läser data som middleware har
//! hämtat ur `core`, som i `verifikat.rs`, och ändrar med vanliga formulär (`/konto/…`) och
//! passkey-ceremonierna i JS-shimmen.

use leptos::prelude::*;
use leptos_router::hooks::use_query_map;
use serde::{Deserialize, Serialize};

use crate::app::{LoggaIn, Läge};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PasskeyVy {
    pub id: String,
    pub namn: String,
    pub skapad: String,
    pub senast_använd: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PasskeySida {
    pub passkeys: Vec<PasskeyVy>,
    /// Om sessionen har en färsk passkey, så att passkeys får hanteras.
    pub färsk: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionVy {
    pub id: String,
    pub enhet: String,
    pub skapad: String,
    pub senast_aktiv: String,
    pub denna: bool,
}

/// Det som sidorna Passkeys och Sessioner visar. Middleware hämtar det ur `core` och lägger
/// det på begäran, och det följer med till webbläsaren vid hydreringen.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Kontodata {
    pub passkeys: Option<PasskeySida>,
    pub sessioner: Option<Vec<SessionVy>>,
}

fn kontodata() -> Kontodata {
    SharedValue::new(|| {
        #[cfg(feature = "ssr")]
        if let Some(parts) = use_context::<axum::http::request::Parts>()
            && let Some(data) = parts.extensions.get::<Kontodata>()
        {
            return data.clone();
        }
        Kontodata::default()
    })
    .into_inner()
}

#[cfg(feature = "ssr")]
pub(crate) mod server {
    use axum::extract::{Request, State};
    use axum::http::StatusCode;
    use axum::middleware::Next;
    use axum::response::{IntoResponse, Response};
    use maya_core::inloggning::{self, Inloggad, InloggningsFel};
    use maya_core::{Databas, Klocka};

    use super::{Kontodata, PasskeySida, PasskeyVy, SessionVy};
    use crate::inloggning::InloggningsState;

    /// Hämtar det som `/passkeys` och `/sessioner` visar. Körs efter `inloggning::identifiera`.
    pub(crate) async fn lägg_på_kontodata(
        State(state): State<InloggningsState>,
        mut req: Request,
        next: Next,
    ) -> Response {
        let Some(inloggad) = req.extensions().get::<Inloggad>().cloned() else {
            return next.run(req).await;
        };
        let data = match req.uri().path() {
            "/passkeys" => passkeys(&state.db, &state.klocka, &inloggad)
                .await
                .map(|p| Kontodata {
                    passkeys: Some(p),
                    sessioner: None,
                }),
            "/sessioner" => sessioner(&state.db, &inloggad).await.map(|s| Kontodata {
                passkeys: None,
                sessioner: Some(s),
            }),
            _ => return next.run(req).await,
        };
        match data {
            Ok(data) => {
                req.extensions_mut().insert(data);
                next.run(req).await
            }
            Err(e) => {
                tracing::error!("kontosidan misslyckades: {e}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Något gick fel i Maya; försök igen.\n",
                )
                    .into_response()
            }
        }
    }

    async fn passkeys(
        db: &Databas,
        klocka: &Klocka,
        inloggad: &Inloggad,
    ) -> Result<PasskeySida, InloggningsFel> {
        Ok(PasskeySida {
            färsk: inloggad.har_färsk_passkey(klocka),
            passkeys: inloggning::passkeys(db, inloggad)
                .await?
                .into_iter()
                .map(|p| PasskeyVy {
                    id: p.id,
                    namn: p.namn,
                    skapad: tid(p.skapad),
                    senast_använd: p.senast_använd.map(tid),
                })
                .collect(),
        })
    }

    async fn sessioner(
        db: &Databas,
        inloggad: &Inloggad,
    ) -> Result<Vec<SessionVy>, InloggningsFel> {
        Ok(inloggning::sessioner(db, inloggad)
            .await?
            .into_iter()
            .map(|s| SessionVy {
                id: s.id,
                enhet: s.enhet.unwrap_or_else(|| "Okänd webbläsare".into()),
                skapad: tid(s.skapad),
                senast_aktiv: tid(s.senast_aktiv),
                denna: s.denna,
            })
            .collect())
    }

    /// En tidpunkt i Stockholm, till minuten.
    fn tid(t: jiff::Timestamp) -> String {
        t.to_zoned(maya_domain::tid::stockholm())
            .strftime("%Y-%m-%d %H:%M")
            .to_string()
    }
}

fn inloggad() -> bool {
    matches!(use_context::<Läge>(), Some(Läge::Inloggad { .. }))
}

/// Felkoderna som `/konto/…` skickar tillbaka i `?fel=`.
fn felmeddelande(kod: &str) -> &'static str {
    match kod {
        "farsk" => "Använd din passkey igen; det krävs för att hantera passkeys.",
        "sista" => "Den sista passkeyn går inte att ta bort.",
        "namn" => "Passkeyn måste ha ett namn.",
        _ => "Det gick inte. Försök igen.",
    }
}

#[component]
pub fn Passkeys() -> impl IntoView {
    if !inloggad() {
        return view! { <LoggaIn /> }.into_any();
    }
    let (ny, fel) = use_query_map().with_untracked(|q| (q.get("ny").is_some(), q.get("fel")));
    let sida = kontodata().passkeys;
    view! {
        <section class="sida">
            <h1>"Passkeys"</h1>
            {ny
                .then(|| {
                    view! {
                        <p class="uppmaning">
                            "Lägg till en passkey till, till exempel på din telefon, så att du inte blir utelåst om du tappar en enhet."
                        </p>
                    }
                })}
            {fel.map(|kod| view! { <p class="fel" role="alert">{felmeddelande(&kod)}</p> })}
            {sida.map(|sida| view! { <PasskeyLista sida /> })}
        </section>
    }
    .into_any()
}

#[component]
fn PasskeyLista(sida: PasskeySida) -> impl IntoView {
    let färsk = sida.färsk;
    let rader = sida
        .passkeys
        .into_iter()
        .map(|p| {
            let hantera = färsk.then(|| {
                let id = p.id.clone();
                view! {
                    <form class="rad-formular" method="post" action="/konto/passkeys/namn">
                        <input type="hidden" name="id" value=id.clone() />
                        <input name="namn" type="text" required value=p.namn.clone() aria-label="Namn" />
                        <button class="knapp knapp-diskret" type="submit">"Byt namn"</button>
                    </form>
                    <form class="rad-formular" method="post" action="/konto/passkeys/ta-bort">
                        <input type="hidden" name="id" value=id />
                        <button class="knapp knapp-diskret" type="submit">"Ta bort"</button>
                    </form>
                }
            });
            view! {
                <li>
                    <strong>{p.namn}</strong>
                    <span class="detalj">
                        " Skapad " {p.skapad}
                        {p.senast_använd.map(|t| format!(", senast använd {t}"))}
                    </span>
                    {hantera}
                </li>
            }
        })
        .collect_view();
    let åtgärder = if färsk {
        view! {
            <form class="formular" data-passkey="lagg-till" data-efter="/passkeys">
                <label for="namn">"Namn på den nya passkeyn"</label>
                <input id="namn" name="namn" type="text" placeholder="t.ex. iPhone" />
                <button class="knapp" type="submit">"Lägg till passkey"</button>
                <p class="fel" role="alert" data-fel hidden></p>
            </form>
        }
        .into_any()
    } else {
        view! {
            <form class="formular" data-passkey="bekrafta" data-efter="/passkeys">
                <p>"Använd din passkey för att lägga till, byta namn på eller ta bort passkeys."</p>
                <button class="knapp" type="submit">"Använd passkey"</button>
                <p class="fel" role="alert" data-fel hidden></p>
            </form>
        }
        .into_any()
    };
    view! {
        <ul class="lista">{rader}</ul>
        {åtgärder}
    }
}

#[component]
pub fn Sessioner() -> impl IntoView {
    if !inloggad() {
        return view! { <LoggaIn /> }.into_any();
    }
    let sessioner = kontodata().sessioner;
    view! {
        <section class="sida">
            <h1>"Sessioner"</h1>
            <p>"Du är inloggad på de här enheterna. Avsluta en session för att logga ut enheten."</p>
            {sessioner.map(|sessioner| view! { <SessionLista sessioner /> })}
        </section>
    }
    .into_any()
}

#[component]
fn SessionLista(sessioner: Vec<SessionVy>) -> impl IntoView {
    let rader = sessioner
        .into_iter()
        .map(|s| {
            view! {
                <li>
                    <strong>{s.enhet}</strong>
                    {s.denna.then(|| view! { <span class="markering">" Den här sessionen"</span> })}
                    <span class="detalj">
                        " Inloggad " {s.skapad} ", senast aktiv " {s.senast_aktiv}
                    </span>
                    <form class="rad-formular" method="post" action="/konto/sessioner/avsluta">
                        <input type="hidden" name="id" value=s.id />
                        <button class="knapp knapp-diskret" type="submit">"Avsluta"</button>
                    </form>
                </li>
            }
        })
        .collect_view();
    view! { <ul class="lista">{rader}</ul> }
}
