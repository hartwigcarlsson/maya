//! Skalet: dokumentet runt sidorna och appens rot. Körs på servern (SSR) och hydreras i
//! webbläsaren.

use leptos::prelude::*;
use leptos_meta::{MetaTags, Stylesheet, Title, provide_meta_context};
use leptos_router::components::{Route, Router, Routes};
use leptos_router::hooks::use_params_map;
use leptos_router::path;
use serde::{Deserialize, Serialize};

use crate::atkomst::ÅtkomstSida;
use crate::behandlingshistorik::BehandlingshistorikSida;
use crate::bokforing::{GrundbokSida, HuvudbokSida};
use crate::foretag::{AttGöra, Företagslista, NyttFöretagSida, Toppmeny, företagsval};
use crate::ingaende_balans::IngåendeBalansSida;
use crate::installningar::FöretagsInställningar;
use crate::konto::{Passkeys, Sessioner};
use crate::kontoplan::KontoplanSida;
use crate::rapporter::{Balansräkning, Rapporter, Resultaträkning};
use crate::sok::SökSida;
use crate::underlag::UnderlagSida;
use crate::verifikat::{NyttVerifikatSida, VerifikatSida};

/// Vad sidan ska visa, avgjort på servern ur sessionscookien.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Läge {
    /// Installationen har ingen användare än: bara hänvisningen till `maya user create`.
    IngenAnvändare,
    Utloggad,
    Inloggad {
        visningsnamn: String,
    },
}

/// Läget som middleware lade på begäran. Följer med till webbläsaren vid hydreringen.
fn läge() -> Läge {
    SharedValue::new(|| {
        #[cfg(feature = "ssr")]
        if let Some(parts) = use_context::<axum::http::request::Parts>()
            && let Some(läge) = parts.extensions.get::<Läge>()
        {
            return läge.clone();
        }
        Läge::Utloggad
    })
    .into_inner()
}

/// Sätter `data-theme` ur det sparade valet innan sidan ritas, så att ett manuellt valt läge
/// inte blinkar till i systemets läge.
const TEMA_FÖRE_RITNING: &str = "try{const t=localStorage.getItem('maya-tema');\
if(t==='light'||t==='dark')document.documentElement.dataset.theme=t}catch(e){}";

/// `/` fokuserar sökfältet på en sida som har ett (kontoplanen), utom när man skriver i ett fält.
const SNABBTANGENT_SÖK: &str = "document.addEventListener('keydown',function(e){\
if(e.key!=='/'||e.ctrlKey||e.metaKey||e.altKey)return;var t=e.target;\
if(t&&(t.isContentEditable||/^(INPUT|TEXTAREA|SELECT)$/.test(t.tagName)))return;\
var f=document.getElementById('kontosok');if(f){e.preventDefault();f.focus();f.select()}})";

/// Dokumentet som servern renderar runt appen.
pub fn shell(options: LeptosOptions) -> impl IntoView {
    view! {
        <!DOCTYPE html>
        <html lang="sv">
            <head>
                <meta charset="utf-8" />
                <meta name="viewport" content="width=device-width, initial-scale=1" />
                <meta name="color-scheme" content="light dark" />
                <link rel="icon" href="/favicon.svg" type="image/svg+xml" />
                <script>{TEMA_FÖRE_RITNING}</script>
                <script>{SNABBTANGENT_SÖK}</script>
                <script src="/auth/webauthn.js" defer></script>
                <HydrationScripts options />
                <MetaTags />
            </head>
            <body>
                <App />
            </body>
        </html>
    }
}

#[component]
pub fn App() -> impl IntoView {
    provide_meta_context();
    let läge = läge();
    provide_context(läge.clone());
    provide_context(företagsval());
    let användarmeny = match läge {
        Läge::Inloggad { visningsnamn } => Some(view! {
            <span class="anvandare">{visningsnamn}</span>
            <a class="knapp knapp-diskret" href="/passkeys" rel="external">
                "Passkeys"
            </a>
            <a class="knapp knapp-diskret" href="/sessioner" rel="external">
                "Sessioner"
            </a>
            <form method="post" action="/auth/utloggning">
                <button class="knapp knapp-diskret" type="submit">
                    "Logga ut"
                </button>
            </form>
        }),
        _ => None,
    };

    view! {
        <Stylesheet id="maya" href="/pkg/maya.css" />
        <Title text="Maya" />
        <Router>
            <header class="toppmeny">
                <a class="logotyp" href="/">
                    "Maya"
                </a>
                <Toppmeny />
                <div class="toppmeny-hoger">
                    {användarmeny}
                    <TemaVäxlare />
                </div>
            </header>
            <main>
                <Innehåll />
            </main>
        </Router>
    }
}

