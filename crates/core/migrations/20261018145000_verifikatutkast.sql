-- Ett utkast får vara ofullständigt: en rad i ett utkast kan sakna konto. Att bokföra kräver
-- fortfarande ett aktivt konto på varje rad (triggern verifikat_bokfors), och ett bokfört
-- verifikats rader ändras aldrig (L1).
--
-- SQLite kan inte ändra en kolumns NOT NULL, så verifikatrad byggs om. Triggern
-- verifikat_bokfors läser verifikatrad och skapas om efter bytet, så att ALTER TABLE inte
-- prövar den mot en tabell som för stunden saknas. Triggers på verifikatrad försvinner med
-- tabellen och skapas om oförändrade.

DROP TRIGGER verifikat_bokfors;

CREATE TABLE verifikatrad_ny (
    verifikat_id INTEGER NOT NULL REFERENCES verifikat (id),
    -- Radens plats i verifikatet, från 1.
    radnr INTEGER NOT NULL CHECK (radnr >= 1),
    -- NULL bara i ett utkast.
    konto INTEGER CHECK (konto BETWEEN 1000 AND 8999),
    debet INTEGER NOT NULL CHECK (debet >= 0),
    kredit INTEGER NOT NULL CHECK (kredit >= 0),
    text TEXT,
    PRIMARY KEY (verifikat_id, radnr)
) STRICT;

INSERT INTO verifikatrad_ny (verifikat_id, radnr, konto, debet, kredit, text)
SELECT verifikat_id, radnr, konto, debet, kredit, text FROM verifikatrad;

DROP TABLE verifikatrad;

ALTER TABLE verifikatrad_ny RENAME TO verifikatrad;

CREATE INDEX verifikatrad_konto ON verifikatrad (konto);

-- Som i 20261018140000_verifikat.sql, och en rad utan konto stoppas av kontokontrollen.
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
