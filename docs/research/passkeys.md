# Passkeys i en Rust-server

Forskningsanteckning för [#10](https://github.com/hartwigcarlsson/maya/issues/10): hur passkey-inloggning (WebAuthn) implementeras i en Rust-server med `webauthn-rs`, hur credentials lagras i SQLite, och hur första användaren bootstrappas och konton återställs utan lösenord.

Researchad 2026-10-12. Källor: W3C WebAuthn Level 3 (W3C Recommendation, 25 augusti 2026), passkeys.dev, FIDO Alliance, källkoden till `webauthn-rs` 0.5.5 (senaste stabila på crates.io, commit [`be696b7`](https://github.com/kanidm/webauthn-rs/tree/be696b79800bd1953df78e87d0215571733cc26f)) samt Kanidm-dokumentationen (samma författare som `webauthn-rs`, som referens för recovery-mönster).

Förkortningar för källor nedan:

- **[WA3]**: <https://www.w3.org/TR/webauthn-3/>
- **[lib]**: [`webauthn-rs/src/lib.rs`](https://github.com/kanidm/webauthn-rs/blob/be696b79800bd1953df78e87d0215571733cc26f/webauthn-rs/src/lib.rs)
- **[iface]**: [`webauthn-rs/src/interface.rs`](https://github.com/kanidm/webauthn-rs/blob/be696b79800bd1953df78e87d0215571733cc26f/webauthn-rs/src/interface.rs)
- **[tut]**: [`tutorial/server/axum`](https://github.com/kanidm/webauthn-rs/tree/be696b79800bd1953df78e87d0215571733cc26f/tutorial/server/axum)

## Sammanfattning

- `webauthn-rs` (crate `webauthn-rs`, v0.5.5) ger ett färdigt "passkey flow" i fyra anrop: `start_passkey_registration` → `finish_passkey_registration` → `start_passkey_authentication` → `finish_passkey_authentication`. Inga andra faktorer behövs; passkey-flödet kräver alltid user verification. ([lib] L14–45, L478–479, L568, L680)
- Det resulterande `Passkey`-objektet är `Serialize`/`Deserialize` och dokumenterat som säkert att lagra i en databas. Ceremony-state (`PasskeyRegistration`, `PasskeyAuthentication`) **måste** lagras serverside; en SQLite-tabell eller en server-side session store duger, en cookie gör det inte. ([iface] L55–57, [lib] L83–103)
- Flera passkeys per användare är normalfallet: man skickar in alla användarens `Passkey` vid autentisering och deras credential IDs som `exclude_credentials` vid registrering. ([lib] L490–494, L674–677)
- Discoverable/usernameless inloggning (conditional UI/autofill) finns bakom feature-flaggan `conditional-ui`, men biblioteksförfattarna avråder från den, och `start_passkey_registration` begär `residentKey: discouraged`. Ett username-first-flöde är det bibliotekets huvudspår. ([lib] L156–171, L566, [core] L298–302)
- Bootstrap och recovery utan lösenord löses i etablerade system med **engångs-tokens** (enrollment/reset link med kort TTL) som utfärdas av en admin, och för den allra första/sista admin-användaren av ett **CLI-kommando på servern** (Kanidm: `kanidmd recover-account`). Spec:en och FIDO rekommenderar dessutom att varje konto har flera registrerade authenticators. ([Kanidm creds], [Kanidm quickstart], [WA3] §6.1.3, [FIDO otp] §3.3.3)

## 1. Registrering

`WebauthnBuilder::new(rp_id, &rp_origin)` bygger en `Webauthn`-instans. `rp_id` måste vara en effective domain av origin, och credentials binds till `rp_id`: den kan **inte** ändras senare utan att alla användares credentials slutar fungera. ([lib] L255–256, L271–273)

Flödet ([lib] L470–575, [tut] `src/auth.rs`):

1. Servern anropar `start_passkey_registration(user_unique_id: Uuid, user_name, user_display_name, exclude_credentials)`. Den returnerar `CreationChallengeResponse` (skickas som JSON till browsern) och `PasskeyRegistration` (sparas serverside). ([lib] L534–540, L496–508)
2. Browsern anropar `navigator.credentials.create()` med optionerna. Tutorialens JS base64url-avkodar `challenge`, `user.id` och `excludeCredentials[].id` och kodar svaret tillbaka. ([tut] `assets/js/auth.js` L13–35)
3. Servern anropar `finish_passkey_registration(&reg, &state)` och får en `Passkey`. ([lib] L652–660)
4. Servern **måste** kontrollera att credential ID:t inte redan är registrerat på något konto. ([lib] L650–651) Spec:en motiverar detta: annars kan en angripare registrera offrets credential på sitt eget konto. ([WA3] §7.1 steg 26)

Detaljer som är värda att känna till:

- `user_unique_id` ska vara ett stabilt, slumpat UUID per användare, inte användarnamnet. `user_name`/`user_display_name` kan ändras när som helst och får inte användas som primärnyckel. ([lib] L481–488, [tut] `src/auth.rs` L63–67) Spec:en säger samma sak om user handle: max 64 byte, får inte innehålla personuppgifter, och bör vara samma för alla credentials på ett konto. ([WA3] §5.4.3, §14.6.1)
- `start_passkey_registration` sätter `attestation: none`, `userVerification: required`, `residentKey: discouraged` (via `require_resident_key(false)`), samt extensions `credProtect`, `uvm` och `credProps`. ([lib] L543–574, [core] L298–302)
- `exclude_credentials` bör innehålla alla credential IDs användaren redan har, så att samma authenticator inte registreras två gånger. ([lib] L490–494, passkeys.dev [bootstrapping])

## 2. Autentisering

1. Servern hämtar användarens alla `Passkey`s och anropar `start_passkey_authentication(&creds)`. Detta ger `RequestChallengeResponse` (till browsern, med `allowCredentials` satt) och `PasskeyAuthentication` (sparas serverside). ([lib] L674–713)
2. Browsern anropar `navigator.credentials.get()`.
3. Servern anropar `finish_passkey_authentication(&cred, &state)` och får ett `AuthenticationResult`. ([lib] L716–721)
4. Om `result.needs_update()` är sant: anropa `passkey.update_credential(&result)` och skriv tillbaka. Det uppdaterar counter och backup-flaggor. Författarna noterar att de flesta synkade passkeys aldrig behöver uppdateras, eftersom de saknar counter. ([iface] L77–88)

Counter-semantik enligt spec: ett nytt värde ≤ lagrat värde kan betyda en klonad authenticator. ([WA3] §6.1.1, [lib] L708–714)

## 3. Discoverable credentials och usernameless login

Terminologin skiljer sig mellan källorna:

- passkeys.dev och FIDO definierar en passkey som en **discoverable credential** (tidigare "resident credential"), som kan användas utan att först ange användar-ID. ([terms])
- `webauthn-rs` definierar däremot passkey som "any possible authenticator" och begär inte resident keys. Motiveringen är att resident keys kan fylla lagringen på CTAP 2.0-säkerhetsnycklar. ([lib] L52–71)

En discoverable credential är en som kan användas när RP:n inte skickar några credential IDs; authenticatorn returnerar då alltid en user handle. ([WA3] §4, §5.2.2, §5.5) Med `residentKey: discouraged` vet man inte säkert om credentialn blev discoverable; `credProps.rk` kan berätta det men är inte garanterat. ([WA3] §5.4.6)

I `webauthn-rs` finns usernameless-flödet bakom feature-flaggan `conditional-ui` ([lib] L156–171, `Cargo.toml` L23–25):

- `start_discoverable_authentication()` skapar en challenge utan `allowCredentials` och tvingar `mediation: conditional`. ([lib] L1317–1345)
- `identify_discoverable_authentication(&cred)` extraherar användarens UUID (user handle) och credential ID ur svaret, så att servern kan slå upp rätt nycklar. ([lib] L1352–1361)
- `finish_discoverable_authentication(&cred, state, &[DiscoverableKey])` slutför. `DiscoverableKey` kan skapas `From<&Passkey>`, så samma lagrade passkeys kan användas i båda flödena. ([lib] L1385–1395, [iface] L623–632, L679–690)

Biblioteket rekommenderar att man alltid stödjer det vanliga passkey-flödet. Conditional UI är bara opportunistiskt, och enligt författarnas användartester är det svårt att aktivera och förvirrande, så de avråder från det. ([lib] L156–168) På klientsidan kräver conditional UI `autocomplete="username webauthn"` på användarnamnsfältet och ett `get()`-anrop med `mediation: "conditional"` vid sidladdning. ([bootstrapping])

## 4. Flera passkeys per användare

- Spec:en: när kontot är känt SHOULD RP:n lista alla kontots credential records i `allowCredentials`. ([WA3] §5.5)
- Spec:en: en credential med `BE = 0` (backup-ineligible) överlever inte förlust av enheten, så RP:n SHOULD se till att kontot har fler authenticators och/eller en recovery-process. Om `BS` går från 1 till 0 bör användaren uppmanas att lägga till en till credential. ([WA3] §6.1.3)
- FIDO rekommenderar två hårdvarunycklar per användare om man använder säkerhetsnycklar. ([FIDO ent] §4.3)
- Level 3 har signal-API:er (`signalUnknownCredential`, `signalAllAcceptedCredentials`, `signalCurrentUserDetails`) som låter RP:n tala om för passkey-providern att en credential tagits bort eller att namnet ändrats. ([WA3] §5.1.10) `webauthn-rs` 0.5.5 har inget server-API för detta (det är rena klient-anrop i JS); detta har jag inte verifierat i detalj.

## 5. Lagring i SQLite

`webauthn-rs` har ingen databasintegration. Tutorialen lagrar allt i en `HashMap` bakom en `Mutex` och har en TODO om att persistera i en databas. ([tut] `src/auth.rs` L140–148) `Passkey` är dock dokumenterat som säkert att serialisera till en databas ([iface] L55–57), och den inre `Credential` (`CredentialV5`) innehåller `cred_id`, publik nyckel, `counter`, `transports`, `user_verified`, `backup_eligible`, `backup_state`, registreringspolicy, extensions och attestation. ([core-iface] L252–300) Det täcker spec:ens rekommenderade "credential record". ([WA3] §4)

Ett rimligt mönster (min slutsats, inte en källas):

```sql
CREATE TABLE users (
  id           BLOB PRIMARY KEY,   -- 16 byte UUID = WebAuthn user handle
  username     TEXT NOT NULL UNIQUE,
  display_name TEXT NOT NULL
);

CREATE TABLE passkeys (
  credential_id BLOB PRIMARY KEY,  -- Passkey::cred_id(); PK ger unikhetskontrollen i §7.1 steg 26
  user_id       BLOB NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  label         TEXT NOT NULL,     -- "MacBook", "YubiKey", för UI:t
  passkey       TEXT NOT NULL,     -- serde_json av webauthn_rs::prelude::Passkey
  created_at    TEXT NOT NULL,
  last_used_at  TEXT
);

CREATE TABLE webauthn_ceremonies (  -- PasskeyRegistration/PasskeyAuthentication
  id         BLOB PRIMARY KEY,      -- slumpat, refereras från en HttpOnly-cookie
  state      TEXT NOT NULL,         -- serde_json, kräver feature danger-allow-state-serialisation
  expires_at TEXT NOT NULL
);
```

Om ceremony-state:

- State-typerna går inte att serialisera som standard, just för att förhindra att de hamnar i en cookie. Serialisering till en databas, eller en cookie som bara är en nyckel till server-side state, nämns uttryckligen som säkert. Det kräver feature `danger-allow-state-serialisation`. ([lib] L83–103)
- Tutorialen gör just så: `danger-allow-state-serialisation` och `tower-sessions` med `MemoryStore`, med kommentaren att det är säkert eftersom storen är serverside. ([tut] `Cargo.toml` L21–23, `src/main.rs` L46, `src/auth.rs` L98–100)
- Ett alternativ utan feature-flaggan är en in-memory-map i servern, nycklad på ett slumpat ceremony-ID. Det fungerar för en process med få användare, men ceremonier i flykt försvinner vid omstart.
- State ska tas bort efter användning (tutorialen gör `remove_value` innan `finish_*`), så att den inte kan återanvändas. ([tut] `src/auth.rs` L125–133)

## 6. Bootstrap av första användaren

Ingen av spec-källorna (W3C, passkeys.dev, FIDO) beskriver hur den allra första användaren skapas i ett system utan lösenord. passkeys.dev förutsätter att användaren är "sufficiently strongly authenticated using other login methods" innan en passkey registreras, eller är nyregistrerad. ([bootstrapping]) Det etablerade mönstret i ett lösenordsfritt Rust-system är Kanidms:

- Admin-kontona har inga användbara credentials från början. De återställs med ett **kommando som körs på servern** (`kanidmd recover-account admin` / `idm_admin`). Rätten att köra kommandon på värden är alltså root of trust. ([Kanidm quickstart])
- Admin skapar sedan personkonton och utfärdar en **credential reset token** (`kanidm person credential create-reset-token <user> [ttl]`). Användaren löser in den via QR-kod, CLI eller `/ui/reset` och registrerar sina credentials. ([Kanidm creds])
- Tokens gäller 1 timme som standard och högst 24 timmar, och ogiltigförklaras direkt när återställningen committas. ([Kanidm creds])

För Maya motsvarar det: ett CLI-subkommando (t.ex. `maya admin enroll <user>`) som körs på servern, skapar användaren om den saknas och skriver ut en engångslänk `https://<host>/enroll/<token>`. Länken leder till en sida som kör `start_passkey_registration`. Tokenen lagras hashad i SQLite med `expires_at` och `used_at`. (Min syntes av mönstret ovan.)

## 7. Återställning när alla passkeys tappats

Källorna pekar åt samma håll:

- **Förebygg**: flera authenticators per konto ([WA3] §6.1.3, [FIDO ent] §4.3). Synkade passkeys (iCloud Keychain, Google Password Manager, Bitwarden m.fl.) återställs till en ny enhet via providern. ([terms], [FIDO ent] §4.2)
- **Device-bound passkeys**: återställ via en backup-metod, "using a backup authenticator, to bootstrap enrollment of a new authenticator". ([FIDO otp] §3.3.3)
- **Recovery får inte vara svagare än credentialn**: "recovery mechanisms should never depend on weaker factors than the credential that the user is trying to recover". ([FIDO otp] §3.3.3) Det utesluter t.ex. "magic link via e-post" som bakdörr, om man tar principen på allvar.
- **Admin-assisterad återställning**: en annan betrodd användare (admin) utfärdar en reset-token med kort TTL och engångsanvändning, som ovan. ([Kanidm creds]) Om den enda admin har tappat allt, är server-CLI:t sista utvägen, precis som vid bootstrap. ([Kanidm quickstart])

FIDO:s enterprise-paper beskriver recovery via ett kvarvarande lösenord. Det mönstret är inte tillämpligt i ett lösenordsfritt system. ([FIDO ent] §4.3)

## Öppna frågor

- **Username-first eller usernameless?** `webauthn-rs` stödjer båda men avråder från conditional UI. Med få användare är username-first enklast; usernameless kräver feature `conditional-ui` och bästa-försök-resident keys.
- **Vilket `rp_id`/domännamn?** Det låses för all framtid ([lib] L255–256), och självhostning bakom t.ex. Tailscale eller en lokal domän påverkar valet. WebAuthn kräver en secure context (HTTPS eller `localhost`), vilket påverkar dev-setup och LAN-installationer. (Secure context-kravet har jag inte verifierat mot spec-texten i denna session.)
- **Sessionshantering efter inloggning** (cookie, livslängd, re-auth för känsliga operationer som att lägga till/ta bort passkeys) ligger utanför WebAuthn och behöver ett eget beslut.
- **Ceremony-state**: SQLite-tabell (kräver `danger-allow-state-serialisation`) eller in-memory-map?
- **Klientsidan i Leptos**: egen JS-shim som i tutorialen, eller `web-sys`/wasm (tutorialen har även en `wasm`-variant)? Level 3:s `PublicKeyCredential.parseCreationOptionsFromJSON()` kan eventuellt ersätta base64-hanteringen; inte verifierat mot webbläsarstöd.
- **Recovery-policy**: räcker admin-utfärdad token + server-CLI, eller vill man ha utskrivna recovery-koder? Inga primärkällor rekommenderar recovery-koder för passkeys, och de måste i så fall vara minst lika starka som en passkey ([FIDO otp] §3.3.3).

## Källor

- [WA3] W3C, *Web Authentication: An API for accessing Public Key Credentials, Level 3*, W3C Recommendation 25 augusti 2026. <https://www.w3.org/TR/webauthn-3/>
- [lib] `webauthn-rs/src/lib.rs` @ `be696b7`. <https://github.com/kanidm/webauthn-rs/blob/be696b79800bd1953df78e87d0215571733cc26f/webauthn-rs/src/lib.rs>
- [iface] `webauthn-rs/src/interface.rs` @ `be696b7`. <https://github.com/kanidm/webauthn-rs/blob/be696b79800bd1953df78e87d0215571733cc26f/webauthn-rs/src/interface.rs>
- [core] `webauthn-rs-core/src/core.rs` @ `be696b7`. <https://github.com/kanidm/webauthn-rs/blob/be696b79800bd1953df78e87d0215571733cc26f/webauthn-rs-core/src/core.rs>
- [core-iface] `webauthn-rs-core/src/interface.rs` @ `be696b7`. <https://github.com/kanidm/webauthn-rs/blob/be696b79800bd1953df78e87d0215571733cc26f/webauthn-rs-core/src/interface.rs>
- `webauthn-rs/Cargo.toml` @ `be696b7` (features). <https://github.com/kanidm/webauthn-rs/blob/be696b79800bd1953df78e87d0215571733cc26f/webauthn-rs/Cargo.toml>
- [tut] axum-tutorial @ `be696b7`. <https://github.com/kanidm/webauthn-rs/tree/be696b79800bd1953df78e87d0215571733cc26f/tutorial/server/axum>
- docs.rs: <https://docs.rs/webauthn-rs/0.5.5/webauthn_rs/>
- [bootstrapping] passkeys.dev, *Bootstrapping*. <https://passkeys.dev/docs/use-cases/bootstrapping/>
- [terms] passkeys.dev, *Terms*. <https://passkeys.dev/docs/reference/terms/>
- [FIDO otp] FIDO Alliance, *Displace Password + OTP Authentication with Passkeys* (2024-09-17), §3.3.3 Recovery. <https://fidoalliance.org/white-paper-displace-password-otp-authentication-with-passkeys/>
- [FIDO ent] FIDO Alliance, *Replacing Password-Only Authentication with Passkeys in the Enterprise* (2024-08-29), §4.2–4.3. <https://fidoalliance.org/white-paper-replacing-password-only-authentication-with-passkeys-in-the-enterprise/>
- [Kanidm creds] Kanidm, *Authentication and Credentials*. <https://kanidm.github.io/kanidm/stable/accounts/authentication_and_credentials.html>
- [Kanidm quickstart] Kanidm, *Evaluation Quickstart*. <https://kanidm.github.io/kanidm/stable/evaluation_quickstart.html>
