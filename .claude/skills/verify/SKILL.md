---
name: verify
description: Run the real Maya server and drive its web UI with Playwright (passkey sign-up via a virtual authenticator) to verify a release or feature end to end, without running the tests.
---

# Verify Maya in a real browser

## 1. Build

`cargo leptos build` in the repo root. It takes about 3 minutes cold. You get `target/debug/maya` and the site in `target/site`. The binary finds the site from a path compiled in, so the server can run from any directory.

## 2. Launch the server in a fresh directory

The server reads `.env` from its working directory, the way `Server::starta` in `crates/maya/tests/it/rok.rs` does. Port 3102 belongs to the `maya` entry in launch.json, so use a free port.

```sh
D=$(mktemp -d); PORT=<free port>
printf "MAYA_PUBLIC_URL=http://localhost:$PORT\nMAYA_LISTEN=127.0.0.1:$PORT\nMAYA_BACKUP_DIR=$D/backup\nMAYA_DATA_DIR=$D/data\n" > $D/.env
mkdir -p $D/backup
cd $D && nohup env -i HOME=$HOME PATH=$PATH <repo>/target/debug/maya serve > <scratch>/server.log 2>&1 &
until curl -sf http://127.0.0.1:$PORT/healthz; do sleep 0.3; done
cd $D && env -i HOME=$HOME PATH=$PATH <repo>/target/debug/maya user create anna@example.se   # prints the /registrera/<token> link
```

## 3. Keep one browser alive, then drive it with short scripts

- Playwright is cached at `~/.npm/_npx/*/node_modules/playwright`. Symlink that `node_modules` into your scratch directory, not into the repo.
- Start a "keeper" process. It runs `chromium.launchPersistentContext(dir, {headless: true, args: ['--remote-debugging-port=9333']})`, opens a CDP session on the page, and sends:
  - `WebAuthn.enable {enableUI: false}`
  - `WebAuthn.addVirtualAuthenticator` with `{protocol: 'ctap2', ctap2Version: 'ctap2_1', transport: 'internal', hasResidentKey: true, hasUserVerification: true, isUserVerified: true, automaticPresenceSimulation: true}`

  The keeper also logs `console` errors and `pageerror` events to a file, and then stays idle. The authenticator lasts only as long as this CDP session does.
- Each step script calls `chromium.connectOverCDP('http://127.0.0.1:9333')` and takes `contexts()[0].pages()[0]`. It ends with `process.exit(0)`. Never call `browser.close()`, because that closes the shared browser. The session cookie stays in the profile, so you only sign in once.

## 4. Sign up and set up a company

- Open the link and wait for `html[data-passkey-shim]`. Fill `#namn`, click `form[data-passkey='registrera'] button`, and wait for the text "Lägg till en passkey till".
- In `/foretag/nytt`, fill `#orgnr` (it must pass Luhn, e.g. 556036-0793 or 556677-8899), `#namn`, `#start` and `#slut`, and choose `#bokforingsmetod` (`faktura` or `kontant`) and `#momsperiod` (`kvartal`). Then submit `form[action='/foretag'] button`.
- Inställningar has the form `form[action$='/installningar/uppgifter']`, which needs the address, `#sate`, `#momsregnr` and `f_skatt`. It also has `form[action$='/fakturauppgifter']`, which needs `#bankgiro`. Without these, the invoice form cannot issue an invoice.

## 5. Driving tips

- Wait for `html[data-hydrerad]` before you type. The forms validate live: the button text reads "Granska och utfärda · N problem kvar".
- Shortcuts such as P (preview) and ⌘K (search) act only when no input has focus. Blur the field first, otherwise P is typed into it.
- After clicking a link, the client router keeps the old `main` for a moment. Use `page.goto` before reading text, or wait for the content you expect.
- `?f=<id>&panel=betalning|kreditera|ratta-betalning` opens panels in invoice documents. The ids in `verifikat?v=` are global, not V-numbers.
- The selectors in `crates/maya/tests/it/rok.rs` (the `klicka`/`fyll` calls) are the best map of the UI.

## 6. Clean up

Kill the server pid and the keeper pid, which closes Chromium.
