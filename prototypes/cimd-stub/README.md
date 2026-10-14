# PROTOTYP: cimd-stub

Slängkod för ticketet [Kan Claude ansluta till Maya via CIMD utan DCR?](https://github.com/hartwigcarlsson/maya/issues/22). Mergas aldrig.

En MCP-server (rmcp 3.5.1, streamable HTTP) bakom en minimal OAuth 2.1-server i axum. Det finns ingen `/register`, så klienten måste använda Client ID Metadata Documents. Allt ligger i minnet, och allt som klienten skickar loggas.

```sh
cargo run                                    # http://localhost:8725, tillståndslöst, access token 120 s
SESSIONS=1 cargo run                         # rmcp:s legacy_session_mode
BASE_URL=https://<publik-värd> cargo run     # bakom tunnel/proxy, för claude.ai
```

Claude Code: `claude mcp add --transport http maya-stub http://localhost:8725/mcp`, sedan `claude mcp login maya-stub` i en riktig terminal. Verktyget `vem_ar_jag` visar klient, scope, protokollversion och session.

## Resultat 2026-10-14 (Claude Code 2.1.299)

- Claude Code valde CIMD direkt och försökte aldrig `/register`. `client_id` = `https://claude.ai/oauth/claude-code-client-metadata`, redirect `http://localhost:<port>/callback`. Dokumentet skickas med `Cache-Control: public, max-age=300`.
- Authorize-begäran: `scope=maya offline_access` (offline_access lades till eftersom AS-metadata listar den), `prompt=consent`, `resource=http://localhost:8725/mcp`, PKCE S256.
- Token-begäran: form-urlencoded, ingen klientautentisering, `resource` med i både code- och refresh-anropet.
- Claude Code godtog en AS på `http://localhost`.
- Med 120 s access token förnyade Claude Code direkt efter utbytet (proaktiv förnyelse inom 5 min). Rotationen fungerade.
- Första anropet var `server/discover` med `2026-07-28`, utan token, och fick 401. Verktygsanropet gick tillståndslöst med `MCP-Protocol-Version: 2026-07-28` och utan `Mcp-Session-Id`.
- claude.ai testades inte: det kräver en publik HTTPS-värd.
