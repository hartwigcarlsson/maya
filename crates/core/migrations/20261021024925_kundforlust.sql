-- Kundförlust och återkallelse (etapp 2, #141): fakturahändelserna som skriver bort det som
-- återstår av en utfärdad kundfaktura, och som tar tillbaka förlusten.
--
-- En konstaterad kundförlust har skäl (konkurs, ackord, utmätning utan resultat, eller annat
-- med fritext) och datum och gäller det som återstår, `belopp`. När fordran är bokförd bokförs
-- förlustens verifikat i samma transaktion (L5): 6351 netto och utgående moms proportionellt
-- mot 1510. När fordran inte är bokförd (kontantmetoden, #149) blir förlusten bara en status,
-- och `verifikat_id` är NULL.
--
-- Återkalla kundförlust vänder förlustens verifikat med ett nytt verifikat, länkat till
-- förlusten genom `kundforlust_aterkallelse`, så att en betalning som ändå kommer in kan tas
-- emot. Förlusten finns kvar och visar återkallelsen, men räknas inte längre i reskontran. Högst
-- en förlust i taget gäller en faktura. Ingen av raderna ändras eller raderas (L1). Status och
-- det som återstår räknas fram och lagras inte.
--
-- Belopp i öre. Id:n återanvänds aldrig, eftersom behandlingshistoriken pekar på dem.

CREATE TABLE kundforlust (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    foretag_id INTEGER NOT NULL REFERENCES foretag (id),
    kundfaktura_id INTEGER NOT NULL REFERENCES kundfaktura (id),
    datum TEXT NOT NULL,
    skal TEXT NOT NULL CHECK (skal IN ('konkurs', 'ackord', 'utmatning', 'annat')),
    -- Beskrivningen när skälet är annat.
    annat TEXT NOT NULL DEFAULT '',
    -- Det som återstod och skrevs bort.
    belopp INTEGER NOT NULL CHECK (belopp > 0),
    -- NULL när fordran inte är bokförd: förlusten är då bara en status.
    verifikat_id INTEGER UNIQUE REFERENCES verifikat (id),
    bokford TEXT NOT NULL,
    bokford_kanal TEXT NOT NULL CHECK (bokford_kanal IN ('webb', 'cli', 'maya')),
    bokford_av INTEGER REFERENCES anvandare (id),
    CHECK ((skal = 'annat') = (trim(annat) <> ''))
) STRICT;

CREATE INDEX kundforlust_foretag ON kundforlust (foretag_id);
CREATE INDEX kundforlust_kundfaktura ON kundforlust (kundfaktura_id);

-- Återkallelsen av en kundförlust. En förlust återkallas högst en gång.
CREATE TABLE kundforlust_aterkallelse (
    kundforlust_id INTEGER PRIMARY KEY REFERENCES kundforlust (id),
    -- Verifikatet som vänder förlustens verifikat; NULL när förlusten inte har något.
    verifikat_id INTEGER UNIQUE REFERENCES verifikat (id),
    aterkallad TEXT NOT NULL,
    aterkallad_kanal TEXT NOT NULL CHECK (aterkallad_kanal IN ('webb', 'cli', 'maya')),
    aterkallad_av INTEGER REFERENCES anvandare (id)
) STRICT;

-- Kundförlusterna som räknas i reskontran: de som inte är återkallade.
CREATE VIEW kundforlust_i_reskontran AS
SELECT f.* FROM kundforlust f
WHERE NOT EXISTS (SELECT 1 FROM kundforlust_aterkallelse a WHERE a.kundforlust_id = f.id);

-- En kundförlust hör till en utfärdad kundfaktura i samma företag som ingen annan förlust
-- gäller, och dess verifikat, om det finns, är bokfört i företaget.
CREATE TRIGGER kundforlust_kraver_faktura_och_verifikat
BEFORE INSERT ON kundforlust
WHEN NOT EXISTS (
        SELECT 1 FROM kundfaktura
        WHERE id = NEW.kundfaktura_id AND foretag_id = NEW.foretag_id AND status <> 'utkast')
  OR (NEW.verifikat_id IS NOT NULL AND NOT EXISTS (
        SELECT 1 FROM verifikat
        WHERE id = NEW.verifikat_id AND foretag_id = NEW.foretag_id AND bokford IS NOT NULL))
BEGIN
    SELECT RAISE(ABORT, 'en kundförlust kräver en utfärdad kundfaktura och ett bokfört verifikat');
END;

CREATE TRIGGER kundforlust_en_i_taget
BEFORE INSERT ON kundforlust
WHEN EXISTS (SELECT 1 FROM kundforlust_i_reskontran WHERE kundfaktura_id = NEW.kundfaktura_id)
BEGIN
    SELECT RAISE(ABORT, 'en kundförlust gäller redan fakturan');
END;

CREATE TRIGGER kundforlust_andras_inte
BEFORE UPDATE ON kundforlust
BEGIN
    SELECT RAISE(ABORT, 'en kundförlust går inte att ändra');
END;

CREATE TRIGGER kundforlust_raderas_inte
BEFORE DELETE ON kundforlust
BEGIN
    SELECT RAISE(ABORT, 'en kundförlust går inte att radera');
END;

-- Återkallelsen har ett bokfört verifikat i förlustens företag precis när förlusten har ett.
CREATE TRIGGER kundforlust_aterkallelse_kraver_verifikat
BEFORE INSERT ON kundforlust_aterkallelse
WHEN (SELECT verifikat_id IS NULL FROM kundforlust WHERE id = NEW.kundforlust_id)
         IS NOT (NEW.verifikat_id IS NULL)
  OR (NEW.verifikat_id IS NOT NULL AND NOT EXISTS (
        SELECT 1 FROM verifikat v JOIN kundforlust f ON f.id = NEW.kundforlust_id
        WHERE v.id = NEW.verifikat_id AND v.foretag_id = f.foretag_id
          AND v.bokford IS NOT NULL AND v.id <> f.verifikat_id))
BEGIN
    SELECT RAISE(ABORT, 'en återkallad kundförlust kräver ett bokfört verifikat som vänder förlusten');
END;

CREATE TRIGGER kundforlust_aterkallelse_andras_inte
BEFORE UPDATE ON kundforlust_aterkallelse
BEGIN
    SELECT RAISE(ABORT, 'en återkallad kundförlust går inte att ändra');
END;

CREATE TRIGGER kundforlust_aterkallelse_raderas_inte
BEFORE DELETE ON kundforlust_aterkallelse
BEGIN
    SELECT RAISE(ABORT, 'en återkallad kundförlust går inte att ändra');
END;
