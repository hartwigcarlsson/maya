-- Rätta kontering på en utfärdad kundfaktura (etapp 2, #142).
--
-- Användaren väljer ett nytt intäktskonto per rad. I samma transaktion vänds fakturans
-- gällande utfärdandeverifikat med en rättelse (R1, R2, VF7) och ett nytt utfärdandeverifikat
-- bokförs med dagens datum. Fakturan, dess rader, numret och PDF:en ändras inte (L1): de nya
-- kontona står här, en rad per rättning med båda verifikaten och orsaken, och raderna med
-- kontot per fakturarad. Den senaste rättningen är den gällande konteringen.
--
-- Raderna ändras och raderas aldrig (L1, A1). De finns bara för en utfärdad faktura, så ett
-- företag med dem kan inte raderas och de behöver ingen plats i raderingen.

CREATE TABLE kundfaktura_kontering (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    faktura_id INTEGER NOT NULL REFERENCES kundfaktura (id),
    -- Rättelsen som vände det föregående utfärdandeverifikatet.
    rattelse_id INTEGER NOT NULL UNIQUE REFERENCES verifikat (id),
    -- Det nya utfärdandeverifikatet.
    verifikat_id INTEGER NOT NULL UNIQUE REFERENCES verifikat (id),
    orsak TEXT NOT NULL,
    rattad TEXT NOT NULL,
    rattad_kanal TEXT NOT NULL CHECK (rattad_kanal IN ('webb', 'cli', 'maya')),
    rattad_av INTEGER REFERENCES anvandare (id)
) STRICT;

CREATE INDEX kundfaktura_kontering_faktura ON kundfaktura_kontering (faktura_id, id);

CREATE TABLE kundfaktura_konteringsrad (
    kontering_id INTEGER NOT NULL REFERENCES kundfaktura_kontering (id),
    radnr INTEGER NOT NULL CHECK (radnr >= 1),
    konto INTEGER NOT NULL,
    PRIMARY KEY (kontering_id, radnr)
) STRICT;

-- Bara en utfärdad faktura kan få en ny kontering.
CREATE TRIGGER kundfaktura_kontering_kraver_utfardad
BEFORE INSERT ON kundfaktura_kontering
WHEN (SELECT status FROM kundfaktura WHERE id = NEW.faktura_id) IS NOT 'utfardad'
BEGIN
    SELECT RAISE(ABORT, 'bara en utfärdad kundfaktura kan få en ny kontering');
END;

CREATE TRIGGER kundfaktura_kontering_andras_inte
BEFORE UPDATE ON kundfaktura_kontering
BEGIN
    SELECT RAISE(ABORT, 'en rättad kontering går inte att ändra');
END;

CREATE TRIGGER kundfaktura_kontering_raderas_inte
BEFORE DELETE ON kundfaktura_kontering
BEGIN
    SELECT RAISE(ABORT, 'en rättad kontering går inte att ändra');
END;

-- Raderna skrivs i samma transaktion som konteringen, innan något annat har hänvisat till
-- den: en rad läggs bara till den senaste konteringen, och bara för en fakturarad som finns
-- och som konteringen inte redan har.
CREATE TRIGGER kundfaktura_konteringsrad_laggs_inte_till
BEFORE INSERT ON kundfaktura_konteringsrad
WHEN NEW.kontering_id IS NOT (SELECT max(id) FROM kundfaktura_kontering)
  OR NOT EXISTS (
      SELECT 1 FROM kundfakturarad r JOIN kundfaktura_kontering k ON k.faktura_id = r.faktura_id
      WHERE k.id = NEW.kontering_id AND r.radnr = NEW.radnr)
  OR EXISTS (
      SELECT 1 FROM behandlingshistorik
      WHERE typ = 'kontering rättad'
        AND json_extract(beskrivning, '$.kontering') = NEW.kontering_id)
BEGIN
    SELECT RAISE(ABORT, 'en rättad kontering går inte att ändra');
END;

CREATE TRIGGER kundfaktura_konteringsrad_andras_inte
BEFORE UPDATE ON kundfaktura_konteringsrad
BEGIN
    SELECT RAISE(ABORT, 'en rättad kontering går inte att ändra');
END;

CREATE TRIGGER kundfaktura_konteringsrad_raderas_inte
BEFORE DELETE ON kundfaktura_konteringsrad
BEGIN
    SELECT RAISE(ABORT, 'en rättad kontering går inte att ändra');
END;
