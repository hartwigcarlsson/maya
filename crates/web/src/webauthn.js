// Mayas JS-shim för passkeys: översätter mellan webauthn-rs JSON (base64url) och
// navigator.credentials. E-post först och sedan passkey, ingen conditional UI.
(() => {
  "use strict";

  const avkoda = (text) => {
    const b64 = text.replace(/-/g, "+").replace(/_/g, "/");
    const binär = atob(b64 + "=".repeat((4 - (b64.length % 4)) % 4));
    return Uint8Array.from(binär, (c) => c.charCodeAt(0)).buffer;
  };

  const koda = (buffer) => {
    let binär = "";
    for (const byte of new Uint8Array(buffer)) binär += String.fromCharCode(byte);
    return btoa(binär).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
  };

  const skicka = async (sökväg, data) => {
    const svar = await fetch(sökväg, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      credentials: "same-origin",
      body: JSON.stringify(data),
    });
    const json = await svar.json().catch(() => ({}));
    if (!svar.ok) throw new Error(json.fel || "något gick fel i Maya; försök igen");
    return json;
  };

  // Skapar en passkey: `start` ger en utmaning, `slutför` tar emot den nya passkeyn.
  const skapa = async (form, start, slutför, data) => {
    const { ceremoni, utmaning } = await skicka(start, data);
    const val = utmaning.publicKey;
    val.challenge = avkoda(val.challenge);
    val.user.id = avkoda(val.user.id);
    for (const c of val.excludeCredentials || []) c.id = avkoda(c.id);
    const kred = await navigator.credentials.create({ publicKey: val });
    await skicka(slutför, {
      ceremoni,
      namn: new FormData(form).get("namn") || "",
      svar: {
        id: kred.id,
        rawId: koda(kred.rawId),
        type: kred.type,
        response: {
          attestationObject: koda(kred.response.attestationObject),
          clientDataJSON: koda(kred.response.clientDataJSON),
          transports: kred.response.getTransports ? kred.response.getTransports() : undefined,
        },
        extensions: kred.getClientExtensionResults(),
      },
    });
  };

  // Använder en passkey: `start` ger en utmaning, `slutför` verifierar svaret.
  const använd = async (start, slutför, data) => {
    const { ceremoni, utmaning } = await skicka(start, data);
    const val = utmaning.publicKey;
    val.challenge = avkoda(val.challenge);
    for (const c of val.allowCredentials || []) c.id = avkoda(c.id);
    const kred = await navigator.credentials.get({ publicKey: val });
    await skicka(slutför, {
      ceremoni,
      svar: {
        id: kred.id,
        rawId: koda(kred.rawId),
        type: kred.type,
        response: {
          authenticatorData: koda(kred.response.authenticatorData),
          clientDataJSON: koda(kred.response.clientDataJSON),
          signature: koda(kred.response.signature),
          userHandle: kred.response.userHandle ? koda(kred.response.userHandle) : null,
        },
        extensions: kred.getClientExtensionResults(),
      },
    });
  };

  const flöden = {
    // Registrerings- och återställningslänkarna från CLI:t.
    registrera: (form) =>
      skapa(form, "/auth/registrering/start", "/auth/registrering/slutfor", {
        token: form.dataset.token,
      }),
    "logga-in": (form) =>
      använd("/auth/inloggning/start", "/auth/inloggning/slutfor", {
        epost: new FormData(form).get("epost") || "",
      }),
    // En inloggad användare lägger till en passkey.
    "lagg-till": (form) => skapa(form, "/auth/passkey/start", "/auth/passkey/slutfor", {}),
    // En inloggad användare använder sin passkey så att den blir färsk.
    bekrafta: () => använd("/auth/bekraftelse/start", "/auth/bekraftelse/slutfor", {}),
  };
  // Röktestet väntar på det här innan det klickar.
  document.documentElement.dataset.passkeyShim = "klar";

  document.addEventListener("submit", async (händelse) => {
    const form = händelse.target;
    const flöde = flöden[form.dataset && form.dataset.passkey];
    if (!flöde) return;
    händelse.preventDefault();
    const fel = form.querySelector("[data-fel]");
    const knapp = form.querySelector("button[type=submit]");
    if (fel) fel.hidden = true;
    if (knapp) knapp.disabled = true;
    try {
      if (!window.PublicKeyCredential) throw new Error("webbläsaren stöder inte passkeys");
      await flöde(form);
      window.location.assign(form.dataset.efter || "/");
    } catch (e) {
      if (fel) {
        fel.textContent =
          e && e.name === "NotAllowedError"
            ? "Passkeyn användes inte. Försök igen."
            : "Det gick inte: " + (e && e.message ? e.message : e);
        fel.hidden = false;
      }
      if (knapp) knapp.disabled = false;
    }
  });
})();
