-- Företag, åtkomst, räkenskapsår med perioder, bokföringsmetod och momsperiod med historik,
-- kontoplanen och behandlingshistoriken. Datum är ÅÅÅÅ-MM-DD, tider RFC 3339 i UTC.

CREATE TABLE foretag (
    id INTEGER PRIMARY KEY,
    -- Tio siffror utan bindestreck. Unikt i installationen och ändras aldrig.
    orgnr TEXT NOT NULL UNIQUE CHECK (length(orgnr) = 10 AND orgnr NOT GLOB '*[^0-9]*'),
    namn TEXT NOT NULL CHECK (namn <> ''),
    skapad TEXT NOT NULL
) STRICT;

-- Åtkomst är en relation mellan användare och företag, utan roller (ADR 0005).
CREATE TABLE atkomst (
    foretag_id INTEGER NOT NULL REFERENCES foretag (id),
    anvandare_id INTEGER NOT NULL REFERENCES anvandare (id),
    PRIMARY KEY (foretag_id, anvandare_id)
) STRICT;

CREATE INDEX atkomst_anvandare ON atkomst (anvandare_id);

CREATE TABLE rakenskapsar (
    id INTEGER PRIMARY KEY,
    foretag_id INTEGER NOT NULL REFERENCES foretag (id),
    start TEXT NOT NULL,
    slut TEXT NOT NULL CHECK (slut >= start),
    UNIQUE (foretag_id, start)
) STRICT;

-- Kalendermånaderna inom ett räkenskapsår; den första kan vara en del av en månad.
CREATE TABLE period (
    id INTEGER PRIMARY KEY,
    rakenskapsar_id INTEGER NOT NULL REFERENCES rakenskapsar (id),
    start TEXT NOT NULL,
    slut TEXT NOT NULL CHECK (slut >= start),
    UNIQUE (rakenskapsar_id, start)
) STRICT;

-- Bokföringsmetoden med historik: varje rad gäller från sitt datum till nästa rads.
CREATE TABLE bokforingsmetod (
    foretag_id INTEGER NOT NULL REFERENCES foretag (id),
    galler_fran TEXT NOT NULL,
    metod TEXT NOT NULL CHECK (metod IN ('faktura', 'kontant')),
    PRIMARY KEY (foretag_id, galler_fran)
) STRICT;

-- Momsperioden med historik, på samma sätt.
CREATE TABLE momsperiod (
    foretag_id INTEGER NOT NULL REFERENCES foretag (id),
    galler_fran TEXT NOT NULL,
    momsperiod TEXT NOT NULL
        CHECK (momsperiod IN ('manad', 'kvartal', 'helar', 'ej_momsregistrerad')),
    PRIMARY KEY (foretag_id, galler_fran)
) STRICT;

-- Företagets kontoplan: en egen kopia av BAS. Ett konto raderas aldrig och byter aldrig nummer.
CREATE TABLE konto (
    foretag_id INTEGER NOT NULL REFERENCES foretag (id),
    nummer INTEGER NOT NULL CHECK (nummer BETWEEN 1000 AND 8999),
    namn TEXT NOT NULL,
    -- Namnet i BAS när kontot kopierades; NULL för ett eget konto.
    bas_namn TEXT,
    ursprung TEXT NOT NULL CHECK (ursprung IN ('bas', 'eget')),
    aktiv INTEGER NOT NULL CHECK (aktiv IN (0, 1)),
    -- BAS-versionen som kontot kom med; NULL för ett eget konto.
    bas_version TEXT,
    PRIMARY KEY (foretag_id, nummer),
    CHECK ((ursprung = 'bas') = (bas_version IS NOT NULL AND bas_namn IS NOT NULL))
) STRICT;

-- Behandlingshistoriken per företag. Append-only (H3).
CREATE TABLE behandlingshistorik (
    id INTEGER PRIMARY KEY,
    foretag_id INTEGER NOT NULL REFERENCES foretag (id),
    tid TEXT NOT NULL,
    kanal TEXT NOT NULL CHECK (kanal IN ('webb', 'cli', 'maya')),
    anvandare_id INTEGER REFERENCES anvandare (id),
    typ TEXT NOT NULL,
    objekt TEXT NOT NULL,
    -- Strukturerad beskrivning av ändringen som JSON.
    beskrivning TEXT NOT NULL CHECK (json_valid(beskrivning))
) STRICT;

CREATE INDEX behandlingshistorik_foretag ON behandlingshistorik (foretag_id, id);

CREATE TRIGGER behandlingshistorik_andras_inte
BEFORE UPDATE ON behandlingshistorik
BEGIN
    SELECT RAISE(ABORT, 'behandlingshistoriken går inte att ändra');
END;

CREATE TRIGGER behandlingshistorik_raderas_inte
BEFORE DELETE ON behandlingshistorik
BEGIN
    SELECT RAISE(ABORT, 'behandlingshistoriken går inte att radera');
END;
