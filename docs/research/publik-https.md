# Publik HTTPS för en self-hostad Maya

Forskningsanteckning för [Hur nås en self-hostad tjänst över HTTPS från internet?](https://github.com/hartwigcarlsson/maya/issues/66): hur en self-hostad Maya (en Rust/axum-binär i Docker Compose som serverar UI, MCP på `/mcp` och en egen OAuth 2.1 AS på samma origin) nås över HTTPS från internet, som claude.ai och passkeys kräver. Jämförs: egen domän med reverse proxy (Caddy, Traefik), tunnlar (Cloudflare Tunnel, Tailscale Funnel, ngrok) och inbyggd ACME i binären (`rustls-acme`).

Researchad 2026-10-14. Källor: Claudes connector-dokumentation, Anthropics IP-adresslista, MCP-specen `2026-07-28`, Let's Encrypts dokumentation, Caddys, Traefiks, Cloudflares, Tailscales och ngroks egna dokumentation, källkoden till Tailscale (`ipn/ipnlocal/serve.go`), `rustls-acme` 0.15.4, `rmcp` 3.5.1 och `axum-extra`, samt Public Suffix List.

Förkortningar för källor:

- **[c-trbl]**: [Claude: Troubleshoot your connector](https://claude.com/docs/connectors/building/troubleshooting.md)
- **[c-test]**: [Claude: Test your connector](https://claude.com/docs/connectors/building/testing.md)
- **[ip]**: [Anthropic: IP addresses](https://platform.claude.com/docs/en/api/ip-addresses)
- **[shttp]**: [MCP 2026-07-28: Streamable HTTP](https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/streamable-http)
- **[le]**: [Let's Encrypt: Challenge types](https://letsencrypt.org/docs/challenge-types/)
- **[caddy-https]**: [Caddy: Automatic HTTPS](https://caddyserver.com/docs/automatic-https)
- **[caddy-rp]**: [Caddy: reverse_proxy](https://caddyserver.com/docs/caddyfile/directives/reverse_proxy)
- **[caddy-img]**: [Docker Hub: caddy](https://hub.docker.com/_/caddy)
- **[traefik-docker]**: [Traefik: Docker setup](https://doc.traefik.io/traefik/setup/docker) och [Expose Docker, advanced](https://doc.traefik.io/traefik/expose/docker/advanced)
- **[traefik-ep]**: [Traefik: EntryPoints](https://doc.traefik.io/traefik/reference/install-configuration/entrypoints)
- **[traefik-svc]**: [Traefik: Service, `responseForwarding`](https://doc.traefik.io/traefik-hub/api-gateway/reference/routing/kubernetes/http/services/ref-service)
- **[cft]**: [Cloudflare Tunnel](https://developers.cloudflare.com/cloudflare-one/networks/connectors/cloudflare-tunnel/)
- **[cft-create]**: [Cloudflare: Create a tunnel (dashboard)](https://developers.cloudflare.com/cloudflare-one/networks/connectors/cloudflare-tunnel/get-started/create-remote-tunnel/)
- **[cft-dns]**: [Cloudflare: Tunnel DNS records](https://developers.cloudflare.com/cloudflare-one/networks/connectors/cloudflare-tunnel/routing-to-tunnel/dns/)
- **[cft-origin]**: [Cloudflare: Origin parameters](https://developers.cloudflare.com/cloudflare-one/networks/connectors/cloudflare-tunnel/configure-tunnels/cloudflared-parameters/origin-parameters/)
- **[cft-quick]**: [Cloudflare: Quick Tunnels](https://developers.cloudflare.com/cloudflare-one/networks/connectors/cloudflare-tunnel/do-more-with-tunnels/trycloudflare/)
- **[cf-524]**: [Cloudflare: Error 524](https://developers.cloudflare.com/support/troubleshooting/http-status-codes/cloudflare-5xx-errors/error-524/)
- **[cf-hdr]**: [Cloudflare: HTTP headers](https://developers.cloudflare.com/fundamentals/reference/http-headers/)
- **[cf-zone]**: [Cloudflare: Zone setups](https://developers.cloudflare.com/dns/zone-setups/)
- **[ts-funnel]**: [Tailscale Funnel](https://tailscale.com/kb/1223/funnel)
- **[ts-docker]**: [Tailscale: Docker parameters](https://tailscale.com/docs/features/containers/docker/docker-params)
- **[ts-serve]**: [`ipn/ipnlocal/serve.go` @ `3d62394`](https://github.com/tailscale/tailscale/blob/3d62394afc62273e4d6a3e9ffaa82987179fad2d/ipn/ipnlocal/serve.go#L1088-L1112)
- **[ngrok]**: [ngrok: Free plan limits](https://ngrok.com/docs/pricing-limits/free-plan-limits/)
- **[acme-lib]**: [`rustls-acme` `src/lib.rs` @ `b74626e`](https://github.com/FlorianUekermann/rustls-acme/blob/b74626e54e16019a3056a7421caf1ebb68553b2f/src/lib.rs)
- **[acme-ex]**: [`rustls-acme` `examples/low_level_axum.rs` @ `b74626e`](https://github.com/FlorianUekermann/rustls-acme/blob/b74626e54e16019a3056a7421caf1ebb68553b2f/examples/low_level_axum.rs)
- **[rmcp-tower]**: [`rmcp` 3.5.1 `streamable_http_server/tower.rs`](https://github.com/modelcontextprotocol/rust-sdk/blob/rmcp-v3.5.1/crates/rmcp/src/transport/streamable_http_server/tower.rs)
- **[axum-extra]**: [`axum-extra` CHANGELOG](https://github.com/tokio-rs/axum/blob/main/axum-extra/CHANGELOG.md)
- **[psl]**: [Public Suffix List](https://publicsuffix.org/list/public_suffix_list.dat)

## Sammanfattning

- **Det hårda kravet kommer från claude.ai.** Hostnamnet måste ha en `A`-post i publik DNS, och *alla* adresser det ger måste vara globalt routbara. Privata adresser, CGNAT (`100.64.0.0/10`), loopback och IPv6-only avvisas innan något anrop görs. Split-horizon-DNS och dynamisk DNS mot ett hemnät bakom NAT fungerar ofta inte ([c-trbl]). Anthropic föreslår själva en publik reverse proxy, en molnvärd eller en tunnel, och nämner Cloudflare Tunnel och ngrok ([c-trbl], [c-test]).
- **Fyra vägar fungerar, med olika förutsättningar.**
  1. *Egen domän + Caddy i Compose*: kräver en publik IPv4 där portarna 443 (och helst 80) når maskinen. Enklast: automatiska certifikat med HTTP-01 och TLS-ALPN-01, SSE flushas direkt och klientens `X-Forwarded-*` ignoreras som standard ([caddy-https], [caddy-rp]). Traefik fungerar likadant men är mer att konfigurera och vill ha Docker-socketen ([traefik-docker]).
  2. *Cloudflare Tunnel (`cloudflared` i Compose)*: fungerar bakom CGNAT och utan öppna portar ([cft]), men domänen måste ha Cloudflare som DNS (full setup på gratisplanen, [cf-zone]), och Cloudflare avslutar TLS och ser all trafik i klartext. Ett svar som är tyst i 125 s avbryts ([cf-524]).
  3. *Tailscale Funnel*: fungerar bakom CGNAT och TLS avslutas på den egna noden, så reläet ser inget i klartext ([ts-funnel]). Men bara namn under `<tailnet>.ts.net`, alltså ingen egen domän, bandbredden är begränsad och funktionen är i beta ([ts-funnel]).
  4. *Inbyggd ACME med `rustls-acme`*: Maya avslutar själv TLS och hämtar certifikat med TLS-ALPN-01 på port 443 ([acme-lib]). Kräver samma publika IPv4 och port 443 som Caddy, men utan extra container. Fungerar inte bakom en tunnel eller en TLS-avslutande proxy.
- **ngrok och Cloudflare Quick Tunnels duger inte för drift.** Quick Tunnels byter hostname vid varje start och stöder inte SSE ([cft-quick]). ngroks gratisplan har ingen egen domän, 1 GB och 20 000 anrop i månaden och en varningssida framför all HTML-trafik från webbläsare ([ngrok]).
- **Maya ska känna sin publika URL från konfiguration, aldrig från anropet.** En enda inställning (t.ex. `https://maya.example.se`, utan sökväg) ger `rp_id` och origin för passkeys, OAuth-`issuer`, PRM-`resource` (`<URL>/mcp`), `allowed_hosts` och `allowed_origins` för rmcp och `Secure`-cookies. `axum-extra` har dessutom tagit bort sina `Host`- och `Scheme`-extraktorer ([axum-extra]).
- **rmcp 3.5.1 släpper som standard bara in `Host: localhost`, `127.0.0.1` och `::1`** ([rmcp-tower] L203). Bakom alla fyra vägar kommer anropet med det publika hostnamnet, så Maya måste sätta `allowed_hosts` till den publika värden, annars får varje MCP-anrop `403`.
- **Proxyn eller tunneln ska skicka hela origin oförändrad**: inga sökvägsomskrivningar, inga omdirigeringar till en annan värd (claude.ai tappar då `Authorization`, [c-trbl]), ingen inloggning eller WAF-regel framför `/mcp`, `/.well-known/*`, `/authorize` och `/token`, och ingen buffring av `text/event-stream`. Se avsnitt 6.
- **Rekommendation:** Maya talar vanlig HTTP på en port och låter TLS ligga utanför binären. Standard är egen domän + Caddy i Compose. Cloudflare Tunnel är alternativet när det saknas publik IPv4. Funnel går om användaren accepterar ett `ts.net`-namn. Inbyggd ACME behövs inte i v1. Valet av domän måste göras före första start, eftersom `rp_id` låses då ([inloggning](https://github.com/hartwigcarlsson/maya/issues/15)).

## 1. Vad claude.ai kräver av nätet

claude.ai-connectors körs i Anthropics infrastruktur och når servern över publikt internet. Innan något HTTP-anrop görs slås hostnamnet upp och resultatet valideras. Anslutningen avvisas om hostnamnet ([c-trbl]):

- ger en privat adress i `10.0.0.0/8`, `172.16.0.0/12` eller `192.168.0.0/16`,
- ger en CGNAT-adress i `100.64.0.0/10`,
- ger en loopback- eller link-local-adress,
- ger en blandning av publika och icke-publika adresser,
- saknar `A`-post i publik DNS. Connectors är bara IPv4, så ett namn med bara `AAAA` når inte fram.

Övrigt som påverkar driften ([c-trbl], [c-test], [ip]):

- **Omdirigeringar:** om MCP-URL:en svarar `301`/`302`/`307`/`308` till en annan värd (t.ex. apex till `www.`) tappar Claude `Authorization` och anslutningen misslyckas med "Authorization with the MCP server failed". Registrera den URL servern faktiskt svarar på.
- **WAF, CDN och bot-skydd:** `403` eller `429` som applikationen inte själv skickade tyder på att något framför servern blockerar Anthropic. Släpp in Anthropics utgående IPv4-intervall `160.79.104.0/21` ([ip]) eller undanta MCP- och OAuth-sökvägarna.
- **Discovery:** en proxy får inte döpa om eller ta bort `WWW-Authenticate`, eller svara `403` på `/.well-known/`-sökvägarna.
- **Svarstider:** Claude väntar högst 10 s på `/token`, inklusive proxyn framför.
- **`Origin`-validering:** en för strikt kontroll kan avvisa Anthropics anrop ([c-test]).

Claude Code ansluter däremot från användarens dator och når `localhost` ([c-test]). Utveckling kräver alltså ingen publik värd.

## 2. Egen domän och reverse proxy i Docker Compose

Gemensamt för Caddy och Traefik: maskinen behöver en publik IPv4 där 443 (och för HTTP-01 port 80) är öppna, direkt eller via portforward i routern. Står Maya hemma bakom CGNAT, som är vanligt hos fiber- och mobiloperatörer, fungerar det inte, och då återstår en VPS eller en tunnel. Med dynamisk publik IP måste `A`-posten uppdateras när adressen byts. Det fungerar så länge adressen verkligen är publik, men Anthropic varnar för att dynamisk DNS ofta pekar på NAT ([c-trbl]).

ACME-utmaningarna ([le]):

| Utmaning | Port | Kommentar |
|---|---|---|
| HTTP-01 | bara 80 | Fungerar inte om operatören blockerar port 80. |
| TLS-ALPN-01 | bara 443 | Helt i TLS-lagret, inget port 80-behov. |
| DNS-01 | ingen | Kräver API-nycklar till DNS-leverantören på servern. Ger wildcard. |

### Caddy

- Automatiska publika certifikat kräver att `A`-/`AAAA`-posten pekar på servern, att 80 och 443 nås utifrån, att domänen står i konfigurationen och att datakatalogen är skrivbar och beständig. HTTP-01 och TLS-ALPN-01 är på som standard. Utfärdare är Let's Encrypt med ZeroSSL som reserv. HTTP omdirigeras automatiskt till HTTPS ([caddy-https]).
- I den officiella imagen ligger certifikat och nycklar i `/data`, som måste vara en volym och inte får behandlas som cache ([caddy-img]).
- `reverse_proxy` skickar vidare inkommande headers, inklusive `Host`, oförändrade (utom hop-by-hop). `X-Forwarded-For`, `X-Forwarded-Proto` och `X-Forwarded-Host` sätts av Caddy, och klientens egna värden ignoreras om de inte kommer från `trusted_proxies` ([caddy-rp]).
- Svar med `Content-Type: text/event-stream` flushas direkt. Utan konfiguration finns ingen timeout på svarshuvuden eller läsning mot backend ([caddy-rp]).

En minimal konfiguration blir därför bara `maya.example.se { reverse_proxy maya:8080 }`. Den uppfyller avsnitt 6 utan tillägg.

### Traefik

- Certifikat via `certificatesresolvers.<namn>.acme` med `email`, `storage` (en fil i en volym) och vald utmaning, plus omdirigering från `web` till `websecure` ([traefik-docker]).
- Konfigurationen görs med labels på containrarna och Docker-providern, som i dokumentationens exempel monterar `/var/run/docker.sock`. Det ger Traefik i praktiken root över värden ([traefik-docker]).
- `X-Forwarded-*` från klienten litas bara på från `forwardedHeaders.trustedIPs` ([traefik-ep]). Strömmande svar flushas direkt, annars var 100 ms ([traefik-svc]).

Traefik är byggt för många tjänster som upptäcks dynamiskt. För en enda tjänst ger Caddy samma resultat med en rad konfiguration och utan Docker-socketen.

## 3. Tunnlar

En tunnel öppnar en utgående anslutning från maskinen till leverantörens kant, och kanten har den publika IPv4-adressen. Det löser CGNAT och kräver inga öppna portar, men leverantören blir en del av vägen.

### Cloudflare Tunnel

- `cloudflared` öppnar bara utgående anslutningar, så all inkommande trafik till ursprunget kan blockeras. Ingen publik IP behövs. En tunnel kan ha flera connectors ([cft]).
- En domän måste vara tillagd i Cloudflare innan en applikation publiceras ([cft-create]). Gratis- och Pro-planerna stöder bara full setup, där Cloudflare är primär DNS. Partial (CNAME) setup kräver Business eller Enterprise ([cf-zone]). Hostnamnet blir en CNAME till `<UUID>.cfargotunnel.com` i samma konto. Om tunneln är nere får besökare fel `1016` ([cft-dns]).
- Ursprunget anges som en tjänst-URL, t.ex. `http://localhost:8000` ([cft-create]), i Compose `http://maya:8080`. Inställningen `httpHostHeader` är tom som standard ([cft-origin]). Dokumentationen säger inte uttryckligen att det publika `Host` behålls, men det följer av att inget skrivs över. Det bör verifieras mot rmcp:s `allowed_hosts`.
- Cloudflare sätter `CF-Connecting-IP`, lägger till i `X-Forwarded-For` och skriver över `X-Forwarded-Proto` med klientens protokoll ([cf-hdr]).
- **TLS avslutas hos Cloudflare.** Bokföringsdata, sessionscookies, OAuth-koder och access tokens passerar Cloudflare i klartext. Det är överföring, inte förvaring, så BFL:s förvaringskrav (A6 i `docs/bfl-kontroll.md`) berörs inte, men Cloudflare blir ett personuppgiftsbiträde.
- **Tidsgräns:** Proxy Read Timeout är 125 s och kan bara höjas på Enterprise ([cf-524]). En SSE-ström måste skicka något oftare än så. rmcp 3.5.1 skickar keep-alive var 15 s som standard ([rmcp-tower] L198), så det räcker, men ett långsamt svar utan SSE får inte ta över 125 s.
- **Bot-skydd och WAF** i Cloudflare kan blockera Anthropics anrop. Släpp då in `160.79.104.0/21` ([c-trbl], [ip]).
- Quick Tunnels (`trycloudflare.com`) är bara för test: hostnamnet byts vid varje start, max 200 samtidiga anrop och inget SSE ([cft-quick]).

### Tailscale Funnel

- Kräver Tailscale v1.38.3+, MagicDNS, HTTPS-certifikat för tailnetet och nodattributet `funnel` i policyfilen. Finns på alla planer men är i beta ([ts-funnel]).
- Lyssnar bara på 443, 8443 och 10000, och bara på namn under `<tailnet>.ts.net`. Egen domän nämns inte och stöds alltså inte ([ts-funnel]).
- Publik DNS svarar med en relä-servers adress. Reläet gör en TCP-proxy till noden och kan inte dekryptera trafiken. Tailscale-klienten på noden avslutar TLS och skickar anropet vidare till den lokala tjänsten ([ts-funnel]). Reläets adresser listas inte, så att namnet har en publik `A`-post måste kontrolleras med `dig` från ett externt nät.
- Bandbredden har gränser som inte går att konfigurera ([ts-funnel]).
- I Docker konfigureras Serve och Funnel med `TS_SERVE_CONFIG` (JSON), `TS_AUTHKEY` och en beständig `TS_STATE_DIR`, annars blir containern en ny nod vid varje omstart ([ts-docker]).
- Proxyn behåller `Host`, sätter `X-Forwarded-Host`, `X-Forwarded-Proto: https` och `X-Forwarded-For`, tar bort klientens `Tailscale-User-*` och sätter `Tailscale-Funnel-Request: ?1` på Funnel-trafik ([ts-serve]).
- `ts.net` finns på Public Suffix List ([psl]), så `maya.<tailnet>.ts.net` duger som WebAuthn-`rp_id` och cookies delas inte med andra tailnets. Men `rp_id` binds då till tailnetets namn: byter man senare till egen domän töms alla passkeys ([inloggning](https://github.com/hartwigcarlsson/maya/issues/15)).

### ngrok

Gratisplanen har en tilldelad domän (`*.ngrok-free.app`), ingen egen domän, 1 GB och 20 000 HTTP-anrop i månaden, och en varningssida framför all HTML-trafik från webbläsare ([ngrok]). Varningssidan stör inloggning och samtycke i webbläsaren. Betalplanen tar bort begränsningarna men ger inget som Cloudflare Tunnel inte redan ger.

## 4. Inbyggd ACME i binären (`rustls-acme`)

- `rustls-acme` 0.15.4 (crates.io, senast uppdaterad 2026-08-11) är en ACME-klient för rustls som validerar med TLS-ALPN-01 eller HTTP-01, så att utmaningssvar och vanlig TLS-trafik delar port. TLS-ALPN-01 är standard och rekommenderas. HTTP-01 finns för när en TLS-avslutande proxy står framför ([acme-lib]).
- Förnyelse sker i den ström som applikationen pollar, inte i bakgrundsuppgifter ([acme-lib]). Med axum: `AcmeConfig::new(domains).contact(..).cache_option(DirCache).directory_lets_encrypt(prod).state()`, sedan `state.axum_acceptor(state.default_rustls_config())` och `axum_server::bind(addr).acceptor(acceptor)`, med en uppgift som pollar `state.next()` för händelser ([acme-ex]).
- Exemplen använder Let's Encrypts staging som standard, eftersom produktionens hastighetsgränser är lätta att slå i under test ([acme-lib]). Certifikat och kontonyckel måste ligga i en beständig cachekatalog (en volym).

Konsekvenser för Maya:

- Samma nätkrav som Caddy: publik IPv4 och port 443 måste nå Maya (i Compose t.ex. `443:8443`). Ingen hjälp bakom CGNAT.
- Fungerar inte bakom en tunnel eller en TLS-avslutande proxy, eftersom TLS-ALPN-01 kräver att Maya själv tar emot TLS på 443 från Let's Encrypt. Maya måste alltså ändå kunna köra vanlig HTTP för tunnelvarianterna, och ACME blir en andra kodväg.
- Maya måste själv göra det Caddy ger gratis: omdirigering från HTTP, HSTS, OCSP/certifikatfel i loggen och larm om förnyelse misslyckas.
- Fördelen är en container mindre och att ingen annan process ser klartexten.

## 5. Hur Maya känner sin publika URL

Allt som claude.ai och webbläsaren jämför mot måste bygga på samma URL:

| Värde | Härleds ur publik URL `https://maya.example.se` |
|---|---|
| WebAuthn `rp_id` och origin | `maya.example.se`, `https://maya.example.se` |
| OAuth `issuer` (RFC 8414) | `https://maya.example.se` |
| PRM `resource` och token-audience (RFC 9728/8707) | `https://maya.example.se/mcp` |
| `WWW-Authenticate: Bearer resource_metadata=…` | `https://maya.example.se/.well-known/oauth-protected-resource/mcp` |
| rmcp `allowed_hosts` | `maya.example.se` (default är bara localhost, [rmcp-tower] L101–110, L203) |
| rmcp `allowed_origins` | `https://maya.example.se` (tom lista stänger av kontrollen, [rmcp-tower] L111–131) |
| Cookies | `Secure`, utan `Domain` |

Regler som följer:

- **Konfigurera URL:en uttryckligen** (miljövariabel eller fil). Härled den inte ur `Host`, `X-Forwarded-Host` eller `X-Forwarded-Proto`. Bakom en proxy ser Maya vanlig HTTP och kan inte veta att klienten använde HTTPS, och headers från klienten går att förfalska. `axum-extra` har tagit bort sina `Host`- och `Scheme`-extraktorer ([axum-extra]).
- **Lägg Maya i roten av origin, utan sökvägsprefix.** Claude och RFC 9728 letar efter `/.well-known/`-dokument i origin-roten med MCP-sökvägen tillagd ([c-trbl]). Ett prefix gör discovery och proxykonfigurationen krångligare.
- **Avvisa fel `Host` på alla rutter**, inte bara `/mcp`. MCP-specen kräver `Origin`-validering mot DNS rebinding ([shttp]), och samma skydd behövs för UI och OAuth.
- **Klientens IP** (för loggar och hastighetsbegränsning) tas ur `X-Forwarded-For` bara när anropet kommer från en betrodd proxy, t.ex. Compose-nätet. Annars används socketens adress.
- **Vid start** kan Maya kontrollera att URL:en börjar med `https://` (utom `localhost` i utveckling) och att `rp_id` stämmer med det sparade värdet, vilket redan är beslutat ([inloggning](https://github.com/hartwigcarlsson/maya/issues/15)).

## 6. Vad som måste gå igenom proxyn eller tunneln

**Sökvägar:** hela origin, oförändrad. Minst:

- `/mcp`: POST. GET och DELETE ska nå Maya, som svarar `405` eftersom Maya kör tillståndslöst ([shttp]).
- `/.well-known/oauth-protected-resource/mcp` och `/.well-known/oauth-authorization-server` ([c-trbl]).
- `/authorize`, `/token` och övriga AS-endpoints, inloggning och passkey-ceremonier samt resten av UI:t.

**Headers in mot Maya:** `Host` (det publika namnet), `Authorization`, `Origin`, `Cookie`, `Accept` (`application/json, text/event-stream`), `Content-Type`, `MCP-Protocol-Version`, `Mcp-Method`, `Mcp-Name` och `Mcp-Param-*`, som mellanled måste skicka vidare även om de inte känner igen dem ([shttp]). `X-Forwarded-For` får proxyn sätta.

**Headers ut från Maya:** `WWW-Authenticate` oförändrad ([c-trbl]), `Set-Cookie`, `Cache-Control`/`ETag`, `Content-Type: text/event-stream`. Maya bör sätta `X-Accel-Buffering: no` på SSE-svar ([shttp]).

**Beteende:**

- Ingen buffring av SSE. Caddy, Traefik och Tailscale flushar strömmande svar direkt ([caddy-rp], [traefik-svc], Go:s `httputil.ReverseProxy` i [ts-serve]).
- Inga omdirigeringar till en annan värd, ingen `www`-kanonisering ([c-trbl]).
- Ingen autentisering framför Maya (Cloudflare Access, basic auth), eftersom claude.ai måste kunna få `401` från Maya själv ([c-trbl]).
- Ingen cache av dynamiska svar.
- Total svarstid på `/token` under 10 s ([c-trbl]). Strömmar får inte vara tysta längre än närmaste tidsgräns, 125 s hos Cloudflare ([cf-524]).

## 7. Jämförelse

| | Caddy + egen domän | Traefik + egen domän | Cloudflare Tunnel | Tailscale Funnel | `rustls-acme` i Maya |
|---|---|---|---|---|---|
| Publik IPv4 hos användaren | krävs | krävs | behövs inte | behövs inte | krävs |
| Öppna portar | 443 (+80) | 443 (+80) | inga | inga | 443 |
| Egen domän | ja | ja | ja, med Cloudflare-DNS | nej, `*.ts.net` | ja |
| Vem ser klartext | bara värden | bara värden | Cloudflare | bara noden | bara Maya |
| Extra container | Caddy | Traefik + Docker-socket | `cloudflared` | `tailscale` | ingen |
| Tidsgränser | inga som standard | inga som standard | 125 s tystnad | bandbreddsgräns | inga |
| Tredjepartsberoende | Let's Encrypt | Let's Encrypt | Cloudflare-konto | Tailscale-konto, beta | Let's Encrypt |
| Kod i Maya | ingen | ingen | ingen | ingen | ACME, TLS, redirect, HSTS |

## Öppna frågor (för driftsättningen)

1. Vilka vägar dokumenteras och stöds: bara Caddy, eller Caddy plus Cloudflare Tunnel som exempel i Compose?
2. Kontrollera i en prototyp att `cloudflared` behåller det publika `Host` mot Maya, och att Funnels namn har en publik `A`-post.
3. Ska Maya vid start eller i en CLI-diagnos själv kontrollera nåbarheten (publik `A`-post, ingen omdirigering, discovery-dokumenten), som checklistan i [c-trbl]?
4. Hur väljer användaren publik URL före första start, så att `rp_id` inte behöver bytas senare?
