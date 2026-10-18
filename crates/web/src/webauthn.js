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

  const registrera = async (form) => {
    const { ceremoni, utmaning } = await skicka("/auth/registrering/start", {
      token: form.dataset.token,
    });
    const val = utmaning.publicKey;
    val.challenge = avkoda(val.challenge);
    val.user.id = avkoda(val.user.id);
    for (const c of val.excludeCredentials || []) c.id = avkoda(c.id);
    const kred = await navigator.credentials.create({ publicKey: val });
    await skicka("/auth/registrering/slutfor", {
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

  const loggaIn = async (form) => {
    const { ceremoni, utmaning } = await skicka("/auth/inloggning/start", {
      epost: new FormData(form).get("epost") || "",
    });
    const val = utmaning.publicKey;
    val.challenge = avkoda(val.challenge);
    for (const c of val.allowCredentials || []) c.id = avkoda(c.id);
    const kred = await navigator.credentials.get({ publicKey: val });
    await skicka("/auth/inloggning/slutfor", {
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

  const flöden = { registrera, "logga-in": loggaIn };

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
      window.location.assign("/");
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
