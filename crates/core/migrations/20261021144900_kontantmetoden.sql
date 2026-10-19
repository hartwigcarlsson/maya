-- Kontantmetoden för fakturor (etapp 2, #149): varje faktura vet om dess fordran eller skuld
-- är bokförd på 1510 eller 2440.
--
-- En rad i `bokford_reskontrapost` säger att verifikatet bokförde fakturans fordran (1510) eller
-- skuld (2440): utfärdandet eller registreringen under fakturametoden, det nya verifikatet från
-- Rätta kontering, och under kontantmetoden T2-verifikatet (#150). Fordran eller skulden är
-- bokförd så länge ett sådant verifikat inte är vänt med en rättelse (vyn
-- `reskontrapost_bokford`); Rätta registrering och Rätta avskrift vänder registreringen, och
-- fakturan registreras om under den bokföringsmetod som gäller på fakturadatumet. Under
-- kontantmetoden bokförs inget när fakturan utfärdas eller registreras, och fakturan får ingen
-- rad förrän T2. Raderna ändras och raderas aldrig (L1).
--
-- Fakturor som redan är bokförda (alla, eftersom kontantmetoden spärrades före #149) får sina
-- rader här. Inga verifikat ändras.

CREATE TABLE bokford_reskontrapost (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    foretag_id INTEGER NOT NULL REFERENCES foretag (id),
    kundfaktura_id INTEGER REFERENCES kundfaktura (id),
    leverantorsfaktura_id INTEGER REFERENCES leverantorsfaktura (id),
    -- Verifikatet som bokförde fordran eller skulden.
    verifikat_id INTEGER NOT NULL REFERENCES verifikat (id),
    CHECK ((kundfaktura_id IS NULL) <> (leverantorsfaktura_id IS NULL)),
    UNIQUE (kundfaktura_id, verifikat_id),
    UNIQUE (leverantorsfaktura_id, verifikat_id)
) STRICT;

CREATE INDEX bokford_reskontrapost_foretag ON bokford_reskontrapost (foretag_id);
CREATE INDEX bokford_reskontrapost_verifikat ON bokford_reskontrapost (verifikat_id);

-- Fakturan är utfärdad eller registrerad i företaget, och verifikatet är bokfört i det.
CREATE TRIGGER bokford_reskontrapost_kraver_faktura_och_verifikat
BEFORE INSERT ON bokford_reskontrapost
WHEN NOT EXISTS (
        SELECT 1 FROM verifikat
        WHERE id = NEW.verifikat_id AND foretag_id = NEW.foretag_id AND bokford IS NOT NULL)
  OR (NEW.kundfaktura_id IS NOT NULL AND NOT EXISTS (
        SELECT 1 FROM kundfaktura
        WHERE id = NEW.kundfaktura_id AND foretag_id = NEW.foretag_id
          AND status IN ('utfardad', 'registrerad')))
  OR (NEW.leverantorsfaktura_id IS NOT NULL AND NOT EXISTS (
        SELECT 1 FROM leverantorsfaktura
        WHERE id = NEW.leverantorsfaktura_id AND foretag_id = NEW.foretag_id
          AND status = 'registrerad'))
BEGIN
    SELECT RAISE(ABORT, 'en bokförd fordran eller skuld kräver en utfärdad eller registrerad faktura och ett bokfört verifikat');
END;

CREATE TRIGGER bokford_reskontrapost_andras_inte
BEFORE UPDATE ON bokford_reskontrapost
BEGIN
    SELECT RAISE(ABORT, 'en bokförd fordran eller skuld går inte att ändra');
END;

CREATE TRIGGER bokford_reskontrapost_raderas_inte
BEFORE DELETE ON bokford_reskontrapost
BEGIN
    SELECT RAISE(ABORT, 'en bokförd fordran eller skuld går inte att ändra');
END;

-- Fakturorna vars fordran eller skuld är bokförd nu: verifikatet som bokförde den är inte vänt.
CREATE VIEW reskontrapost_bokford AS
SELECT DISTINCT b.foretag_id, b.kundfaktura_id, b.leverantorsfaktura_id
FROM bokford_reskontrapost b
WHERE NOT EXISTS (SELECT 1 FROM verifikat r WHERE r.rattar = b.verifikat_id);

-- De bokförda fakturorna före #149.
INSERT INTO bokford_reskontrapost (foretag_id, kundfaktura_id, verifikat_id)
SELECT foretag_id, id, verifikat_id FROM kundfaktura
WHERE verifikat_id IS NOT NULL AND status IN ('utfardad', 'registrerad');

INSERT INTO bokford_reskontrapost (foretag_id, kundfaktura_id, verifikat_id)
SELECT f.foretag_id, f.id, k.verifikat_id
FROM kundfaktura_kontering k JOIN kundfaktura f ON f.id = k.faktura_id
WHERE f.status IN ('utfardad', 'registrerad');

INSERT INTO bokford_reskontrapost (foretag_id, leverantorsfaktura_id, verifikat_id)
SELECT foretag_id, id, verifikat_id FROM leverantorsfaktura
WHERE verifikat_id IS NOT NULL AND status = 'registrerad';
