//! Användarmenyns sidor för passkeys och sessioner. Sidorna läser med server functions och
//! ändrar med vanliga formulär (`/konto/…`) och passkey-ceremonierna i JS-shimmen.

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

#[cfg(feature = "ssr")]
mod server {
    use leptos::prelude::*;
    use maya_core::inloggning::Inloggad;
    use maya_core::{Databas, Klocka};

    pub fn inloggad() -> Result<Inloggad, ServerFnError> {
        use_context::<axum::http::request::Parts>()
            .and_then(|parts| parts.extensions.get::<Inloggad>().cloned())
            .ok_or_else(|| ServerFnError::new("du är inte inloggad"))
    }

    pub fn databas() -> Result<(Databas, Klocka), ServerFnError> {
        Ok((
            use_context::<Databas>().ok_or_else(|| ServerFnError::new("databasen saknas"))?,
            use_context::<Klocka>().ok_or_else(|| ServerFnError::new("klockan saknas"))?,
        ))
    }

    pub fn fel(e: impl std::fmt::Display) -> ServerFnError {
        tracing::error!("kontosidan misslyckades: {e}");
        ServerFnError::new("något gick fel i Maya; försök igen")
    }

    /// En tidpunkt i Stockholm, till minuten.
    pub fn tid(t: jiff::Timestamp) -> String {
        t.to_zoned(maya_domain::tid::stockholm())
            .strftime("%Y-%m-%d %H:%M")
            .to_string()
    }
}

#[server]
pub async fn hämta_passkeys() -> Result<PasskeySida, ServerFnError> {
    let inloggad = server::inloggad()?;
    let (db, klocka) = server::databas()?;
    let passkeys = maya_core::inloggning::passkeys(&db, &inloggad)
        .await
        .map_err(server::fel)?;
    Ok(PasskeySida {
        färsk: inloggad.har_färsk_passkey(&klocka),
        passkeys: passkeys
            .into_iter()
            .map(|p| PasskeyVy {
                id: p.id,
                namn: p.namn,
                skapad: server::tid(p.skapad),
                senast_använd: p.senast_använd.map(server::tid),
            })
            .collect(),
    })
}

#[server]
pub async fn hämta_sessioner() -> Result<Vec<SessionVy>, ServerFnError> {
    let inloggad = server::inloggad()?;
    let (db, _) = server::databas()?;
    let sessioner = maya_core::inloggning::sessioner(&db, &inloggad)
        .await
        .map_err(server::fel)?;
    Ok(sessioner
        .into_iter()
        .map(|s| SessionVy {
            id: s.id,
            enhet: s.enhet.unwrap_or_else(|| "Okänd webbläsare".into()),
            skapad: server::tid(s.skapad),
            senast_aktiv: server::tid(s.senast_aktiv),
            denna: s.denna,
        })
        .collect())
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
    let sida = Resource::new(|| (), |_| hämta_passkeys());
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
            <Suspense fallback=|| view! { <p>"Hämtar passkeys…"</p> }>
                {move || Suspend::new(async move {
                    match sida.await {
                        Ok(sida) => view! { <PasskeyLista sida /> }.into_any(),
                        Err(e) => view! { <p class="fel">{e.to_string()}</p> }.into_any(),
                    }
                })}
            </Suspense>
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
    let sessioner = Resource::new(|| (), |_| hämta_sessioner());
    view! {
        <section class="sida">
            <h1>"Sessioner"</h1>
            <p>"Du är inloggad på de här enheterna. Avsluta en session för att logga ut enheten."</p>
            <Suspense fallback=|| view! { <p>"Hämtar sessioner…"</p> }>
                {move || Suspend::new(async move {
                    match sessioner.await {
                        Ok(sessioner) => view! { <SessionLista sessioner /> }.into_any(),
                        Err(e) => view! { <p class="fel">{e.to_string()}</p> }.into_any(),
                    }
                })}
            </Suspense>
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
