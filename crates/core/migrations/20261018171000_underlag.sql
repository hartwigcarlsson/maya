-- Underlag (ADR 0002): originalfilen som BLOB i samma databas, med SHA-256 räknad när den togs
-- emot. Innehållet ändras aldrig (VF5). Ett underlag kopplas till verifikat många-till-många. En
-- koppling till ett utkast kan tas bort; en koppling till ett bokfört verifikat aldrig, och en
-- koppling som görs efter bokföringen är en komplettering med vem och när (VF6). Ett underlag som
-- har hört till ett bokfört verifikat kan inte tas bort.

CREATE TABLE underlag (
    -- AUTOINCREMENT: ett borttaget underlags id återanvänds aldrig, så behandlingshistoriken
    -- pekar bara på ett underlag.
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    foretag_id INTEGER NOT NULL REFERENCES foretag (id),
    filnamn TEXT NOT NULL CHECK (filnamn <> ''),
    mime TEXT NOT NULL,
    -- Högst 25 MB (maya_domain::underlag::STÖRSTA_STORLEK).
    storlek INTEGER NOT NULL CHECK (storlek BETWEEN 0 AND 26214400),
    -- Hex, gemener.
    sha256 TEXT NOT NULL CHECK (length(sha256) = 64),
    innehall BLOB NOT NULL,
    uppladdad TEXT NOT NULL,
    uppladdad_kanal TEXT NOT NULL CHECK (uppladdad_kanal IN ('webb', 'cli', 'maya')),
    uppladdad_av INTEGER REFERENCES anvandare (id),
    CHECK (length(innehall) = storlek)
) STRICT;

CREATE INDEX underlag_foretag ON underlag (foretag_id, sha256);

CREATE TABLE underlagskoppling (
    underlag_id INTEGER NOT NULL REFERENCES underlag (id),
    verifikat_id INTEGER NOT NULL REFERENCES verifikat (id),
    kopplad TEXT NOT NULL,
    kopplad_kanal TEXT NOT NULL CHECK (kopplad_kanal IN ('webb', 'cli', 'maya')),
    kopplad_av INTEGER REFERENCES anvandare (id),
    -- 1 när kopplingen gjordes efter att verifikatet bokfördes (VF6).
    komplettering INTEGER NOT NULL CHECK (komplettering IN (0, 1)),
    PRIMARY KEY (underlag_id, verifikat_id)
) STRICT;

CREATE INDEX underlagskoppling_verifikat ON underlagskoppling (verifikat_id);

-- Originalet ändras aldrig (VF5).
CREATE TRIGGER underlag_andras_inte
BEFORE UPDATE ON underlag
BEGIN
    SELECT RAISE(ABORT, 'ett underlag går inte att ändra');
END;

-- Ett underlag som har hört till ett bokfört verifikat finns kvar (A1). En koppling till ett
-- bokfört verifikat kan inte tas bort, så det räcker att se efter en sådan koppling.
CREATE TRIGGER underlag_till_bokfort_raderas_inte
BEFORE DELETE ON underlag
WHEN EXISTS (
    SELECT 1 FROM underlagskoppling k JOIN verifikat v ON v.id = k.verifikat_id
    WHERE k.underlag_id = OLD.id AND v.bokford IS NOT NULL
)
BEGIN
    SELECT RAISE(ABORT, 'ett underlag som hör till ett bokfört verifikat går inte att ta bort');
END;

-- Underlaget och verifikatet hör till samma företag. En koppling till ett utkast är vanlig; en
-- koppling till ett bokfört verifikat är en komplettering.
CREATE TRIGGER underlagskoppling_laggs_till
BEFORE INSERT ON underlagskoppling
BEGIN
    SELECT RAISE(ABORT, 'underlaget och verifikatet hör till olika företag')
    WHERE (SELECT foretag_id FROM underlag WHERE id = NEW.underlag_id)
        IS NOT (SELECT foretag_id FROM verifikat WHERE id = NEW.verifikat_id);
    SELECT RAISE(ABORT, 'en koppling till ett bokfört verifikat är en komplettering')
    WHERE ((SELECT bokford FROM verifikat WHERE id = NEW.verifikat_id) IS NOT NULL)
        <> NEW.komplettering;
END;

CREATE TRIGGER underlagskoppling_andras_inte
BEFORE UPDATE ON underlagskoppling
BEGIN
    SELECT RAISE(ABORT, 'en underlagskoppling går inte att ändra');
END;

CREATE TRIGGER underlagskoppling_till_bokfort_raderas_inte
BEFORE DELETE ON underlagskoppling
WHEN (SELECT bokford FROM verifikat WHERE id = OLD.verifikat_id) IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'en koppling till ett bokfört verifikat går inte att ta bort');
END;
