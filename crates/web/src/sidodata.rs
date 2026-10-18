//! Sidornas data: ett mönster för både serverrenderingen och klientens router.
//!
//! En sida som visar data ur `core` har en server function som tar sidans adress (sökväg och
//! fråga) och ger datan. Sidan läser den med [`sida`]: en `Resource` som hämtar om när adressen
//! ändras. På servern körs funktionen i samma begäran (routen har `ssr=SsrMode::Async`, så
//! sidan är färdig, med rätt statuskod, innan den skickas), och värdet följer med vid
//! hydreringen. När routern navigerar på klienten anropar `Resource` funktionen över HTTP.
//!
//! Det som hör till sessionen och bara ändras vid en hel sidladdning (`Läge`, `Företagsval`)
//! läggs på av middleware och delas med `SharedValue`. Sidans data gör det inte: den fanns bara
//! vid serverladdningen, så en sida som routern ritade på klienten blev tom eller gammal.

use std::future::Future;

use leptos::prelude::*;
use leptos_router::hooks::use_location;
use serde::Serialize;
use serde::de::DeserializeOwned;

/// Sidan: datan från `hämta` för den aktuella adressen, ritad med `vy`. Hela vyn ritas om när
/// adressen ändras, så `vy` får läsa sökvägen och frågan med `with_untracked`. Föregående vy
/// står kvar medan nästa hämtas.
pub fn sida<T, Fut, V>(
    hämta: impl Fn(String) -> Fut + Send + Sync + 'static,
    vy: impl Fn(T) -> V + Send + Sync + Clone + 'static,
) -> impl IntoView
where
    T: Clone + Serialize + DeserializeOwned + Send + Sync + 'static,
    Fut: Future<Output = Result<T, ServerFnError>> + Send + 'static,
    V: IntoView + 'static,
{
    let location = use_location();
    let data = Resource::new(
        move || match location.search.get() {
            fråga if fråga.is_empty() => location.pathname.get(),
            fråga => format!("{}?{fråga}", location.pathname.get()),
        },
        hämta,
    );
    view! {
        <Transition>
            {move || {
                let vy = vy.clone();
                Suspend::new(async move {
                    match data.await {
                        Ok(data) => vy(data).into_any(),
                        Err(_) => view! { <Serverfel /> }.into_any(),
                    }
                })
            }}
        </Transition>
    }
}

/// En sida utan egen data som läser sökvägen eller frågan med `with_untracked`: ritas om när
/// adressen ändras. Routern återanvänder annars vyn när bara parametrarna byts, t.ex. vid byte
/// från `/foretag/1` till `/foretag/2`.
pub fn per_adress<V: IntoView + 'static>(
    vy: impl Fn() -> V + Send + Sync + 'static,
) -> impl IntoView {
    let location = use_location();
    move || {
        location.pathname.track();
        location.search.track();
        untrack(&vy)
    }
}

/// Felet är redan loggat på servern.
#[component]
fn Serverfel() -> impl IntoView {
    #[cfg(feature = "ssr")]
    if let Some(response) = use_context::<leptos_axum::ResponseOptions>() {
        response.set_status(axum::http::StatusCode::INTERNAL_SERVER_ERROR);
    }
    view! {
        <section class="tom-sida">
            <h1>"Något gick fel i Maya"</h1>
            <p>"Försök igen."</p>
        </section>
    }
}

#[cfg(feature = "ssr")]
pub(crate) use server::{Sidbegäran, serverfel};

#[cfg(feature = "ssr")]
mod server {
    use std::sync::Arc;

    use axum::http::request::Parts;
    use leptos::prelude::*;
    use maya_core::config::Config;
    use maya_core::inloggning::Inloggad;
    use maya_core::{Databas, Klocka};

    /// Det en sidas server function behöver: konfigurationen, databasen, klockan, den inloggade
    /// och adressen. `server.rs` lägger konfigurationen, databasen och klockan i kontexten.
    pub(crate) struct Sidbegäran {
        pub config: Arc<Config>,
        pub db: Databas,
        pub klocka: Klocka,
        pub inloggad: Inloggad,
        sökväg: String,
        fråga: String,
    }

    impl Sidbegäran {
        /// Begäran för adressen `adress`, eller `None` utan inloggning (sidan visar då
        /// inloggningen, ur `Läge`).
        pub(crate) fn för(adress: &str) -> Result<Option<Self>, ServerFnError> {
            let parts = use_context::<Parts>().ok_or_else(|| serverfel("begäran saknas"))?;
            let Some(inloggad) = parts.extensions.get::<Inloggad>().cloned() else {
                return Ok(None);
            };
            let (sökväg, fråga) = adress.split_once('?').unwrap_or((adress, ""));
            Ok(Some(Sidbegäran {
                config: use_context().ok_or_else(|| serverfel("konfigurationen saknas"))?,
                db: use_context().ok_or_else(|| serverfel("databasen saknas"))?,
                klocka: use_context().ok_or_else(|| serverfel("klockan saknas"))?,
                inloggad,
                sökväg: sökväg.to_string(),
                fråga: fråga.to_string(),
            }))
        }

        /// Sökvägens delar, t.ex. `["foretag", "1", "verifikat"]`.
        pub(crate) fn delar(&self) -> Vec<&str> {
            self.sökväg.trim_matches('/').split('/').collect()
        }

        /// Frågan, utan `?`.
        pub(crate) fn fråga(&self) -> &str {
            &self.fråga
        }

        /// Frågans värde för `namn`.
        pub(crate) fn värde(&self, namn: &str) -> Option<String> {
            url::form_urlencoded::parse(self.fråga.as_bytes())
                .find(|(k, _)| k == namn)
                .map(|(_, v)| v.into_owned())
        }
    }

    /// Loggar felet och ger ett fel utan detaljer till sidan.
    pub(crate) fn serverfel(e: impl std::fmt::Display) -> ServerFnError {
        tracing::error!("sidans data: {e}");
        ServerFnError::new("något gick fel i Maya; försök igen")
    }
}
