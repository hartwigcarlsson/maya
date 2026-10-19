-- no-transaction
-- Kreditfaktura från leverantör och Rätta avskrift (etapp 2, #148).
--
-- En kreditfaktura från leverantören är en leverantörsfaktura med negativt totalbelopp och
-- negativa rader, med en frivillig hänvisning (`krediterar`) till originalet: en registrerad
-- faktura från samma leverantör som inte själv är en kreditfaktura. SQLite kan inte ändra en
-- CHECK, så `leverantorsfaktura` och `leverantorsfakturarad` byggs om utan kraven på belopp som
-- inte är negativa, med mallen från 20261018181100_rattelse.sql. Triggrarna och indexen på
-- tabellerna försvinner med dem och skapas om här.
--
-- Rätta avskrift vänder registreringens verifikat med en rättelse (R1) och låser upp fakturan:
-- status `upplast`. Upplåsningen står i `leverantorsfaktura_avskrift`, som aldrig ändras (R2).
-- En registrerad faktura ändras fortfarande aldrig (L1), utom att den blir upplåst när en
-- avskriftsrättelse av just dess registrering finns. En upplåst faktura kan skrivas av igen, men
-- originalet, företaget och vem som skapade den ändras inte, och den raderas inte; den
-- registreras om med ett nytt verifikat som inte är rättat. Betalningarna ligger kvar.
--
-- Det som återstår av en leverantörsfaktura räknas på ett ställe, i vyn
-- `leverantorsfaktura_i_reskontran`: totalen minus de registrerade kreditfakturorna som
-- hänvisar till den och betalningarna i reskontran (med öresutjämning och kursdifferens). En
-- kreditfaktura med hänvisning räknas av mot originalet och har inget kvar själv; en utan
-- hänvisning har sin negativa total kvar, ett tillgodohavande som kan återbetalas.

PRAGMA foreign_keys = OFF;
PRAGMA legacy_alter_table = ON;

BEGIN IMMEDIATE;

CREATE TABLE leverantorsfaktura_ny (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    foretag_id INTEGER NOT NULL REFERENCES foretag (id),
    status TEXT NOT NULL CHECK (status IN ('utkast', 'registrerad', 'upplast')),
    -- Originalet, ett underlag i företaget.
    underlag_id INTEGER NOT NULL REFERENCES underlag (id),
    -- En leverantör i registret, eller en *ny* leverantör som hör till utkastet.
    leverantor_id INTEGER REFERENCES leverantor (id),
    fakturanummer TEXT NOT NULL DEFAULT '',
    fakturadatum TEXT,
    forfallodag TEXT,
    -- OCR eller meddelande.
    referens TEXT NOT NULL DEFAULT '',
    -- I öre; negativt för en kreditfaktura.
    total INTEGER,
    -- Originalvaluta och originalbelopp i fritext (L6: fakturan registreras i SEK).
    originalvaluta TEXT NOT NULL DEFAULT '',
    -- Registreringens verifikat (fakturametoden); efter Rätta avskrift det senaste.
    verifikat_id INTEGER REFERENCES verifikat (id),
    skapad TEXT NOT NULL,
    skapad_kanal TEXT NOT NULL CHECK (skapad_kanal IN ('webb', 'cli', 'maya')),
    skapad_av INTEGER REFERENCES anvandare (id),
    -- Den senaste registreringen.
    registrerad TEXT,
    registrerad_kanal TEXT CHECK (registrerad_kanal IN ('webb', 'cli', 'maya')),
    registrerad_av INTEGER REFERENCES anvandare (id),
    omvand_skattskyldighet TEXT
        CHECK (omvand_skattskyldighet IN ('ingen', 'eu_tjanst', 'eu_vara', 'bygg', 'utanfor_eu')),
    -- Originalet som en kreditfaktura krediterar.
    krediterar INTEGER REFERENCES leverantorsfaktura (id),
    CHECK (status <> 'registrerad' OR (
        registrerad IS NOT NULL AND registrerad_kanal IS NOT NULL
        AND leverantor_id IS NOT NULL AND fakturanummer <> ''
        AND fakturadatum IS NOT NULL AND forfallodag IS NOT NULL AND total IS NOT NULL)),
    -- En upplåst faktura har registrerats en gång.
    CHECK (status <> 'upplast' OR (registrerad IS NOT NULL AND registrerad_kanal IS NOT NULL)),
    -- En registrerad kreditfaktura med hänvisning har negativ total.
    CHECK (status <> 'registrerad' OR krediterar IS NULL OR total < 0),
    CHECK (krediterar IS NOT id)
) STRICT;

INSERT INTO leverantorsfaktura_ny
    (id, foretag_id, status, underlag_id, leverantor_id, fakturanummer, fakturadatum,
     forfallodag, referens, total, originalvaluta, verifikat_id, skapad, skapad_kanal,
     skapad_av, registrerad, registrerad_kanal, registrerad_av, omvand_skattskyldighet)
SELECT id, foretag_id, status, underlag_id, leverantor_id, fakturanummer, fakturadatum,
       forfallodag, referens, total, originalvaluta, verifikat_id, skapad, skapad_kanal,
       skapad_av, registrerad, registrerad_kanal, registrerad_av, omvand_skattskyldighet
FROM leverantorsfaktura;

CREATE TABLE leverantorsfakturarad_ny (
    faktura_id INTEGER NOT NULL REFERENCES leverantorsfaktura (id),
    radnr INTEGER NOT NULL CHECK (radnr >= 1),
    konto INTEGER,
    -- I öre; negativt på en kreditfaktura.
    netto INTEGER,
    momssats INTEGER NOT NULL CHECK (momssats IN (0, 6, 12, 25)),
    -- NULL i ett utkast: räknas ur nettot och momssatsen.
    moms INTEGER,
    PRIMARY KEY (faktura_id, radnr)
) STRICT;

INSERT INTO leverantorsfakturarad_ny (faktura_id, radnr, konto, netto, momssats, moms)
SELECT faktura_id, radnr, konto, netto, momssats, moms FROM leverantorsfakturarad;

DROP TABLE leverantorsfakturarad;
DROP TABLE leverantorsfaktura;

ALTER TABLE leverantorsfaktura_ny RENAME TO leverantorsfaktura;
ALTER TABLE leverantorsfakturarad_ny RENAME TO leverantorsfakturarad;

-- Som i 20261020014500_leverantorsfaktura.sql.
CREATE INDEX leverantorsfaktura_foretag ON leverantorsfaktura (foretag_id, status);
CREATE INDEX leverantorsfaktura_verifikat ON leverantorsfaktura (verifikat_id);
CREATE INDEX leverantorsfaktura_leverantor ON leverantorsfaktura (leverantor_id);
CREATE INDEX leverantorsfaktura_krediterar ON leverantorsfaktura (krediterar)
WHERE krediterar IS NOT NULL;

-- Skyddsnätet för dubblettregeln: samma leverantör och fakturanummer registreras en gång.
CREATE UNIQUE INDEX leverantorsfaktura_nummer
ON leverantorsfaktura (foretag_id, leverantor_id, lower(fakturanummer))
WHERE status <> 'utkast';

-- Rätta avskrift: upplåsningen av en registrering, med rättelsen av registreringens verifikat
-- (R1). `registrering` är tiden för registreringen som låstes upp. Verifikatet och rättelsen är
-- båda satta, eller båda tomma för en faktura vars skuld inte var bokförd (kontantmetoden, #149).
CREATE TABLE leverantorsfaktura_avskrift (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    foretag_id INTEGER NOT NULL REFERENCES foretag (id),
    faktura_id INTEGER NOT NULL REFERENCES leverantorsfaktura (id),
    registrering TEXT NOT NULL,
    -- Registreringens verifikat, som rättelsen vänder.
    verifikat_id INTEGER REFERENCES verifikat (id),
    rattelse_id INTEGER UNIQUE REFERENCES verifikat (id),
    orsak TEXT NOT NULL DEFAULT '',
    rattad TEXT NOT NULL,
    rattad_kanal TEXT NOT NULL CHECK (rattad_kanal IN ('webb', 'cli', 'maya')),
    rattad_av INTEGER REFERENCES anvandare (id),
    CHECK ((verifikat_id IS NULL) = (rattelse_id IS NULL))
) STRICT;

CREATE INDEX leverantorsfaktura_avskrift_foretag ON leverantorsfaktura_avskrift (foretag_id);
CREATE INDEX leverantorsfaktura_avskrift_verifikat ON leverantorsfaktura_avskrift (verifikat_id);

-- Upplåsningen gäller fakturans registrering som den är nu, och rättelsen är en bokförd rättelse
-- av just registreringens verifikat.
CREATE TRIGGER leverantorsfaktura_avskrift_kraver_registreringen
BEFORE INSERT ON leverantorsfaktura_avskrift
WHEN NOT EXISTS (
        SELECT 1 FROM leverantorsfaktura
        WHERE id = NEW.faktura_id AND foretag_id = NEW.foretag_id AND status = 'registrerad'
          AND registrerad = NEW.registrering AND verifikat_id IS NEW.verifikat_id)
  OR (NEW.rattelse_id IS NOT NULL AND NOT EXISTS (
        SELECT 1 FROM verifikat
        WHERE id = NEW.rattelse_id AND rattar = NEW.verifikat_id AND bokford IS NOT NULL))
BEGIN
    SELECT RAISE(ABORT, 'rätta avskrift kräver en registrerad faktura och en bokförd rättelse av registreringen');
END;

CREATE TRIGGER leverantorsfaktura_avskrift_andras_inte
BEFORE UPDATE ON leverantorsfaktura_avskrift
BEGIN
    SELECT RAISE(ABORT, 'en rättad avskrift går inte att ändra');
END;

CREATE TRIGGER leverantorsfaktura_avskrift_raderas_inte
BEFORE DELETE ON leverantorsfaktura_avskrift
BEGIN
    SELECT RAISE(ABORT, 'en rättad avskrift går inte att ändra');
END;

-- En registrerad faktura ändras aldrig (L1), utom att den blir upplåst, med allt annat som det
-- var, när en avskriftsrättelse av just den här registreringen finns.
CREATE TRIGGER leverantorsfaktura_registrerad_andras_inte
BEFORE UPDATE ON leverantorsfaktura
WHEN OLD.status = 'registrerad' AND NOT (
    NEW.status = 'upplast'
    AND NEW.id = OLD.id AND NEW.foretag_id = OLD.foretag_id
    AND NEW.underlag_id = OLD.underlag_id AND NEW.leverantor_id IS OLD.leverantor_id
    AND NEW.fakturanummer = OLD.fakturanummer AND NEW.fakturadatum IS OLD.fakturadatum
    AND NEW.forfallodag IS OLD.forfallodag AND NEW.referens = OLD.referens
    AND NEW.total IS OLD.total AND NEW.originalvaluta = OLD.originalvaluta
    AND NEW.verifikat_id IS OLD.verifikat_id AND NEW.skapad = OLD.skapad
    AND NEW.skapad_kanal = OLD.skapad_kanal AND NEW.skapad_av IS OLD.skapad_av
    AND NEW.registrerad IS OLD.registrerad AND NEW.registrerad_kanal IS OLD.registrerad_kanal
    AND NEW.registrerad_av IS OLD.registrerad_av
    AND NEW.omvand_skattskyldighet IS OLD.omvand_skattskyldighet
    AND NEW.krediterar IS OLD.krediterar
    AND EXISTS (SELECT 1 FROM leverantorsfaktura_avskrift
                WHERE faktura_id = OLD.id AND registrering = OLD.registrerad
                  AND verifikat_id IS OLD.verifikat_id))
BEGIN
    SELECT RAISE(ABORT, 'en registrerad leverantörsfaktura går inte att ändra');
END;

-- En upplåst faktura behåller sitt original, sitt företag och sin skapare, blir inte ett utkast
-- igen och behåller registreringen tills den registreras om. Att registrera om ger en ny
-- registrering med ett verifikat som inte redan är rättat.
CREATE TRIGGER leverantorsfaktura_upplast_andras_bara_i_avskriften
BEFORE UPDATE ON leverantorsfaktura
WHEN OLD.status = 'upplast' AND (
    NEW.id <> OLD.id OR NEW.foretag_id <> OLD.foretag_id OR NEW.underlag_id <> OLD.underlag_id
    OR NEW.skapad <> OLD.skapad OR NEW.skapad_kanal <> OLD.skapad_kanal
    OR NEW.skapad_av IS NOT OLD.skapad_av
    OR NEW.status = 'utkast'
    OR (NEW.status = 'upplast' AND (
        NEW.verifikat_id IS NOT OLD.verifikat_id OR NEW.registrerad IS NOT OLD.registrerad
        OR NEW.registrerad_kanal IS NOT OLD.registrerad_kanal
        OR NEW.registrerad_av IS NOT OLD.registrerad_av))
    OR (NEW.status = 'registrerad' AND NEW.verifikat_id IS NOT NULL AND (
        EXISTS (SELECT 1 FROM verifikat WHERE rattar = NEW.verifikat_id)
        OR NOT EXISTS (SELECT 1 FROM verifikat
                       WHERE id = NEW.verifikat_id AND foretag_id = OLD.foretag_id
                         AND bokford IS NOT NULL))))
BEGIN
    SELECT RAISE(ABORT, 'en upplåst leverantörsfaktura ändras bara i avskriften');
END;

-- En kreditfaktura som registreras hänvisar till en registrerad faktura från samma leverantör
-- i samma företag, som inte själv är en kreditfaktura; en faktura som andra krediterar blir
-- ingen kreditfaktura.
CREATE TRIGGER leverantorsfaktura_kredit_kraver_originalet
BEFORE UPDATE ON leverantorsfaktura
WHEN NEW.status = 'registrerad' AND OLD.status <> 'registrerad' AND NEW.krediterar IS NOT NULL
  AND (NOT EXISTS (
        SELECT 1 FROM leverantorsfaktura o
        WHERE o.id = NEW.krediterar AND o.foretag_id = NEW.foretag_id
          AND o.leverantor_id = NEW.leverantor_id AND o.status = 'registrerad'
          AND o.krediterar IS NULL)
    OR EXISTS (SELECT 1 FROM leverantorsfaktura k WHERE k.krediterar = NEW.id))
BEGIN
    SELECT RAISE(ABORT, 'en kreditfaktura hänvisar till en registrerad faktura från samma leverantör');
END;

-- En registrerad faktura raderas aldrig (L1, A1), inte heller en upplåst.
CREATE TRIGGER leverantorsfaktura_registrerad_raderas_inte
BEFORE DELETE ON leverantorsfaktura
WHEN OLD.status <> 'utkast'
BEGIN
    SELECT RAISE(ABORT, 'en registrerad leverantörsfaktura går inte att radera');
END;

-- Raderna på en registrerad faktura ändras inte; på ett utkast och en upplåst faktura skrivs de
-- av fritt.
CREATE TRIGGER leverantorsfakturarad_registrerad_andras_inte
BEFORE UPDATE ON leverantorsfakturarad
WHEN (SELECT status FROM leverantorsfaktura WHERE id = OLD.faktura_id) = 'registrerad'
  OR (SELECT status FROM leverantorsfaktura WHERE id = NEW.faktura_id) = 'registrerad'
BEGIN
    SELECT RAISE(ABORT, 'en registrerad leverantörsfaktura går inte att ändra');
END;

CREATE TRIGGER leverantorsfakturarad_registrerad_raderas_inte
BEFORE DELETE ON leverantorsfakturarad
WHEN (SELECT status FROM leverantorsfaktura WHERE id = OLD.faktura_id) = 'registrerad'
BEGIN
    SELECT RAISE(ABORT, 'en registrerad leverantörsfaktura går inte att ändra');
END;

CREATE TRIGGER leverantorsfakturarad_registrerad_laggs_inte_till
BEFORE INSERT ON leverantorsfakturarad
WHEN (SELECT status FROM leverantorsfaktura WHERE id = NEW.faktura_id) = 'registrerad'
BEGIN
    SELECT RAISE(ABORT, 'en registrerad leverantörsfaktura går inte att ändra');
END;

-- Nya betalningar gäller en registrerad faktura som inte är en kreditfaktura med hänvisning:
-- inte en upplåst faktura, och inte en kreditfaktura som räknas av mot sitt original.
CREATE TRIGGER betalning_kraver_registrerad_leverantorsfaktura
BEFORE INSERT ON betalning
WHEN NEW.leverantorsfaktura_id IS NOT NULL AND NOT EXISTS (
    SELECT 1 FROM leverantorsfaktura
    WHERE id = NEW.leverantorsfaktura_id AND status = 'registrerad' AND krediterar IS NULL)
BEGIN
    SELECT RAISE(ABORT, 'en betalning kräver en registrerad leverantörsfaktura');
END;

-- Det som återstår av varje leverantörsfaktura som inte är ett utkast, i öre: totalen minus det
-- som registrerade kreditfakturor har krediterat och det som är betalt (med öresutjämning och
-- kursdifferens). Negativt är ett tillgodohavande. En kreditfaktura med hänvisning har 0 kvar.
CREATE VIEW leverantorsfaktura_i_reskontran AS
SELECT f.id, f.foretag_id, f.status, f.total,
       coalesce((SELECT -sum(k.total) FROM leverantorsfaktura k
                 WHERE k.krediterar = f.id AND k.status = 'registrerad'), 0) AS krediterat,
       coalesce((SELECT sum(b.belopp + b.oresutjamning + b.kursdifferens)
                 FROM betalning_i_reskontran b WHERE b.leverantorsfaktura_id = f.id), 0)
           AS betalt,
       CASE WHEN f.krediterar IS NOT NULL THEN 0
            ELSE f.total
                 - coalesce((SELECT -sum(k.total) FROM leverantorsfaktura k
                             WHERE k.krediterar = f.id AND k.status = 'registrerad'), 0)
                 - coalesce((SELECT sum(b.belopp + b.oresutjamning + b.kursdifferens)
                             FROM betalning_i_reskontran b WHERE b.leverantorsfaktura_id = f.id), 0)
       END AS aterstar
FROM leverantorsfaktura f
WHERE f.status <> 'utkast';

-- Inga främmande nycklar bröts under bytet; annars avbryts migreringen här.
CREATE TEMP TABLE leverantorskredit_kontroll (brott INTEGER NOT NULL CHECK (brott = 0));
INSERT INTO leverantorskredit_kontroll SELECT count(*) FROM pragma_foreign_key_check;
DROP TABLE leverantorskredit_kontroll;

COMMIT;

PRAGMA legacy_alter_table = OFF;
PRAGMA foreign_keys = ON;
