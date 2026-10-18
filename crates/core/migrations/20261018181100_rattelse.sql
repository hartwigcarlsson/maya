-- no-transaction
-- Rättelse och registrera om (R1, R2, VF7). En rättelse är ett nytt verifikat med källan
-- 'rattelse' som pekar på originalet med `rattar`; originalet ändras aldrig (L1), så länken från
-- originalet till rättelsen läses ur rättelsen. Ett verifikat rättas högst en gång (unikt index)
-- och en rättelse rättas inte. Ett utkast som registrerar om originalet har `kopia_av`.
--
-- SQLite kan inte ändra en CHECK, så verifikat byggs om. Andra tabeller pekar på verifikat, så
-- främmande nycklar stängs av under bytet (det går inte inne i en transaktion, därav
-- `no-transaction`), och `legacy_alter_table` hindrar att namnbytet prövar triggers som läser
-- verifikat medan tabellen saknas. Bytet görs i en egen transaktion och kontrollerar de främmande
-- nycklarna innan det sparas. Triggers på verifikat försvinner med tabellen och skapas om.

PRAGMA foreign_keys = OFF;
PRAGMA legacy_alter_table = ON;

BEGIN IMMEDIATE;

CREATE TABLE verifikat_ny (
    id INTEGER PRIMARY KEY,
    foretag_id INTEGER NOT NULL REFERENCES foretag (id),
    -- Räkenskapsåret som verifikatdatumet avgör; NULL för ett utkast utan datum.
    rakenskapsar_id INTEGER REFERENCES rakenskapsar (id),
    datum TEXT,
    beskrivning TEXT NOT NULL DEFAULT '',
    -- Var verifikatet kommer ifrån. Mayas flöden läggs till senare.
    kalla TEXT NOT NULL CHECK (kalla IN ('manuell', 'rattelse')),
    skapad TEXT NOT NULL,
    skapad_kanal TEXT NOT NULL CHECK (skapad_kanal IN ('webb', 'cli', 'maya')),
    skapad_av INTEGER REFERENCES anvandare (id),
    -- Sätts när verifikatet bokförs, alla tre tillsammans.
    nummer INTEGER CHECK (nummer >= 1),
    bokford TEXT,
    bokford_kanal TEXT CHECK (bokford_kanal IN ('webb', 'cli', 'maya')),
    bokford_av INTEGER REFERENCES anvandare (id),
    -- Verifikatet som en rättelse rättar.
    rattar INTEGER REFERENCES verifikat (id),
    -- Verifikatet som ett utkast från "registrera om" är en kopia av.
    kopia_av INTEGER REFERENCES verifikat (id),
    CHECK ((nummer IS NULL) = (bokford IS NULL) AND (bokford IS NULL) = (bokford_kanal IS NULL)),
    CHECK (
        bokford IS NULL
        OR (datum IS NOT NULL AND rakenskapsar_id IS NOT NULL AND trim(beskrivning) <> '')
    ),
    CHECK ((kalla = 'rattelse') = (rattar IS NOT NULL))
) STRICT;

INSERT INTO verifikat_ny
    (id, foretag_id, rakenskapsar_id, datum, beskrivning, kalla, skapad, skapad_kanal,
     skapad_av, nummer, bokford, bokford_kanal, bokford_av)
SELECT id, foretag_id, rakenskapsar_id, datum, beskrivning, kalla, skapad, skapad_kanal,
       skapad_av, nummer, bokford, bokford_kanal, bokford_av
FROM verifikat;

DROP TABLE verifikat;

ALTER TABLE verifikat_ny RENAME TO verifikat;

-- Skyddsnätet för numreringen: ett nummer per företag, räkenskapsår och nummer (VF3).
CREATE UNIQUE INDEX verifikat_nummer ON verifikat (foretag_id, rakenskapsar_id, nummer)
    WHERE nummer IS NOT NULL;

CREATE INDEX verifikat_foretag ON verifikat (foretag_id, id);

-- Ett verifikat rättas högst en gång.
CREATE UNIQUE INDEX verifikat_rattar ON verifikat (rattar) WHERE rattar IS NOT NULL;

CREATE INDEX verifikat_kopia_av ON verifikat (kopia_av) WHERE kopia_av IS NOT NULL;

-- Som i 20261018140000_verifikat.sql.
CREATE TRIGGER verifikat_skapas_obokfort
BEFORE INSERT ON verifikat
WHEN NEW.bokford IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'ett verifikat skapas som utkast och bokförs sedan');
END;

-- Som i 20261018145000_verifikatutkast.sql.
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
    SELECT RAISE(ABORT, 'rättelsen är daterad före verifikatet som den rättar')
    WHERE NEW.rattar IS NOT NULL
      AND NEW.datum < (SELECT datum FROM verifikat WHERE id = NEW.rattar);
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

-- En rättelse rättar ett bokfört verifikat i samma företag som inte självt är en rättelse.
CREATE TRIGGER verifikat_rattelse_skapas
BEFORE INSERT ON verifikat
WHEN NEW.rattar IS NOT NULL OR NEW.kopia_av IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'en rättelse rättar ett bokfört verifikat i samma företag')
    WHERE NEW.rattar IS NOT NULL AND NOT EXISTS (
        SELECT 1 FROM verifikat
        WHERE id = NEW.rattar AND foretag_id = NEW.foretag_id AND bokford IS NOT NULL
    );
    SELECT RAISE(ABORT, 'en rättelse kan inte rättas')
    WHERE NEW.rattar IS NOT NULL
      AND (SELECT kalla FROM verifikat WHERE id = NEW.rattar) = 'rattelse';
    SELECT RAISE(ABORT, 'ett utkast är en kopia av ett bokfört verifikat i samma företag')
    WHERE NEW.kopia_av IS NOT NULL AND NOT EXISTS (
        SELECT 1 FROM verifikat
        WHERE id = NEW.kopia_av AND foretag_id = NEW.foretag_id AND bokford IS NOT NULL
    );
END;

-- Källan och länkarna sätts när verifikatet skapas och ändras inte.
CREATE TRIGGER verifikat_lankar_andras_inte
BEFORE UPDATE OF kalla, rattar, kopia_av ON verifikat
WHEN OLD.kalla IS NOT NEW.kalla OR OLD.rattar IS NOT NEW.rattar
    OR OLD.kopia_av IS NOT NEW.kopia_av
BEGIN
    SELECT RAISE(ABORT, 'ett verifikats källa och länkar går inte att ändra');
END;

-- Inga främmande nycklar bröts under bytet; annars avbryts migreringen här.
CREATE TEMP TABLE rattelse_kontroll (brott INTEGER NOT NULL CHECK (brott = 0));
INSERT INTO rattelse_kontroll SELECT count(*) FROM pragma_foreign_key_check;
DROP TABLE rattelse_kontroll;

COMMIT;

PRAGMA legacy_alter_table = OFF;
PRAGMA foreign_keys = ON;
