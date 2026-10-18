//! Användarmenyns sidor för passkeys och sessioner. Sidorna läser sin data med
//! `sidodata::sida` och ändrar med vanliga formulär (`/konto/…`) och passkey-ceremonierna i
//! JS-shimmen.

use leptos::prelude::*;
use leptos_router::hooks::use_query_map;
use serde::{Deserialize, Serialize};

use crate::app::{LoggaIn, Läge};
use crate::sidodata::sida;

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

/// Det som sidorna Passkeys och Sessioner visar.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Kontodata {
    pub passkeys: Option<PasskeySida>,
    pub sessioner: Option<Vec<SessionVy>>,
}

/// Datan för `/passkeys` och `/sessioner`.
#[server(prefix = "/api", endpoint = "kontodata", input = server_fn::codec::GetUrl)]
pub async fn hämta_kontodata(adress: String) -> Result<Kontodata, ServerFnError> {
    match crate::sidodata::Sidbegäran::för(&adress)? {
        Some(begäran) => server::kontodata(&begäran).await,
        None => Ok(Kontodata::default()),
    }
}

#[cfg(feature = "ssr")]
mod server {
    use leptos::prelude::ServerFnError;
    use maya_core::inloggning::{self, Inloggad, InloggningsFel};
    use maya_core::{Databas, Klocka};

    use super::{Kontodata, PasskeySida, PasskeyVy, SessionVy};
    use crate::sidodata::{Sidbegäran, serverfel};

    /// Hämtar det som `/passkeys` och `/sessioner` visar.
    pub(crate) async fn kontodata(b: &Sidbegäran) -> Result<Kontodata, ServerFnError> {
        let data = match b.delar().as_slice() {
            ["passkeys"] => passkeys(&b.db, &b.klocka, &b.inloggad)
                .await
                .map(|p| Kontodata {
                    passkeys: Some(p),
                    sessioner: None,
                }),
            ["sessioner"] => sessioner(&b.db, &b.klocka, &b.inloggad)
                .await
                .map(|s| Kontodata {
                    passkeys: None,
                    sessioner: Some(s),
                }),
            _ => Ok(Kontodata::default()),
        };
        data.map_err(serverfel)
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
        klocka: &Klocka,
        inloggad: &Inloggad,
    ) -> Result<Vec<SessionVy>, InloggningsFel> {
        Ok(inloggning::sessioner(db, klocka, inloggad)
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

    use maya_domain::tid::minut as tid;
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
    sida(hämta_kontodata, passkeys)
}

fn passkeys(data: Kontodata) -> AnyView {
    if !inloggad() {
        return view! { <LoggaIn /> }.into_any();
    }
    let (ny, fel) = use_query_map().with_untracked(|q| (q.get("ny").is_some(), q.get("fel")));
    let sida = data.passkeys;
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
    sida(hämta_kontodata, sessioner)
}

fn sessioner(data: Kontodata) -> AnyView {
    if !inloggad() {
        return view! { <LoggaIn /> }.into_any();
    }
    let sessioner = data.sessioner;
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
