-- Kontoplanens regler i schemat: ett konto raderas aldrig, och numret, företaget, ursprunget och
-- det som kom från BAS ändras aldrig. Bara namnet och aktiv kan ändras.

-- 1 för ett konto som en ny BAS-version lade till (ADR 0007), "Nya i BAS".
ALTER TABLE konto ADD COLUMN bas_tillagg INTEGER NOT NULL DEFAULT 0 CHECK (bas_tillagg IN (0, 1));

CREATE TRIGGER konto_raderas_inte
BEFORE DELETE ON konto
BEGIN
    SELECT RAISE(ABORT, 'ett konto raderas aldrig');
END;

CREATE TRIGGER konto_nummer_andras_inte
BEFORE UPDATE OF foretag_id, nummer ON konto
BEGIN
    SELECT RAISE(ABORT, 'kontonumret går inte att ändra');
END;

CREATE TRIGGER konto_ursprung_andras_inte
BEFORE UPDATE OF ursprung, bas_namn, bas_version, bas_tillagg ON konto
BEGIN
    SELECT RAISE(ABORT, 'kontots ursprung går inte att ändra');
END;
