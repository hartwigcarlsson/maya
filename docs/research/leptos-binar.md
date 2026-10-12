# Leptos som en enda körbar binär

Forskningsunderlag för [#9](https://github.com/hartwigcarlsson/maya/issues/9) (karta: #1). Insamlat 2026-10-12.

**Fråga:** Hur byggs en Leptos-applikation (aktuell version) som en enda körbar binär med inbäddade assets? Hur står sig SSR + hydrering (med Axum) mot ren CSR i det upplägget: byggverktyg (cargo-leptos), server functions, delning av domänlogik med en MCP-server i samma process, mognad och kända problem?

## Kort svar

Det går, och det är ett mönster som Leptos-repot självt visar upp, men det är inte standardläget. Standardläget med `cargo leptos build` är en server-binär plus en `site/`-katalog (JS, WASM, CSS, assets) som läses från disk vid körning. För att få en fil:

1. Bygg med cargo-leptos som vanligt (SSR-binär med feature `ssr`, WASM-bibliotek med `hydrate`).
2. Bädda in `site-root` i server-binären med `rust-embed` och ersätt `leptos_axum::file_and_error_handler` (som läser från disk) med en egen fallback-handler som serverar från det inbäddade.
3. Se till att frontend byggs *före* servern. Det gör cargo-leptos bara i vissa lägen.
4. Låt bli `hash-files`. Den kräver en `hash.txt` bredvid binären vid körning.

MCP-servern (`rmcp`) kan monteras i samma `axum::Router` med `nest_service("/mcp", …)`. Båda använder axum 0.8, och domänlogiken kan ligga i en vanlig crate som både server functions och MCP-verktyg anropar.

**Rekommendation för Maya:** SSR + hydrering via cargo-leptos och `leptos_axum`, med en inbäddad fallback-handler enligt `hackernews_islands_axum`. CSR är ett fungerande reservalternativ med samma binärupplägg (exemplet `todo_app_sqlite_csr`), men det har svagare stöd i verktyget.

## Versionsläge

- Leptos senaste stabila är **v0.8.22** (2026-10-05). **v0.9.0-beta** släpptes 2026-07-18 som förhandsversion [[releases]](https://github.com/leptos-rs/leptos/releases). Enligt release-noterna för 0.9-betan är de flesta ändringarna API-putsning: `lazy` blir en feature, server functions byter till `serde_qs` 1.0, signaler anropas via deref och edition blir 2024. Ingen större migrering väntas [[v0.9.0-beta]](https://github.com/leptos-rs/leptos/releases/tag/v0.9.0-beta).
- `leptos_axum` 0.8.10 beror på `axum ^0.8`, `tokio ^1.49` och `tower-http ^0.6` [[leptos_axum Cargo.toml @v0.8.22]](https://github.com/leptos-rs/leptos/blob/v0.8.22/integrations/axum/Cargo.toml).
- cargo-leptos senaste är **v0.3.11** (2026-10-02) [[cargo-leptos releases]](https://github.com/leptos-rs/cargo-leptos/releases).

## 1. Standardläget: binär + `site/`-katalog

- cargo-leptos kompilerar frontend-biblioteket till `wasm32-unknown-unknown` med `--no-default-features --features=hydrate` och server-binären med `--no-default-features --features=ssr` [[cargo-leptos README, Build features]](https://github.com/leptos-rs/cargo-leptos/blob/main/README.md#build-features).
- Allt frontend-output (JS, WASM, CSS, kopierade assets) hamnar i `site-root`, som standard `target/site`. Katalogen töms vid varje ombyggnad [[cargo-leptos README, Site parameters]](https://github.com/leptos-rs/cargo-leptos/blob/main/README.md#site-parameters).
- Leptos-bokens Dockerfiler kopierar därför binären *och* `target/site` till runtime-imagen och sätter `LEPTOS_SITE_ROOT` [[book: Deploying SSR Apps]](https://book.leptos.dev/deployment/ssr.html).
- `leptos_axum::file_and_error_handler` serverar statiska filer med `tower_http::services::ServeDir::new(options.site_root)` från disk, med förkomprimerade `.gz`/`.br` om de finns, och renderar annars appens 404-sida. Den är gated bakom `leptos_axum`s `default`-feature (som drar in `tower-http/fs`). Doc-kommentaren säger uttryckligen att man kan kopiera och anpassa den [[leptos_axum lib.rs @v0.8.22, `file_and_error_handler`/`get_static_file`]](https://github.com/leptos-rs/leptos/blob/v0.8.22/integrations/axum/src/lib.rs).

## 2. Att bädda in assets: rust-embed

### Mönstret i Leptos-repot

Exemplet `hackernews_islands_axum` gör precis detta. `#[derive(Embed)] #[folder = "target/site/"] struct Assets;` plus en egen `file_and_error_handler` som:
- slår upp `Assets::get(path)`,
- i release väljer `<path>.br` eller `<path>.gz` utifrån `Accept-Encoding` och sätter `Content-Encoding`,
- gissar `Content-Type` med `mime_guess` och sätter `Cache-Control: max-age=86400` i release

[[examples/hackernews_islands_axum/src/fallback.rs @v0.8.22]](https://github.com/leptos-rs/leptos/blob/v0.8.22/examples/hackernews_islands_axum/src/fallback.rs), med `rust-embed = { version = "8.5", features = ["axum", "mime_guess", "tokio"] }` [[Cargo.toml]](https://github.com/leptos-rs/leptos/blob/v0.8.22/examples/hackernews_islands_axum/Cargo.toml).

Notera:
- Exempel-handlern ger en enkel textsvar-404 i stället för appens 404-sida. Den som vill ha Leptos-renderad 404 måste falla tillbaka på `leptos_axum::handle_response_inner`/`render_app_to_stream` som originalet gör [[leptos_axum lib.rs]](https://github.com/leptos-rs/leptos/blob/v0.8.22/integrations/axum/src/lib.rs). (Slutsats av koden, inte dokumenterat.)
- Handlern antar att `.br`/`.gz` finns i release. Det stämmer bara om man bygger med `--precompress` (se nedan). (Slutsats av koden.)
- Användare efterfrågar en officiell mall för detta (`start-embeded-assets`). Det beskrivs som "multiple hours of work" att sätta upp, och någon mall finns inte än [[discussion #4660]](https://github.com/leptos-rs/leptos/discussions/4660).

### rust-embed-beteende som spelar roll

- Aktuell version: 8.13.0 [[crates.io]](https://crates.io/crates/rust-embed). Features: `axum-ex`, `compression`, `debug-embed`, `deterministic-timestamps`, `include-exclude`, `interpolate-folder-path`, `mime-guess` m.fl. [[crates.io 8.13.0]](https://crates.io/crates/rust-embed/8.13.0).
- I **release** bäddas filerna in i binären. I **debug** läses de från filsystemet vid varje anrop, om man inte slår på `debug-embed` [[docs.rs: trait RustEmbed]](https://docs.rs/rust-embed/latest/rust_embed/trait.RustEmbed.html). Det passar `cargo leptos watch` i utveckling.
- En relativ `folder` löses mot `CARGO_MANIFEST_DIR` vid kompilering. Med `interpolate-folder-path` kan man skriva t.ex. `"$CARGO_MANIFEST_DIR/…"` [[rust-embed readme / impl]](https://pyrossh.dev/repos/rust-embed). I en workspace är cargo-leptos `site-root` relativ till *workspace*-roten [[cargo-leptos README]](https://github.com/leptos-rs/cargo-leptos/blob/main/README.md#site-parameters), så sökvägen i `#[folder]` måste justeras därefter. cargo-leptos sätter alltid `LEPTOS_SITE_ROOT` i miljön för byggprocesserna [[cargo-leptos `Project::to_envs`]](https://github.com/leptos-rs/cargo-leptos/blob/main/_autodocs/api-reference/config-project.md). Det borde gå att använda via `interpolate-folder-path`, men det har inte verifierats.

### Byggordningen: frontend måste vara klar innan servern kompileras

rust-embed läser `site-root` när *server-crate:n* kompileras. Därför måste WASM/JS/CSS vara färdiga innan dess.

- I `cargo leptos build` körs frontend, assets, style och server **parallellt**, utom när `hash_files` är på eller när `release && precompress` gäller. I de fallen byggs frontend först och servern sist. Kommentaren i koden säger uttryckligen att förkomprimeringen görs före serverbygget "to make it possible to include them as assets into the binary itself" [[cargo-leptos src/command/build.rs]](https://github.com/leptos-rs/cargo-leptos/blob/main/src/command/build.rs).
- Flaggor: `-P/--precompress` (gzip + brotli, bara release), `--frontend-only`, `--server-only` [[cargo-leptos src/config/cli.rs]](https://github.com/leptos-rs/cargo-leptos/blob/main/src/config/cli.rs).
- Tidigare rapport om just rust-embed och byggordning: [[cargo-leptos #47]](https://github.com/leptos-rs/cargo-leptos/issues/47).

Två robusta varianter för Mayas Dockerfile:
- `cargo leptos build --release --precompress`, som ger sekventiell ordning och `.br`/`.gz` som exempel-handlern förväntar sig, **eller**
- två steg: `cargo leptos build --release --frontend-only` följt av `cargo leptos build --release --server-only`.

### Runtime-konfiguration utan filer

- `get_configuration(None)` läser `LeptosOptions` från miljövariabler med standardvärden (`LEPTOS_SITE_ADDR` → `127.0.0.1:3000` osv.). `output_name` faller tillbaka på `option_env!("LEPTOS_OUTPUT_NAME")`, alltså värdet som cargo-leptos satte **vid kompileringen** [[leptos_config lib.rs @v0.8.22, `try_from_env`/`get_configuration`]](https://github.com/leptos-rs/leptos/blob/v0.8.22/leptos_config/src/lib.rs). Därför behövs ingen `Cargo.toml` vid körning. I Docker räcker `LEPTOS_SITE_ADDR=0.0.0.0:…`.
- Alternativ: `get_config_from_str(include_str!(…/Cargo.toml))` bäddar in konfigurationen [[leptos #1633, kommentar]](https://github.com/leptos-rs/leptos/issues/1633).

### Fallgrop: `hash-files`

Med `hash-files = true` läser Leptos `hash.txt` från **katalogen där den körbara filen ligger** (`std::env::current_exe().parent()`) vid körning, och kräver dessutom `LEPTOS_HASH_FILES=true` vid körning [[leptos/src/hydration/mod.rs @v0.8.22]](https://github.com/leptos-rs/leptos/blob/v0.8.22/leptos/src/hydration/mod.rs), [[cargo-leptos README]](https://github.com/leptos-rs/cargo-leptos/blob/main/README.md#compilation-parameters). Det går inte ihop med en enda fil. Problemet är rapporterat men olöst [[discussion #4680]](https://github.com/leptos-rs/leptos/discussions/4680). Notera också att `hackernews_islands_axum` skriver `hash_file = true` (understreck), vilket enligt samma tråd sannolikt inte aktiverar funktionen alls. **Slutsats:** kör utan `hash-files` och hantera cache-busting på annat sätt, t.ex. kort `Cache-Control` eller version i sökvägen.

## 3. SSR + hydrering mot CSR

| | SSR + hydrering (`leptos_axum`) | CSR + server functions |
|---|---|---|
| Byggverktyg | cargo-leptos, primärt fall | cargo-leptos med `lib-features = ["csr"]` (exempel finns). README säger dock "client-side rendering mode not supported" |
| Routing på server | `.leptos_routes(…)` registrerar sidor **och** server functions | Manuell route `/api/{*fn_name}` → `leptos_axum::handle_server_fns`, plus fallback som ger `index.html` |
| Förstaladdning | Server-renderad HTML, sedan hydrering | Tom skal-HTML, WASM renderar allt |
| Felklass | Hydreringsbuggar (mismatch server/klient, ogiltig HTML, kod som bara kan köras på ena sidan) | Inga hydreringsbuggar |
| Inbäddning | Samma rust-embed-mönster | Samma rust-embed-mönster |

Källor:
- cargo-leptos README, Features: "Build server and client for hydration (client-side rendering mode not supported)" [[README]](https://github.com/leptos-rs/cargo-leptos/blob/main/README.md#features).
- Ändå bygger Leptos-repots `todo_app_sqlite_csr` med cargo-leptos: `bin-features = ["ssr"]`, `lib-features = ["csr"]`. Servern registrerar `.route("/api/{*fn_name}", post(leptos_axum::handle_server_fns))` och en `file_or_index_handler`. Kommentaren säger "we're not actually doing server side rendering". WASM-entry heter `hydrate()` men anropar `mount_to_body` [[examples/todo_app_sqlite_csr @v0.8.22: main.rs, lib.rs, Cargo.toml]](https://github.com/leptos-rs/leptos/tree/v0.8.22/examples/todo_app_sqlite_csr).
- Server functions fungerar genom villkorlig kompilering. På servern blir de HTTP-endpoints, i klientbygget ersätts kroppen med ett HTTP-anrop [[book: Server Functions]](https://book.leptos.dev/server/25_server_functions.html). De är "part of the public API" och kan anropas av vilken HTTP-klient som helst, får inte vara generiska och kräver serialiserbara argument [[server_fn crate docs @v0.8.22]](https://github.com/leptos-rs/leptos/blob/v0.8.22/server_fn/src/lib.rs).
- Registreringen sker automatiskt via `inventory`. `ServerFn::register_explicit()` finns för miljöer där `inventory` inte fungerar [[server_fn lib.rs @v0.8.22]](https://github.com/leptos-rs/leptos/blob/v0.8.22/server_fn/src/lib.rs).
- Bokens kapitel om hydreringsbuggar listar de typiska felklasserna: mismatch mellan server- och klientkod, ogiltig HTML (t.ex. `<table>` utan `<tbody>`), klientkod som inte kan köras på servern och tvärtom [[book: Hydration Bugs]](https://book.leptos.dev/ssr/24_hydration_bugs.html).
- För ren CSR utan server beskriver boken Trunk och en `dist/`-katalog [[book: Deploying CSR Apps]](https://book.leptos.dev/deployment/csr.html). Det ger ingen backend och passar därför inte Maya.

**Bedömning (inferens):** För ett bokföringssystem med formulär och listor väger SSR:s fördelar, snabb första rendering och progressiv förbättring med `<ActionForm>`, tyngre än risken för hydreringsbuggar. SSR är också läget som cargo-leptos och `leptos_axum` uttryckligen stöder. CSR är en möjlig reträtt om hydreringsbuggar blir ett problem, och den kräver ingen ändring av binärupplägget.

## 4. MCP-server i samma process

- Den officiella Rust-SDK:n `rmcp` (3.5.1) [[crates.io]](https://crates.io/crates/rmcp) exponerar Streamable HTTP som en tower-service: `StreamableHttpService::new(|| Ok(handler), LocalSessionManager::default().into(), config)`, monterad med `axum::Router::new().nest_service("/mcp", service)` [[rust-sdk examples/servers/src/counter_streamhttp.rs]](https://github.com/modelcontextprotocol/rust-sdk/blob/main/examples/servers/src/counter_streamhttp.rs).
- `rmcp` 3.5.1 beror på `axum ^0.8`, samma major som `leptos_axum` 0.8.10 [[crates.io deps rmcp]](https://crates.io/crates/rmcp/3.5.1/dependencies), [[crates.io deps leptos_axum]](https://crates.io/crates/leptos_axum/0.8.10/dependencies). Därmed kan samma `Router` bära Leptos-rutter, server functions, `/mcp` och den inbäddade fallbacken.
- Delat tillstånd till server functions: `leptos_routes_with_context(&state, routes, move || provide_context(state.clone()), App)` och `expect_context::<AppState>()` i server-funktionen, eller `extract_with_state` för axum-extractors [[book: Extractors]](https://book.leptos.dev/server/26_extractors.html).

**Föreslagen struktur (inferens, ej verifierad i kod):**
- `crates/domain` är ren Rust (sqlx, bokföringsregler) utan Leptos-beroenden.
- `crates/app` innehåller Leptos-komponenter och `#[server]`-funktioner. Under `ssr` anropar de `domain` med en `AppState { pool: SqlitePool, … }` från context.
- `crates/server` (bin) bygger `AppState`, `Router` med `.leptos_routes_with_context(…)`, `.nest_service("/mcp", rmcp-service som håller en klon av AppState)` och `.fallback(inbäddad handler)`.
- MCP-verktyg och server functions blir två tunna adaptrar över samma domän-API. Server functions ska inte anropas från MCP, eftersom de är HTTP-endpoints med egna serialiseringsregler.

## 5. Mognad och kända problem

- **Ingen officiell mall för inbäddning.** Mönstret finns bara i ett exempel och efterfrågas av användare [[#4660]](https://github.com/leptos-rs/leptos/discussions/4660).
- **`hash-files` och en enda fil går inte ihop** (se ovan) [[#4680]](https://github.com/leptos-rs/leptos/discussions/4680).
- **Byggordningen** är implicit kopplad till `--precompress`/`hash-files` [[build.rs]](https://github.com/leptos-rs/cargo-leptos/blob/main/src/command/build.rs).
- **wasm-bindgen-versioner:** `wasm-bindgen-cli` måste matcha `wasm-bindgen`-crate:ns patch-version exakt. cargo-leptos laddar ner rätt version, utom när en global installation finns [[cargo-leptos README, wasm-bindgen version management]](https://github.com/leptos-rs/cargo-leptos/blob/main/README.md#note-wasm-bindgen-version-management). Lås versionen i Docker-bygget.
- **CSR-stödet i cargo-leptos** är motsägelsefullt dokumenterat: README säger "not supported" medan ett officiellt exempel gör det.
- **0.9 är i beta.** En uppgradering ser liten ut enligt release-noterna [[v0.9.0-beta]](https://github.com/leptos-rs/leptos/releases/tag/v0.9.0-beta).
- Bokens Docker-exempel använder både nightly och stable Rust [[book: Deploying SSR Apps]](https://book.leptos.dev/deployment/ssr.html). Nightly behövs inte för grundfunktionaliteten. I 0.9 fungerar även funktionsanropssyntaxen för signaler på stable [[v0.9.0-beta]](https://github.com/leptos-rs/leptos/releases/tag/v0.9.0-beta).

## Öppna frågor

- Triggar en ändring som bara rör `site/` (t.ex. CSS) en omkompilering av server-crate:n i release? rust-embed borde spåra filerna via `include_bytes!`, men det är inte verifierat.
- Fungerar `#[folder = "$LEPTOS_SITE_ROOT"]` med `interpolate-folder-path` i ett workspace-bygge?
- Hur ger man Leptos-renderade 404-sidor från den inbäddade fallbacken? Det kräver att man kombinerar exemplets handler med `handle_response_inner`.
- Vilken cache-busting-strategi ersätter `hash-files`?
- Auth: server functions och `/mcp` är båda publika HTTP-endpoints och behöver ett gemensamt autentiseringslager (axum-middleware).