#[component]
fn Innehåll() -> impl IntoView {
    if use_context::<Läge>() == Some(Läge::IngenAnvändare) {
        view! { <IngenAnvändare /> }.into_any()
    } else {
        view! {
            <Routes fallback=|| view! { <SidanFinnsInte /> }>
                <Route path=path!("") view=Startsida />
                <Route path=path!("registrera/:token") view=Registrera />
                <Route path=path!("passkeys") view=Passkeys />
                <Route path=path!("sessioner") view=Sessioner />
                <Route path=path!("foretag/nytt") view=NyttFöretagSida />
                <Route path=path!("foretag/:id") view=AttGöra />
                <Route path=path!("foretag/:id/verifikat") view=VerifikatSida />
                <Route path=path!("foretag/:id/verifikat/nytt") view=NyttVerifikatSida />
                <Route path=path!("foretag/:id/rapporter") view=Rapporter />
                <Route path=path!("foretag/:id/rapporter/grundbok") view=GrundbokSida />
                <Route path=path!("foretag/:id/rapporter/huvudbok") view=HuvudbokSida />
                <Route path=path!("foretag/:id/rapporter/resultatrakning") view=Resultaträkning />
                <Route path=path!("foretag/:id/rapporter/balansrakning") view=Balansräkning />
                <Route path=path!("foretag/:id/sok") view=SökSida />
                <Route path=path!("foretag/:id/underlag") view=UnderlagSida />
                <Route path=path!("foretag/:id/installningar") view=FöretagsInställningar />
                <Route
                    path=path!("foretag/:id/installningar/ingaende-balans")
                    view=IngåendeBalansSida
                />
                <Route path=path!("foretag/:id/installningar/kontoplan") view=KontoplanSida />
                <Route path=path!("foretag/:id/installningar/atkomst") view=ÅtkomstSida />
                <Route
                    path=path!("foretag/:id/installningar/behandlingshistorik")
                    view=BehandlingshistorikSida
                />
            </Routes>
        }
        .into_any()
    }
}

/// Innan någon användare finns: bara hänvisningen till CLI:t, så att ingen främling kan ta
/// över en ny installation.
#[component]
fn IngenAnvändare() -> impl IntoView {
    view! {
        <section class="tom-sida">
            <h1>"Välkommen till Maya"</h1>
            <p>"Maya har ingen användare än. Skapa den första på servern:"</p>
            <pre class="kommando">"maya user create <e-post>"</pre>
            <p>"Kommandot skriver ut en länk där du registrerar din passkey."</p>
        </section>
    }
}

#[component]
fn Startsida() -> impl IntoView {
    match use_context::<Läge>() {
        Some(Läge::Inloggad { .. }) => view! { <Företagslista /> }.into_any(),
        _ => view! { <LoggaIn /> }.into_any(),
    }
}

/// E-post först och sedan passkey, utan conditional UI.
#[component]
pub(crate) fn LoggaIn() -> impl IntoView {
    view! {
        <section class="tom-sida">
            <h1>"Logga in"</h1>
            <form class="formular" data-passkey="logga-in">
                <label for="epost">"E-post"</label>
                <input id="epost" name="epost" type="email" autocomplete="username" required />
                <button class="knapp" type="submit">
                    "Fortsätt med passkey"
                </button>
                <p class="fel" role="alert" data-fel hidden></p>
            </form>
        </section>
    }
}

/// Sidan som registrerings- och återställningslänkarna från CLI:t och inbjudningslänkarna
/// leder till. Efteråt ber Maya användaren lägga till en passkey till.
#[component]
fn Registrera() -> impl IntoView {
    let token = use_params_map().with_untracked(|p| p.get("token").unwrap_or_default());
    view! {
        <section class="tom-sida">
            <h1>"Registrera passkey"</h1>
            <p>"Med en passkey loggar du in i Maya utan lösenord."</p>
            <form
                class="formular"
                data-passkey="registrera"
                data-token=token
                data-efter="/passkeys?ny=1"
            >
                <label for="namn">"Namn på passkeyn"</label>
                <input id="namn" name="namn" type="text" placeholder="t.ex. MacBook" />
                <button class="knapp" type="submit">
                    "Registrera passkey"
                </button>
                <p class="fel" role="alert" data-fel hidden></p>
            </form>
        </section>
    }
}

#[component]
fn SidanFinnsInte() -> impl IntoView {
    #[cfg(feature = "ssr")]
    if let Some(response) = use_context::<leptos_axum::ResponseOptions>() {
        response.set_status(axum::http::StatusCode::NOT_FOUND);
    }
    view! {
        <section class="tom-sida">
            <h1>"Sidan finns inte"</h1>
            <p>
                <a href="/">"Till startsidan"</a>
            </p>
        </section>
    }
}

/// Växlar mellan ljust och mörkt läge. Utan val följer Maya systemets läge.
#[component]
fn TemaVäxlare() -> impl IntoView {
    let växla = move |_| {
        #[cfg(feature = "hydrate")]
        växla_tema();
    };
    view! {
        <button class="knapp knapp-diskret" type="button" title="Växla ljust och mörkt läge" on:click=växla>
            "Ljust/mörkt"
        </button>
    }
}

#[cfg(feature = "hydrate")]
fn växla_tema() {
    let Some(rot) = document().document_element() else {
        return;
    };
    let mörkt_nu = match rot.get_attribute("data-theme").as_deref() {
        Some("dark") => true,
        Some("light") => false,
        _ => window()
            .match_media("(prefers-color-scheme: dark)")
            .ok()
            .flatten()
            .is_some_and(|m| m.matches()),
    };
    let nytt = if mörkt_nu { "light" } else { "dark" };
    let _ = rot.set_attribute("data-theme", nytt);
    if let Ok(Some(lagring)) = window().local_storage() {
        let _ = lagring.set_item("maya-tema", nytt);
    }
}
