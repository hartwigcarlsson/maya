# MCP över Streamable HTTP med egen OAuth-server: vad specen och Claude kräver

Forskningsunderlag för [#3](https://github.com/hartwigcarlsson/maya/issues/3) (karta: #1).
Insamlat 2026-10-12. Alla påståenden har en källa; där jag drar egna slutsatser står det **Slutsats** eller **Rekommendation**.

## Sammanfattning

- **Två spec-versioner gäller samtidigt.** Aktuell MCP-version är `2026-07-28` ([versioning][ver]), som gör protokollet tillståndslöst (ingen `initialize`, inga sessioner, ingen GET-ström) ([changelog][chg]). Claude följer däremot authorization-specarna `2025-03-26`, `2025-06-18` och `2025-11-25` ([Claude: Build an MCP server][c-idx]). Maya måste alltså tala minst `2025-11-25`-eran mot Claude idag, och gärna `2026-07-28` också. rmcp 3.5.1 stöder båda ([rmcp model.rs][rmcp-model]).
- **Maya blir både resource server (MCP-servern) och authorization server (AS).** MCP-servern MÅSTE publicera RFC 9728 Protected Resource Metadata; AS:en MÅSTE publicera RFC 8414-metadata eller OIDC Discovery ([auth 2026-07-28][a26]).
- **Klientregistrering: implementera CIMD, och DCR som reserv.** Specen säger SHOULD för Client ID Metadata Documents och har *deprecerat* DCR i `2026-07-28` ([client registration][creg], [changelog][chg]). Claude väljer CIMD bara om AS-metadata har både `client_id_metadata_document_supported: true` och `"none"` i `token_endpoint_auth_methods_supported`; annars faller Claude tillbaka till DCR ([Claude auth][c-auth]). Med CIMD behövs ingen klientdatabas alls för Claude.
- **PKCE S256 är obligatoriskt i praktiken.** Claude skickar alltid `code_challenge_method=S256`, och spec-följande klienter vägrar fortsätta om `code_challenge_methods_supported` saknas i metadata ([Claude auth][c-auth], [security 2026-07-28][sec26]).
- **Tokens:** korta access tokens (SHOULD), refresh tokens MÅSTE roteras för publika klienter, vilket Claude är via CIMD/DCR ([sec26], [c-auth]). Claude förnyar 5 min före utgång och vid 401, och kräver `invalid_grant` när en refresh token inte längre gäller ([c-auth]).
- **Nåbarhet är den stora praktiska frågan för en självhostad server.** claude.ai/Desktop/mobil ansluter från Anthropics infrastruktur: servern måste ha publik IPv4-adress (inga privata/CGNAT-adresser), och AS-endpoints MÅSTE vara HTTPS ([Claude troubleshooting][c-trbl], [sec26]). Claude Code ansluter däremot från användarens maskin och kan nå `localhost` ([Claude lazy auth][c-lazy]).
- **Rust:** rmcp (officiell SDK) ger Streamable HTTP-server, men dess `auth`-feature är *klientsidan*. Serversidans AS finns bara som exempel i repot, handskrivna med axum ([rmcp OAUTH_SUPPORT.md][rmcp-oauth], [rmcp examples][rmcp-ex]). oxide-auth har PKCE men varken RFC 8414-metadata, DCR, CIMD eller RFC 8707 och har inte släppts sedan 2024-06 (se avsnitt 6). **Rekommendation:** rmcp för MCP-transporten och en egen liten AS i axum.

## 1. Versionsläget

| | `2025-11-25` | `2026-07-28` (aktuell) |
|---|---|---|
| Handskakning | `initialize` + `notifications/initialized` | Borttagen; version och capabilities i varje requests `_meta`, plus obligatorisk `server/discover` ([chg]) |
| Sessioner | Servern MAY sätta `MCP-Session-Id`, 404 när den gått ut, DELETE för att avsluta ([t25]) | Borttagna; servern SHOULD ignorera `Mcp-Session-Id` och svara 405 på GET/DELETE ([shttp26]) |
| GET-ström | Servern MÅSTE svara SSE eller 405 ([t25]) | Borttagen; ersatt av `subscriptions/listen` ([chg]) |
| Server→klient-requests | På SSE-strömmar | Via MRTR (`InputRequiredResult`) ([shttp26]) |
| Headers | `MCP-Protocol-Version` ([t25]) | Dessutom `Mcp-Method`, `Mcp-Name` (REQUIRED), validering mot body, felkod `-32020` ([shttp26]) |
| DCR | MAY ([a25]) | MAY men deprecated ([a26], [chg]) |
| RFC 9207 `iss` | Nämns inte ([a25]) | AS SHOULD skicka `iss` i authorization response och annonsera `authorization_response_iss_parameter_supported` ([a26]) |

Claude anger stöd för authorization-specarna `2025-03-26`, `2025-06-18` och `2025-11-25`. Claude stöder Streamable HTTP och den äldre HTTP+SSE-transporten. Resource subscriptions och sampling stöds inte ([c-idx]).

**Slutsats:** Maya bör svara på både den `initialize`-baserade eran och `2026-07-28`. Specen beskriver hur klienter detekterar eran via 400-svarets body ([shttp26]). Ingen källa säger om Claude kräver sessioner. Specen gör dem valfria i `2025-11-25` (servern MAY tilldela en), så en tillståndslös server bör fungera. Det är obekräftat mot Claude.

## 2. Krav på MCP-servern (resource server)

**Transport (Streamable HTTP)**
- En enda MCP-endpoint, t.ex. `https://maya.example.se/mcp`. Den tar POST, och i `2025-11-25` även GET, där 405 räcker ([t25], [shttp26]).
- Svar på en request är `application/json` eller `text/event-stream`, och klienten MÅSTE stödja båda. Notifications ger `202` utan body ([shttp26]).
- Servern MÅSTE validera `Origin` och svara 403 vid ogiltig. Lokalt SHOULD den binda till 127.0.0.1 ([shttp26]).
- Okänd/ostödd `MCP-Protocol-Version` ger `400` ([t25], [shttp26]).

**Authorization**
- Servern MÅSTE implementera RFC 9728 Protected Resource Metadata. `authorization_servers` MÅSTE ha minst en post ([asd26]).
- Discovery: en `401` med `WWW-Authenticate: Bearer resource_metadata="…"` och/eller well-known-URI. Klienter provar `/.well-known/oauth-protected-resource/<mcp-path>` först, sedan `/.well-known/oauth-protected-resource` ([asd26]).
- Servern SHOULD skicka `scope=` i `WWW-Authenticate` ([a26]).
- Bearer-token i `Authorization`-headern på *varje* request, aldrig i query string ([a26]).
- Servern MÅSTE validera att token utfärdats för just den, alltså audience enligt RFC 8707. Ogiltig eller utgången token ger `401`. Servern får inte acceptera eller skicka vidare andra tokens ([a26], [sec26]).
- Otillräcklig scope SHOULD ge `403` med `error="insufficient_scope"`, `scope=…` och `resource_metadata` ([a26]).

**Claude-specifikt** ([c-auth], [c-lazy])
- Inloggning startar *bara* på en HTTP-`401` med `WWW-Authenticate`. En `WWW-Authenticate` på ett `200`-svar ignoreras. Ett tool-fel (`200` + `isError`) ger ingen inloggningsprompt. Kontrollen måste alltså göras i HTTP-lagret, före MCP-SDK:n.
- `resource` i PRM måste vara exakt den URL användaren matar in, med path.
- Claude använder bara *första* posten i `authorization_servers`.
- `403` ger omauktorisering bara tillsammans med `insufficient_scope`. Annars blir det ett terminalt fel.
- Claude cachar discovery-metadata globalt per URL i cirka 5 min.
- Claude skickar `resource` kanoniskt: gemener i scheme och host, inget avslutande `/`, ingen default-port. Servern bör jämföra `aud` mot den kanoniska formen ([c-trbl]).
- Redirects till annan host gör att `Authorization`-headern tappas. Registrera den slutliga URL:en ([c-trbl]).
- Tool-resultat får vara högst ~150 000 tecken (claude.ai) och 25 000 tokens (Claude Code), med 240 s timeout per anrop på claude.ai ([c-idx]).

## 3. Krav på authorization servern

**Obligatoriskt eller i praktiken obligatoriskt**
- AS:en MÅSTE implementera OAuth 2.1 (draft-ietf-oauth-v2-1-13) för både konfidentiella och publika klienter ([a26]).
- Metadata via RFC 8414 (`/.well-known/oauth-authorization-server`) eller OIDC Discovery. Klienten provar RFC 8414 först, och `issuer` måste vara identisk med den issuer som well-known-URL:en byggdes från ([asd26], [c-trbl]).
- `code_challenge_methods_supported: ["S256"]`. Utan fältet MÅSTE klienter avbryta ([sec26]). Claude skickar alltid S256 ([c-auth]).
- Alla AS-endpoints MÅSTE vara HTTPS. Redirect URIs MÅSTE vara `localhost` eller HTTPS ([sec26]).
- Exakt matchning av redirect URI mot registrerade värden ([sec26]). Undantag: loopback-URI:er matchas utan port enligt RFC 8252 §7.3, och Claude Code kräver samma sak för `localhost` ([c-auth], [c-lazy]).
- Refresh tokens MÅSTE roteras för publika klienter ([sec26]).
- Consent-sidan MÅSTE visa redirect-URI:ns hostname tydligt och SHOULD varna när bara loopback-URI:er finns ([sec26]).
- `/token` måste ta `application/x-www-form-urlencoded`. `/register` tar `application/json` ([c-auth]).
- Svarstider: discovery, registrering och token inom 10 s, refresh inom 30 s ([c-auth]).

**Bör**
- Korta access tokens ([sec26]).
- `iss` i authorization response (RFC 9207) och `authorization_response_iss_parameter_supported: true` ([a26]). Specen väntas göra detta till MUST ([a26]).
- Om `offline_access` finns i AS-metadata `scopes_supported` lägger Claude till den för att få refresh token ([c-auth]). MCP-servern SHOULD NOT lista `offline_access` i PRM eller i `WWW-Authenticate` ([a26]).

**Minimal AS-metadata som Claude accepterar för CIMD** (från Anthropics exempel, [c-lazy]):

```json
{
  "issuer": "https://maya.example.se",
  "authorization_endpoint": "https://maya.example.se/authorize",
  "token_endpoint": "https://maya.example.se/token",
  "response_types_supported": ["code"],
  "grant_types_supported": ["authorization_code", "refresh_token"],
  "token_endpoint_auth_methods_supported": ["none"],
  "code_challenge_methods_supported": ["S256"],
  "client_id_metadata_document_supported": true
}
```

## 4. DCR kontra Client ID Metadata Documents

| | CIMD | DCR (RFC 7591) |
|---|---|---|
| Spec-status | SHOULD ([a26]) | MAY, deprecated i `2026-07-28` ([chg]) |
| Klientprioritet i specen | 2:a, efter förregistrering | 3:e ([creg]) |
| Hur | `client_id` är en HTTPS-URL. AS:en hämtar JSON-dokumentet, kontrollerar att `client_id` matchar URL:en exakt och validerar `redirect_uri` mot `redirect_uris` ([creg]) | `POST /register` ger ett `client_id` |
| Lagring i Maya | Ingen klienttabell, bara cache enligt HTTP-cacheheaders (SHOULD) ([creg]) | Klienttabell som växer. Claude registrerar en ny klient vid varje ny anslutning ([c-auth]) |
| Risker | SSRF när AS:en hämtar godtyckliga URL:er (SHOULD hanteras). Localhost-imitation går inte att förhindra med CIMD ([sec26]) | Öppen registreringsendpoint |
| Claude | Kräver `client_id_metadata_document_supported: true` och `"none"` som auth method ([c-auth]) | Standardreserv ([c-auth]) |

Kända CIMD-dokument:
- Claude Code: `https://claude.ai/oauth/claude-code-client-metadata`. Det anger `redirect_uris` `http://localhost/callback` och `http://127.0.0.1/callback` och `token_endpoint_auth_method: "none"` (hämtat 2026-10-12, [cc-cimd]).
- Hostade Claude-appar (claude.ai, Desktop, mobil, Cowork) använder redirect `https://claude.ai/api/mcp/auth_callback` ([c-auth]). Anthropic säger att de hostar ett CIMD för detta ([c-auth]), men URL:en till det dokumentet står inte i källorna jag läste.

**Rekommendation:** Implementera CIMD. Det är den enklaste vägen, den som specen pekar framåt mot, och den som inte kräver en klientdatabas. Lägg eventuellt till DCR om andra MCP-klienter (äldre) behöver det. Eftersom Maya har en enda ägare kan AS:en köra en trust policy, alltså en allowlist över `client_id`-domäner (t.ex. `claude.ai`). Det är uttryckligen tillåtet (MAY) ([sec26], [a25]) och minskar SSRF-ytan.

## 5. PKCE, token-livslängd och refresh

- PKCE: klienten MÅSTE använda S256 ([sec26]). AS:en verifierar `code_verifier` mot `code_challenge` vid `/token`.
- RFC 8707 `resource` skickas i både authorization- och token-request, satt till MCP-serverns kanoniska URI. Klienter skickar den oavsett AS-stöd ([a26]). AS:en bör binda token-audience till den.
- Specen sätter ingen siffra på livslängd, bara "short-lived" (SHOULD) ([sec26]).
- Claude förnyar proaktivt upp till 5 min före lagrad utgång, och reaktivt vid `401`. Rotera refresh token och returnera den nya i samma svar som ogiltigförklarar den gamla. Använd `invalid_grant` för ogiltig refresh token ([c-auth]).
- Klienter får inte förutsätta att refresh tokens utfärdas ([a26]).
- **Slutsats:** Eftersom AS och RS är samma binär behövs inga JWT:er. Opaka, slumpade tokens som lagras hashade i SQLite räcker. RS-validering blir då en uppslagning som också kontrollerar audience och utgång. Det är vad rmcp:s exempel-AS gör, fast i minnet ([rmcp-ex]). Specen kräver inget visst tokenformat, bara audience-validering ([a26]).

## 6. Rust-alternativ

| Crate | Version (crates.io, 2026-10-12) | Vad den ger Maya |
|---|---|---|
| `rmcp` | 3.5.1, släppt 2026-10-05, officiell SDK ([crates-rmcp], [rmcp-repo]) | Streamable HTTP-server som tower-service (`transport-streamable-http-server`). Protokollversionerna `2025-03-26` till `2026-07-28` ([rmcp-model]). `legacy_session_mode` (default `true`) gäller bara äldre versioner ([rmcp-tower]). `allowed_hosts` är som standard bara loopback och måste sättas för publik drift ([rmcp-tower]). Featuren `auth` är OAuth-*klient* byggd på `oauth2` ([rmcp-oauth], [rmcp-cargo]). |
| rmcp-exempel | `simple_auth_streamhttp.rs`, `complex_auth_streamhttp.rs` (DCR), `cimd_auth_streamhttp.rs` (CIMD), axum ([rmcp-ex]) | Färdiga mallar för en egen AS: CIMD-URL-validering, hämtning av metadata och PRM på path-suffixad well-known. Tokens och koder ligger i minnet. |
| `oxide-auth` (+ `oxide-auth-axum`) | 0.6.1, senaste release 2024-06-02. Senaste commit 2026-01-31 ([crates-oxide], [oxide-repo]) | OAuth2-serverprimitiver med PKCE (S256) och refresh-rotation. I källkoden hittar jag inget stöd för RFC 8414-metadata, DCR, CIMD eller RFC 8707 (bara RFC-texten under `docs/`). |
| `oauth2` | 5.0.0 (2025-01) ([crates-oauth2]) | Bara klient. Inte relevant för AS:en. |
| `jsonwebtoken` | 11.1.0 (2026-09) ([crates-jwt]) | Behövs bara om vi väljer JWT-tokens. |

Ingen mogen Rust-crate implementerar hela MCP-profilen av OAuth 2.1 på serversidan (bedömning utifrån sökning på crates.io och genomläsning av källkoden ovan).

**Rekommendation:** Använd `rmcp` för MCP-delen. Skriv AS:en själv i axum med `cimd_auth_streamhttp.rs` som förlaga. Ytan är liten: två metadata-endpoints, `/authorize` med inloggning, consent och CIMD-hämtning, `/token` med `authorization_code` + PKCE och `refresh_token` med rotation, samt eventuellt `/register`. Det håller också single-binary-kravet.

## 7. Driftkonsekvenser för självhostning

- claude.ai och Desktop/mobil kräver att hostnamnet ger enbart globalt routbara IPv4-adresser. Privata adresser, CGNAT, loopback och IPv6-only avvisas. Dynamisk DNS mot hemnät fungerar ofta inte ([c-trbl]).
- WAF/CDN måste släppa in Anthropics egress `160.79.104.0/21` ([c-auth]).
- MCP tunnels (utgående anslutning, ingen öppen port) finns bara för Enterprise och i research preview ([c-tun]). Det är inget alternativ för Mayas målgrupp. En vanlig publik tunnel/reverse proxy med TLS (t.ex. cloudflared, ngrok) nämns av Anthropic som sätt att exponera en lokal server ([c-lazy]).
- Claude Code når `localhost` direkt ([c-lazy]). För utveckling räcker därför `http://localhost` för MCP-endpointen, men specen kräver HTTPS för AS-endpoints ([sec26]).

## Öppna frågor (kandidater till nya tickets)

1. **Hur exponeras en självhostad Maya publikt med TLS** (reverse proxy, egen domän, cloudflared, inbyggd ACME)? Det är ett krav för claude.ai men inte för Claude Code.
2. **Hur loggar ägaren in på AS:en?** `/authorize` behöver en användarautentisering, t.ex. lösenord eller passkey mot Mayas egen användartabell. Det knyter an till hur Leptos-UI:t autentiserar.
3. **Scope-modell för bokföring:** t.ex. `read` respektive `write`/`bokför`, med step-up via 403 `insufficient_scope`. Vilka verktyg kräver vad?
4. **Ska Maya stödja både `2025-11-25` och `2026-07-28`**, och i så fall med sessioner (`legacy_session_mode`) eller tillståndslöst? Behöver verifieras mot Claude i praktiken.
5. **CIMD trust policy:** allowlist (bara `claude.ai`) eller öppen? Därtill SSRF-skydd vid hämtning.
6. **Vilket CIMD använder de hostade Claude-apparna**, och fungerar flödet utan DCR från claude.ai? Verifieras med ett prototypanrop.

## Källor

[ver]: https://modelcontextprotocol.io/specification/versioning
[chg]: https://modelcontextprotocol.io/specification/2026-07-28/changelog
[a26]: https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization
[asd26]: https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization/authorization-server-discovery
[creg]: https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization/client-registration
[sec26]: https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization/security-considerations
[shttp26]: https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/streamable-http
[a25]: https://modelcontextprotocol.io/specification/2025-11-25/basic/authorization
[t25]: https://modelcontextprotocol.io/specification/2025-11-25/basic/transports
[c-idx]: https://claude.com/docs/connectors/building/index.md
[c-auth]: https://claude.com/docs/connectors/building/authentication.md
[c-lazy]: https://claude.com/docs/connectors/building/lazy-authentication.md
[c-trbl]: https://claude.com/docs/connectors/building/troubleshooting.md
[c-tun]: https://claude.com/docs/connectors/mcp-tunnels/overview.md
[cc-cimd]: https://claude.ai/oauth/claude-code-client-metadata
[rmcp-repo]: https://github.com/modelcontextprotocol/rust-sdk
[rmcp-model]: https://github.com/modelcontextprotocol/rust-sdk/blob/8763a3c3ffb259254d6a16ec1eb4ae9173d5d57d/crates/rmcp/src/model.rs#L167-L207
[rmcp-tower]: https://github.com/modelcontextprotocol/rust-sdk/blob/8763a3c3ffb259254d6a16ec1eb4ae9173d5d57d/crates/rmcp/src/transport/streamable_http_server/tower.rs#L78-L212
[rmcp-cargo]: https://github.com/modelcontextprotocol/rust-sdk/blob/8763a3c3ffb259254d6a16ec1eb4ae9173d5d57d/crates/rmcp/Cargo.toml
[rmcp-oauth]: https://github.com/modelcontextprotocol/rust-sdk/blob/8763a3c3ffb259254d6a16ec1eb4ae9173d5d57d/docs/OAUTH_SUPPORT.md
[rmcp-ex]: https://github.com/modelcontextprotocol/rust-sdk/tree/8763a3c3ffb259254d6a16ec1eb4ae9173d5d57d/examples/servers/src
[crates-rmcp]: https://crates.io/crates/rmcp
[crates-oxide]: https://crates.io/crates/oxide-auth
[oxide-repo]: https://github.com/HeroicKatora/oxide-auth
[crates-oauth2]: https://crates.io/crates/oauth2
[crates-jwt]: https://crates.io/crates/jsonwebtoken

- MCP Versioning: <https://modelcontextprotocol.io/specification/versioning>
- MCP 2026-07-28 changelog: <https://modelcontextprotocol.io/specification/2026-07-28/changelog>
- MCP 2026-07-28 Authorization, inklusive underavsnitten Authorization Server Discovery, Client Registration och Security Considerations: <https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization>
- MCP 2026-07-28 Streamable HTTP: <https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/streamable-http>
- MCP 2025-11-25 Authorization: <https://modelcontextprotocol.io/specification/2025-11-25/basic/authorization>
- MCP 2025-11-25 Transports: <https://modelcontextprotocol.io/specification/2025-11-25/basic/transports>
- Claude, Build an MCP server: <https://claude.com/docs/connectors/building/index.md>
- Claude, Authentication for connectors: <https://claude.com/docs/connectors/building/authentication.md>
- Claude, Lazy authentication: <https://claude.com/docs/connectors/building/lazy-authentication.md>
- Claude, Troubleshoot your connector: <https://claude.com/docs/connectors/building/troubleshooting.md>
- Claude, MCP tunnels: <https://claude.com/docs/connectors/mcp-tunnels/overview.md>
- Claude Codes CIMD: <https://claude.ai/oauth/claude-code-client-metadata>
- rmcp (modelcontextprotocol/rust-sdk) på commit `8763a3c`: <https://github.com/modelcontextprotocol/rust-sdk>
- crates.io-API för rmcp, oxide-auth, oauth2, openidconnect och jsonwebtoken (hämtat 2026-10-12)
- oxide-auth: <https://github.com/HeroicKatora/oxide-auth>
- Refererade RFC:er (via MCP-specen): RFC 6750, 7591, 7636, 8252, 8414, 8707, 9207, 9728, draft-ietf-oauth-v2-1-13, draft-ietf-oauth-client-id-metadata-document-00
