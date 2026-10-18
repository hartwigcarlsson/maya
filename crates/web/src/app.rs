//! Skalet: dokumentet runt sidorna och appens rot. Körs på servern (SSR) och hydreras i
//! webbläsaren.

use leptos::prelude::*;
use leptos_meta::{MetaTags, Stylesheet, Title, provide_meta_context};
use leptos_router::components::{Route, Router, Routes};
use leptos_router::path;

/// Sätter `data-theme` ur det sparade valet innan sidan ritas, så att ett manuellt valt läge
/// inte blinkar till i systemets läge.
const TEMA_FÖRE_RITNING: &str = "try{const t=localStorage.getItem('maya-tema');\
if(t==='light'||t==='dark')document.documentElement.dataset.theme=t}catch(e){}";

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

    view! {
        <Stylesheet id="maya" href="/pkg/maya.css" />
        <Title text="Maya" />
        <Router>
            <header class="toppmeny">
                <a class="logotyp" href="/">
                    "Maya"
                </a>
                <TemaVäxlare />
            </header>
            <main>
                <Routes fallback=|| view! { <SidanFinnsInte /> }>
                    <Route path=path!("") view=Startsida />
                </Routes>
            </main>
        </Router>
    }
}

#[component]
fn Startsida() -> impl IntoView {
    view! {
        <section class="tom-sida">
            <h1>"Välkommen till Maya"</h1>
            <p>"Här kommer bokföringen att finnas."</p>
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
