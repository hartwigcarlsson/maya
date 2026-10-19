-- T2 under kontantmetoden (etapp 2, #150): obetalda fordringar och skulder bokförs vid
-- räkenskapsårets slut (T2).
--
-- Dagen efter räkenskapsårets slut skapar Maya ett T2-utkast: ett verifikatutkast daterat på
-- årets sista dag med det obetalda beloppet vid årets slut på varje kund- och
-- leverantörsfaktura med fakturadatum i året vars fordran eller skuld inte är bokförd. En rad i
-- `t2` säger att verifikatet är årets T2-verifikat. Maya räknar om utkastet tills en användare
-- bokför det; det kan inte ändras eller raderas för hand (`core`), och blir det tomt raderar
-- Maya det. När det bokförs får varje faktura i det en rad i `bokford_reskontrapost` (#149).
-- Ett bokfört T2-verifikat vänds aldrig (L1, R1): ingen rättelse får hänvisa till det.

CREATE TABLE t2 (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    foretag_id INTEGER NOT NULL REFERENCES foretag (id),
    -- Ett T2-verifikat per räkenskapsår.
    rakenskapsar_id INTEGER NOT NULL UNIQUE REFERENCES rakenskapsar (id),
    verifikat_id INTEGER NOT NULL UNIQUE REFERENCES verifikat (id)
) STRICT;

CREATE INDEX t2_foretag ON t2 (foretag_id);

-- T2-verifikatet är ett utkast i företaget när det blir årets T2, och året är företagets.
CREATE TRIGGER t2_kraver_utkast_i_aret
BEFORE INSERT ON t2
WHEN NOT EXISTS (
        SELECT 1 FROM verifikat
        WHERE id = NEW.verifikat_id AND foretag_id = NEW.foretag_id AND bokford IS NULL
          AND kalla = 'manuell' AND rattar IS NULL)
  OR NOT EXISTS (
        SELECT 1 FROM rakenskapsar WHERE id = NEW.rakenskapsar_id AND foretag_id = NEW.foretag_id)
BEGIN
    SELECT RAISE(ABORT, 'ett T2-verifikat kräver ett utkast och ett räkenskapsår i företaget');
END;

CREATE TRIGGER t2_andras_inte
BEFORE UPDATE ON t2
BEGIN
    SELECT RAISE(ABORT, 'ett T2-verifikat går inte att ändra');
END;

-- Bara ett T2-utkast som har blivit tomt raderas; ett bokfört T2-verifikat finns kvar.
CREATE TRIGGER t2_bokfort_raderas_inte
BEFORE DELETE ON t2
WHEN (SELECT bokford FROM verifikat WHERE id = OLD.verifikat_id) IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'ett bokfört T2-verifikat går inte att radera');
END;

-- T2-verifikatet vänds inte i det nya året: ingen rättelse hänvisar till det.
CREATE TRIGGER t2_vands_inte
BEFORE INSERT ON verifikat
WHEN NEW.rattar IS NOT NULL AND EXISTS (SELECT 1 FROM t2 WHERE verifikat_id = NEW.rattar)
BEGIN
    SELECT RAISE(ABORT, 'T2-verifikatet vänds inte');
END;
