-- Verifikat och verifikatrader. Ett verifikat är ett utkast tills det bokförs: då får det
-- bokförd-tid, bokförd-aktör och nästa nummer i räkenskapsårets V-serie, och sedan ändras det
-- aldrig (L1). Triggers nedan avvisar ändringar av bokfört, och att bokföra något som inte är
-- balanserat (L7) eller med ett nummer som lämnar en lucka eller en dubblett i serien (VF3).
-- Belopp är heltal i öre. Datum ÅÅÅÅ-MM-DD, tider RFC 3339 i UTC.

CREATE TABLE verifikat (
    id INTEGER PRIMARY KEY,
    foretag_id INTEGER NOT NULL REFERENCES foretag (id),
    -- Räkenskapsåret som verifikatdatumet avgör; NULL för ett utkast utan datum.
    rakenskapsar_id INTEGER REFERENCES rakenskapsar (id),
    datum TEXT,
    beskrivning TEXT NOT NULL DEFAULT '',
    -- Var verifikatet kommer ifrån. Rättelser och Mayas flöden läggs till senare.
    kalla TEXT NOT NULL CHECK (kalla IN ('manuell')),
    skapad TEXT NOT NULL,
    skapad_kanal TEXT NOT NULL CHECK (skapad_kanal IN ('webb', 'cli', 'maya')),
    skapad_av INTEGER REFERENCES anvandare (id),
    -- Sätts när verifikatet bokförs, alla tre tillsammans.
    nummer INTEGER CHECK (nummer >= 1),
    bokford TEXT,
    bokford_kanal TEXT CHECK (bokford_kanal IN ('webb', 'cli', 'maya')),
    bokford_av INTEGER REFERENCES anvandare (id),
    CHECK ((nummer IS NULL) = (bokford IS NULL) AND (bokford IS NULL) = (bokford_kanal IS NULL)),
    CHECK (
        bokford IS NULL
        OR (datum IS NOT NULL AND rakenskapsar_id IS NOT NULL AND trim(beskrivning) <> '')
    )
) STRICT;

-- Skyddsnätet för numreringen: ett nummer per företag, räkenskapsår och nummer (VF3).
CREATE UNIQUE INDEX verifikat_nummer ON verifikat (foretag_id, rakenskapsar_id, nummer)
    WHERE nummer IS NOT NULL;

CREATE INDEX verifikat_foretag ON verifikat (foretag_id, id);

CREATE TABLE verifikatrad (
    verifikat_id INTEGER NOT NULL REFERENCES verifikat (id),
    -- Radens plats i verifikatet, från 1.
    radnr INTEGER NOT NULL CHECK (radnr >= 1),
    konto INTEGER NOT NULL CHECK (konto BETWEEN 1000 AND 8999),
    debet INTEGER NOT NULL CHECK (debet >= 0),
    kredit INTEGER NOT NULL CHECK (kredit >= 0),
    text TEXT,
    PRIMARY KEY (verifikat_id, radnr)
) STRICT;

CREATE INDEX verifikatrad_konto ON verifikatrad (konto);

-- Ett verifikat skapas som utkast och bokförs med en UPDATE, så att kontrollerna nedan alltid
-- körs.
CREATE TRIGGER verifikat_skapas_obokfort
BEFORE INSERT ON verifikat
WHEN NEW.bokford IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'ett verifikat skapas som utkast och bokförs sedan');
END;

-- Att bokföra: nästa nummer i årets serie, minst två rader, varje rad i debet eller kredit,
-- debet = kredit och bara aktiva konton i företagets kontoplan.
CREATE TRIGGER verifikat_bokfors
BEFORE UPDATE OF bokford ON verifikat
WHEN OLD.bokford IS NULL AND NEW.bokford IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'verifikationsnumret lämnar en lucka eller en dubblett i serien')
    WHERE NEW.nummer IS NOT (
        SELECT coalesce(max(nummer), 0) + 1 FROM verifikat
        WHERE foretag_id = NEW.foretag_id AND rakenskapsar_id = NEW.rakenskapsar_id
    );
    SELECT RAISE(ABORT, 'ett bokfört verifikat behöver minst två rader')
    WHERE (SELECT count(*) FROM verifikatrad WHERE verifikat_id = NEW.id) < 2;
    SELECT RAISE(ABORT, 'varje rad har ett belopp antingen i debet eller i kredit')
    WHERE EXISTS (
        SELECT 1 FROM verifikatrad
        WHERE verifikat_id = NEW.id AND (debet > 0) = (kredit > 0)
    );
    SELECT RAISE(ABORT, 'debet är inte lika med kredit')
    WHERE (SELECT sum(debet) - sum(kredit) FROM verifikatrad WHERE verifikat_id = NEW.id) <> 0;
    SELECT RAISE(ABORT, 'ett konto finns inte eller är inaktivt')
    WHERE EXISTS (
        SELECT 1 FROM verifikatrad r
        LEFT JOIN konto k ON k.foretag_id = NEW.foretag_id AND k.nummer = r.konto
        WHERE r.verifikat_id = NEW.id AND coalesce(k.aktiv, 0) = 0
    );
    SELECT RAISE(ABORT, 'verifikatdatumet ligger inte i räkenskapsåret')
    WHERE NOT EXISTS (
        SELECT 1 FROM rakenskapsar
        WHERE id = NEW.rakenskapsar_id AND foretag_id = NEW.foretag_id
          AND NEW.datum BETWEEN start AND slut
    );
END;

-- Ett bokfört verifikat och dess rader är oföränderliga (L1).
CREATE TRIGGER verifikat_bokfort_andras_inte
BEFORE UPDATE ON verifikat
WHEN OLD.bokford IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'ett bokfört verifikat går inte att ändra');
END;

CREATE TRIGGER verifikat_bokfort_raderas_inte
BEFORE DELETE ON verifikat
WHEN OLD.bokford IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'ett bokfört verifikat går inte att radera');
END;

CREATE TRIGGER verifikatrad_laggs_inte_till_bokfort
BEFORE INSERT ON verifikatrad
WHEN (SELECT bokford FROM verifikat WHERE id = NEW.verifikat_id) IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'ett bokfört verifikat går inte att ändra');
END;

CREATE TRIGGER verifikatrad_bokford_andras_inte
BEFORE UPDATE ON verifikatrad
WHEN (SELECT bokford FROM verifikat WHERE id = OLD.verifikat_id) IS NOT NULL
    OR (SELECT bokford FROM verifikat WHERE id = NEW.verifikat_id) IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'ett bokfört verifikat går inte att ändra');
END;

CREATE TRIGGER verifikatrad_bokford_raderas_inte
BEFORE DELETE ON verifikatrad
WHEN (SELECT bokford FROM verifikat WHERE id = OLD.verifikat_id) IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'ett bokfört verifikat går inte att radera');
END;
