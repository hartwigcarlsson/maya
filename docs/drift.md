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

## Användare och domän

Det som rör en användare som helhet görs bara med CLI:t på servern. Varje kommando skrivs i installationsloggen.

- `maya user reset <e-post>`: en användare som har tappat alla sina passkeys får en länk där hen registrerar en ny. Länken gäller i 24 timmar och kan användas en gång.
- `maya user deactivate <e-post>`: användaren kan inte logga in, och hens sessioner och oanvända länkar upphör. Användare raderas aldrig.
- `maya user reactivate <e-post>`: användaren kan logga in med sina passkeys igen.
- `maya domain change`: byt först `MAYA_PUBLIC_URL` i `.env`, och kör sedan `docker compose run --rm maya maya domain change` (Maya startar inte med den nya domänen förrän bytet är gjort). Kommandot sparar nytt `rp_id` och origin och tömmer alla passkeys och sessioner. Ge sedan varje användare en ny passkey med `maya user reset` och starta Maya.

## Backup

Maya säkerhetskopierar hela installationen varje natt på `MAYA_BACKUP_TIME` (som standard 03:00 i `Europe/Stockholm`). Backupen är en ögonblicksbild av databasen med `VACUUM INTO`, alltså också alla underlag, och ett manifest med kontrollsummor: antal bokförda verifikat per serie och räkenskapsår, debet och kredit per konto och år, en hash över de bokförda raderna och varje underlags SHA-256. I målet heter filerna `maya-<tid i UTC>.db` och `maya-<tid i UTC>.json`.

Direkt efter varje backup läser Maya tillbaka den ur målet och verifierar den: filens SHA-256 mot manifestet, `PRAGMA integrity_check`, underlagens innehåll och kontrollsummorna. Sedan gallras äldre backuper: den senaste från var och en av de 7 senaste dagarna och de 12 senaste månaderna sparas. Resultatet syns i loggen och sparas för Att göra och sidan Installation.

- **Katalog** (`MAYA_BACKUP_DIR`, i containern `/backup`): lägg den på en annan disk än databasen. Maya varnar vid start om katalogen ligger på samma enhet som databasen, eftersom kopian då inte är åtskild från originalet.
- **S3-kompatibel lagring** (`MAYA_BACKUP_S3_ENDPOINT`, `_BUCKET`, `_REGION`, `_ACCESS_KEY_ID`, `_SECRET_ACCESS_KEY` eller `_SECRET_ACCESS_KEY_FILE`): Maya använder path-style-adresser (`<endpoint>/<bucket>/<fil>`), så det fungerar med AWS S3, MinIO, Garage och liknande. Bucketen måste finnas; nyckeln behöver rätt att lista, läsa, skriva och radera i den.

Lagra backupen i Sverige. Förvaring i ett annat EU-land kräver en anmälan till Skatteverket och utanför EU ett tillstånd (BFL 7:3 a). Maya krypterar inte backupen; om målet är krypterat ska nyckeln bevaras som räkenskapsinformation och anges i arkivplanen (`MAYA_ARKIVPLAN_NYCKEL`), så att kopian kan läsas under hela bevarandetiden.

Provåterställ en gång per år: `docker compose exec maya maya backup verify` verifierar den senaste backupen (eller `maya backup verify <namn>` en viss), skriver resultatet på svenska och loggar körningen i installationsloggen.

## Återställa

`maya restore` återställer hela installationen ur en backup i backupmålet; ett enskilt företag går inte att återställa. Stoppa servern först, eftersom databasfilen byts ut:

```sh
docker compose stop maya
docker compose run --rm maya restore            # den senaste backupen
docker compose run --rm maya restore maya-20261018T010000Z
docker compose start maya
```

Kommandot visar backupens tidpunkt och kräver att du skriver `ja`. Backupen granskas mot sitt manifest innan något ändras, och en backup vars `rp_id` skiljer sig från domänen i `MAYA_PUBLIC_URL` vägras, eftersom passkeys är bundna till domänen. Den nuvarande databasen raderas inte utan flyttas till `maya.db.fore-aterstallning-<tid i UTC>` i datakatalogen. Efter återställningen jämförs kontrollsummorna med backupen; stämmer de inte läggs den tidigare databasen tillbaka. Återställningen loggas i varje företags behandlingshistorik och i installationsloggen, och varje användare ser en varning under Att göra tills hen kvitterar den: det som registrerades efter backupen måste registreras om.

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
