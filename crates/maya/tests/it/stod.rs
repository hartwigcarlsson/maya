//! Stöd för webbläsartesterna (`rok`, `hydrering`, `klientnavigering`).

use std::path::Path;

/// Om sajten från `cargo leptos build` finns, så att sidorna kan hydreras. Utan den hoppar ett
/// webbläsartest över sig självt lokalt, men i CI (där `CI` är satt) misslyckas det: ett
/// överhoppat test vore grönt utan att ha prövat något.
pub fn sajten_finns() -> bool {
    let wasm = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/site/pkg/maya.wasm");
    if wasm.exists() {
        return true;
    }
    assert!(
        std::env::var_os("CI").is_none(),
        "{} saknas: CI ska köra `cargo leptos build` före testerna",
        wasm.display()
    );
    eprintln!(
        "hoppar över: {} saknas, kör `cargo leptos build`",
        wasm.display()
    );
    false
}
