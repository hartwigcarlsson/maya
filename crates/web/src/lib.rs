//! Webben: Leptos med SSR och full hydrering. Server functions och sidor är tunna adaptrar mot
//! `maya-core` (ADR 0003).

// Formulären med många fält ger djupa vytyper.
#![recursion_limit = "256"]

pub mod app;
pub mod foretag;
pub mod konto;
pub mod kontoplan;
pub mod verifikat;

#[cfg(feature = "ssr")]
mod assets;
#[cfg(feature = "ssr")]
mod inloggning;
#[cfg(feature = "ssr")]
mod server;

#[cfg(feature = "ssr")]
pub use assets::serve_embedded;
#[cfg(feature = "ssr")]
pub use server::{ClientIp, client_ip, router};

/// Startpunkten för WASM-bygget: hydrerar sidan som servern renderade.
#[cfg(feature = "hydrate")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn hydrate() {
    console_error_panic_hook::set_once();
    leptos::mount::hydrate_body(app::App);
    // Klar: röktesten väntar på den här markören innan de klickar.
    if let Some(rot) = leptos::prelude::document().document_element() {
        let _ = rot.set_attribute("data-hydrerad", "");
    }
}
