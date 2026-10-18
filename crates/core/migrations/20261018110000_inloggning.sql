-- Användare, passkeys, engångslänkar och sessioner. Tider är RFC 3339 i UTC.

-- En användare raderas aldrig, bara avaktiveras.
CREATE TABLE anvandare (
    id INTEGER PRIMARY KEY,
    -- WebAuthn user handle: 16 slumpade byte, aldrig personuppgifter.
    handle BLOB NOT NULL UNIQUE CHECK (length(handle) = 16),
    epost TEXT NOT NULL UNIQUE,
    visningsnamn TEXT NOT NULL,
    skapad TEXT NOT NULL,
    avaktiverad TEXT
) STRICT;

CREATE TABLE passkey (
    -- Primärnyckeln ger kontrollen att samma credential inte registreras två gånger.
    credential_id BLOB PRIMARY KEY,
    anvandare_id INTEGER NOT NULL REFERENCES anvandare (id),
    namn TEXT NOT NULL,
    -- `webauthn_rs::prelude::Passkey` som JSON.
    passkey TEXT NOT NULL,
    skapad TEXT NOT NULL,
    senast_anvand TEXT
) STRICT;

CREATE INDEX passkey_anvandare ON passkey (anvandare_id);

-- Registrerings-, inbjudnings- och återställningslänkar. Bara SHA-256 av token sparas.
CREATE TABLE engangslank (
    hash BLOB PRIMARY KEY,
    syfte TEXT NOT NULL CHECK (syfte IN ('registrering')),
    anvandare_id INTEGER NOT NULL REFERENCES anvandare (id),
    skapad TEXT NOT NULL,
    giltig_till TEXT NOT NULL,
    anvand TEXT
) STRICT;

-- Bara SHA-256 av sessionstoken sparas.
CREATE TABLE session (
    hash BLOB PRIMARY KEY,
    anvandare_id INTEGER NOT NULL REFERENCES anvandare (id),
    skapad TEXT NOT NULL,
    senast_aktiv TEXT NOT NULL,
    -- När användaren senast använde en passkey i sessionen (färsk passkey).
    passkey_anvand TEXT NOT NULL
) STRICT;

CREATE INDEX session_anvandare ON session (anvandare_id);

-- Den installationsövergripande loggen: CLI-händelser och radering av företag. Append-only (H3).
CREATE TABLE installationslogg (
    id INTEGER PRIMARY KEY,
    tid TEXT NOT NULL,
    kanal TEXT NOT NULL CHECK (kanal IN ('webb', 'cli', 'maya')),
    anvandare_id INTEGER REFERENCES anvandare (id),
    typ TEXT NOT NULL,
    objekt TEXT NOT NULL,
    -- Strukturerad beskrivning av händelsen som JSON.
    beskrivning TEXT NOT NULL CHECK (json_valid(beskrivning))
) STRICT;

CREATE TRIGGER installationslogg_andras_inte
BEFORE UPDATE ON installationslogg
BEGIN
    SELECT RAISE(ABORT, 'installationsloggen går inte att ändra');
END;

CREATE TRIGGER installationslogg_raderas_inte
BEFORE DELETE ON installationslogg
BEGIN
    SELECT RAISE(ABORT, 'installationsloggen går inte att radera');
END;
