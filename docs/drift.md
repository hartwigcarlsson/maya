# Drift av Maya

Maya levereras som en färdig image på GHCR (`ghcr.io/hartwigcarlsson/maya`) för amd64 och arm64, med semver-taggar. Beslutet finns i [Hur driftsätts Maya?](https://github.com/hartwigcarlsson/maya/issues/70) och [publik HTTPS](https://github.com/hartwigcarlsson/maya/issues/66).

## Kom igång

1. Kopiera `compose.yaml`, `deploy/` och `.env.example` till servern och döp om `.env.example` till `.env`.
2. Fyll i `MAYA_DOMAIN`, `MAYA_PUBLIC_URL` och backupmålet. Domänen väljs före första start, eftersom passkeys binds till den.
3. Skapa backupkatalogen på en annan disk än Dockers volymer och ge den till Mayas användare (uid 65532):

   ```sh
   mkdir -p /mnt/backupdisk/maya && sudo chown 65532:65532 /mnt/backupdisk/maya
   ```

   och sätt `MAYA_BACKUP_HOST_DIR=/mnt/backupdisk/maya` i `.env`.
4. Starta: `docker compose up -d`. Maya skriver ut sin tolkade konfiguration, utan hemligheter, i `docker compose logs maya`.

5. Skapa den första användaren: `docker compose exec maya maya user create <e-post>`. Kommandot skriver ut en länk som gäller i 24 timmar och kan användas en gång. Öppna den och registrera en passkey. Fler användare bjuds in i webben.

Maya kräver en publik IPv4 och port 443. Kontrollera att `https://<domän>/healthz` svarar `ok`.

Vid första start sparar Maya `rp_id` och origin ur `MAYA_PUBLIC_URL`. Passkeys är bundna till dem, så Maya vägrar sedan starta om den publika URL:en ger en annan domän eller port.

## Uppgradera

Byt taggen i `compose.yaml` och kör `docker compose up -d`. Maya uppdateras aldrig automatiskt, och taggen är alltid en exakt version, aldrig `latest`. Migreringarna körs i ordning vid start, så det går att hoppa över versioner. Maya vägrar starta mot en databas med nyare schema än binären; nedgradering görs bara med `maya restore`.

## Konfiguration

All konfiguration läses ur miljön och `.env`. Se `.env.example` för alla variabler.

| Variabel | |
|---|---|
| `MAYA_PUBLIC_URL` | Krävs. Mayas publika adress, ett https-origin utan sökväg. Härleds aldrig ur `Host` eller `X-Forwarded-*`, och en begäran med annan `Host` avvisas. |
| `MAYA_BACKUP_DIR` eller `MAYA_BACKUP_S3_…` | Krävs. Backupmålet: en katalog (i containern `/backup`) eller S3-kompatibel lagring. |
| `MAYA_TRUSTED_PROXY` | Adresser eller nätblock vars `X-Forwarded-For` godtas. Sätts av `compose.yaml`. |
| `MAYA_LISTEN`, `MAYA_DATA_DIR` | `0.0.0.0:8080` och `/data` i imagen. |
| `MAYA_BACKUP_TIME` | Den nattliga backupen, som standard 03:00 i `Europe/Stockholm`. |
| `MAYA_SMTP_…`, `MAYA_DRIFT_EPOST` | Valfria, för larm till driften. |
| `MAYA_ARKIVPLAN_…` | Arkivplanens fritext: plats och land, offsite, nyckel, ansvarig. |

En hemlighet (`MAYA_SMTP_PASSWORD`, `MAYA_BACKUP_S3_SECRET_ACCESS_KEY`) kan i stället anges som en fil med `_FILE`-varianten, till exempel en Docker secret: `MAYA_SMTP_PASSWORD_FILE=/run/secrets/smtp`.

Klockan är alltid `Europe/Stockholm`, oavsett värdens tidszon. Tidszonsdatabasen är inbyggd i binären.

## Proxy

Proxyn ska skicka hela origin oförändrad (`Host` som klienten skickade), inte buffra svar (server-sent events) och inte omdirigera till en annan värd. Maya godtar `X-Forwarded-For` bara från adresserna i `MAYA_TRUSTED_PROXY`.

### Caddy (standard)

`COMPOSE_PROFILES=caddy` i `.env`. Caddy skaffar certifikatet själv. Konfigurationen är `deploy/Caddyfile`.

### Traefik

`COMPOSE_PROFILES=traefik` i `.env`. Traefik körs med file provider (`deploy/traefik-dynamic.yml`), alltså utan Docker-socket, och skaffar certifikatet med ACME via TLS-ALPN-01. Ange `MAYA_ACME_EPOST`.

### En Traefik som redan finns

`compose.traefik-extern.yaml` är ett exempel med labels på ett externt nätverk. Sätt `MAYA_TRUSTED_PROXY` i `.env` till Traefiks adress eller nätverkets nätblock.

### Tailscale Funnel

> **Varning.** Funnel är bara ett alternativ för den som sitter bakom CGNAT och inte kan öppna port 443. Det finns ingen Compose-profil för det. Funnel-namnet har publika `A`- och `AAAA`-poster mot Tailscales reläer bara medan Funnel är på, och två öppna buggar hos Tailscale beskriver tillfälliga NXDOMAIN för `A` (cirka 9 % av uppslagningarna) och tappade IPv6-anslutningar. Passkeys binds till domänen, så den kan inte bytas senare utan `maya domain change`.

Med Funnel blir `MAYA_PUBLIC_URL` maskinens Funnel-namn, till exempel `https://maya.tailnet-namn.ts.net`, och Funnel pekar på `http://127.0.0.1:8080`:

```sh
tailscale funnel --bg 8080
```

Publicera då port 8080 på `127.0.0.1` (`ports: ["127.0.0.1:8080:8080"]` på tjänsten `maya`) och sätt `COMPOSE_PROFILES=` tomt, så att varken Caddy eller Traefik startar. Anslutningarna kommer då från nätverkets gateway, så sätt `MAYA_TRUSTED_PROXY=172.31.80.1`.

## Containern

Maya kör som icke-root (uid 65532) i en distroless-image, lyssnar på port 8080, loggar till stdout och har `/healthz` för Dockers hälsokontroll (`maya healthcheck`). Databasen ligger i volymen `/data`.
