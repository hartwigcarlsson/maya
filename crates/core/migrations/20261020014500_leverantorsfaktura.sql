-- Leverantörsfakturor (etapp 2, #145): utkast och registrerade fakturor i leverantörsreskontran.
--
-- Ett utkast har originalet (ett underlag) kopplat och kan ändras och raderas fritt. Att
-- registrera låser fakturan och bokför registreringens verifikat i samma transaktion under
-- fakturametoden, med originalet som underlag (VF5). En registrerad faktura ändras aldrig och
-- raderas aldrig (L1, A1); schemat avvisar det. Status `upplast` (Rätta avskrift, #148) finns
-- redan i CHECK så att tabellen inte behöver byggas om.
--
-- Belopp i öre. Ett utkast får sakna det som inte är ifyllt; en registrerad faktura har allt.
-- Id:n återanvänds aldrig, eftersom historiken pekar på dem.

CREATE TABLE leverantorsfaktura (
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
    total INTEGER CHECK (total >= 0),
    -- Originalvaluta och originalbelopp i fritext (L6: fakturan registreras i SEK).
    originalvaluta TEXT NOT NULL DEFAULT '',
    -- Registreringens verifikat (fakturametoden).
    verifikat_id INTEGER REFERENCES verifikat (id),
    skapad TEXT NOT NULL,
    skapad_kanal TEXT NOT NULL CHECK (skapad_kanal IN ('webb', 'cli', 'maya')),
    skapad_av INTEGER REFERENCES anvandare (id),
    registrerad TEXT,
    registrerad_kanal TEXT CHECK (registrerad_kanal IN ('webb', 'cli', 'maya')),
    registrerad_av INTEGER REFERENCES anvandare (id),
    CHECK (status = 'utkast' OR (
        registrerad IS NOT NULL AND registrerad_kanal IS NOT NULL
        AND leverantor_id IS NOT NULL AND fakturanummer <> ''
        AND fakturadatum IS NOT NULL AND forfallodag IS NOT NULL AND total IS NOT NULL))
) STRICT;

CREATE INDEX leverantorsfaktura_foretag ON leverantorsfaktura (foretag_id, status);
CREATE INDEX leverantorsfaktura_verifikat ON leverantorsfaktura (verifikat_id);
CREATE INDEX leverantorsfaktura_leverantor ON leverantorsfaktura (leverantor_id);

-- Skyddsnätet för dubblettregeln: samma leverantör och fakturanummer registreras en gång.
CREATE UNIQUE INDEX leverantorsfaktura_nummer
ON leverantorsfaktura (foretag_id, leverantor_id, lower(fakturanummer))
WHERE status <> 'utkast';

CREATE TABLE leverantorsfakturarad (
    faktura_id INTEGER NOT NULL REFERENCES leverantorsfaktura (id),
    radnr INTEGER NOT NULL CHECK (radnr >= 1),
    konto INTEGER,
    netto INTEGER CHECK (netto >= 0),
    momssats INTEGER NOT NULL CHECK (momssats IN (0, 6, 12, 25)),
    -- NULL i ett utkast: räknas ur nettot och momssatsen.
    moms INTEGER CHECK (moms >= 0),
    PRIMARY KEY (faktura_id, radnr)
) STRICT;

-- En registrerad faktura ändras och raderas aldrig (L1, A1).
CREATE TRIGGER leverantorsfaktura_registrerad_andras_inte
BEFORE UPDATE ON leverantorsfaktura
WHEN OLD.status <> 'utkast'
BEGIN
    SELECT RAISE(ABORT, 'en registrerad leverantörsfaktura går inte att ändra');
END;

CREATE TRIGGER leverantorsfaktura_registrerad_raderas_inte
BEFORE DELETE ON leverantorsfaktura
WHEN OLD.status <> 'utkast'
BEGIN
    SELECT RAISE(ABORT, 'en registrerad leverantörsfaktura går inte att radera');
END;

CREATE TRIGGER leverantorsfakturarad_registrerad_andras_inte
BEFORE UPDATE ON leverantorsfakturarad
WHEN (SELECT status FROM leverantorsfaktura WHERE id = OLD.faktura_id) <> 'utkast'
  OR (SELECT status FROM leverantorsfaktura WHERE id = NEW.faktura_id) <> 'utkast'
BEGIN
    SELECT RAISE(ABORT, 'en registrerad leverantörsfaktura går inte att ändra');
END;

CREATE TRIGGER leverantorsfakturarad_registrerad_raderas_inte
BEFORE DELETE ON leverantorsfakturarad
WHEN (SELECT status FROM leverantorsfaktura WHERE id = OLD.faktura_id) <> 'utkast'
BEGIN
    SELECT RAISE(ABORT, 'en registrerad leverantörsfaktura går inte att ändra');
END;

CREATE TRIGGER leverantorsfakturarad_registrerad_laggs_inte_till
BEFORE INSERT ON leverantorsfakturarad
WHEN (SELECT status FROM leverantorsfaktura WHERE id = NEW.faktura_id) <> 'utkast'
BEGIN
    SELECT RAISE(ABORT, 'en registrerad leverantörsfaktura går inte att ändra');
END;

-- En registrerad leverantörsfaktura är räkenskapsinformation: ett företag med en sådan raderas
-- inte (A1). Triggrarna från 20261019100000_radering_av_foretag.sql byggs om med det villkoret.
DROP TRIGGER foretag_radering_kraver_utan_rakenskapsinformation;

CREATE TRIGGER foretag_radering_kraver_utan_rakenskapsinformation
BEFORE INSERT ON foretag_radering
WHEN EXISTS (SELECT 1 FROM verifikat WHERE foretag_id = NEW.foretag_id AND bokford IS NOT NULL)
  OR EXISTS (SELECT 1 FROM underlag WHERE foretag_id = NEW.foretag_id)
  OR EXISTS (SELECT 1 FROM ingaende_balans b JOIN rakenskapsar r ON r.id = b.rakenskapsar_id
             WHERE r.foretag_id = NEW.foretag_id)
  OR EXISTS (SELECT 1 FROM leverantorsfaktura
             WHERE foretag_id = NEW.foretag_id AND status <> 'utkast')
BEGIN
    SELECT RAISE(ABORT, 'ett företag med räkenskapsinformation går inte att radera');
END;

DROP TRIGGER foretag_raderas_inte;

CREATE TRIGGER foretag_raderas_inte
BEFORE DELETE ON foretag
WHEN NOT EXISTS (SELECT 1 FROM foretag_radering WHERE foretag_id = OLD.id)
  OR EXISTS (SELECT 1 FROM verifikat WHERE foretag_id = OLD.id AND bokford IS NOT NULL)
  OR EXISTS (SELECT 1 FROM underlag WHERE foretag_id = OLD.id)
  OR EXISTS (SELECT 1 FROM ingaende_balans b JOIN rakenskapsar r ON r.id = b.rakenskapsar_id
             WHERE r.foretag_id = OLD.id)
  OR EXISTS (SELECT 1 FROM leverantorsfaktura WHERE foretag_id = OLD.id AND status <> 'utkast')
BEGIN
    SELECT RAISE(ABORT, 'ett företag raderas bara utan räkenskapsinformation, genom Maya');
END;
