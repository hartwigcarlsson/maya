-- Betalningar av fakturor (etapp 2, #139): fakturahändelserna som minskar det som återstår i
-- kund- och leverantörsreskontran.
--
-- En betalning registreras med betaldag, belopp och betalkonto, och dess verifikat bokförs i
-- samma transaktion (L5). Betalningen hör till exakt en faktura: en kundfaktura nu och en
-- leverantörsfaktura med #147, så att tabellen inte behöver byggas om då. Beloppet har tecken:
-- en återbetalning av ett tillgodohavande (#140) är en betalning åt andra hållet. En rest som
-- stängs mot öresutjämning står i `oresutjamning` och räknas som betald.
--
-- En betalning ändras och raderas aldrig (L1). Rätta betalning vänder dess verifikat med en
-- rättelse (R1) och skriver en rad i `betalningsrattelse`; betalningen finns kvar och visar
-- rättelsen (R2), men räknas inte längre i reskontran. Status, det som återstår och förfallen
-- räknas fram och lagras inte.
--
-- Belopp i öre. Id:n återanvänds aldrig, eftersom behandlingshistoriken pekar på dem.

CREATE TABLE betalning (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    foretag_id INTEGER NOT NULL REFERENCES foretag (id),
    kundfaktura_id INTEGER REFERENCES kundfaktura (id),
    leverantorsfaktura_id INTEGER REFERENCES leverantorsfaktura (id),
    betaldag TEXT NOT NULL,
    belopp INTEGER NOT NULL CHECK (belopp <> 0),
    -- Betalkontot, ett konto i klass 1.
    konto INTEGER NOT NULL CHECK (konto BETWEEN 1000 AND 1999),
    -- Resten som stängdes mot öresutjämning (3740), högst 10 kr.
    oresutjamning INTEGER NOT NULL DEFAULT 0 CHECK (abs(oresutjamning) <= 1000),
    verifikat_id INTEGER NOT NULL REFERENCES verifikat (id),
    registrerad TEXT NOT NULL,
    registrerad_kanal TEXT NOT NULL CHECK (registrerad_kanal IN ('webb', 'cli', 'maya')),
    registrerad_av INTEGER REFERENCES anvandare (id),
    CHECK ((kundfaktura_id IS NULL) <> (leverantorsfaktura_id IS NULL))
) STRICT;

CREATE INDEX betalning_foretag ON betalning (foretag_id);
CREATE INDEX betalning_kundfaktura ON betalning (kundfaktura_id) WHERE kundfaktura_id IS NOT NULL;
CREATE INDEX betalning_leverantorsfaktura ON betalning (leverantorsfaktura_id)
WHERE leverantorsfaktura_id IS NOT NULL;
CREATE UNIQUE INDEX betalning_verifikat ON betalning (verifikat_id);

-- Rätta betalning: rättelsen av betalningens verifikat. En betalning rättas högst en gång.
CREATE TABLE betalningsrattelse (
    betalning_id INTEGER PRIMARY KEY REFERENCES betalning (id),
    -- Rättelsen, som vänder betalningens verifikat.
    verifikat_id INTEGER NOT NULL UNIQUE REFERENCES verifikat (id),
    orsak TEXT NOT NULL DEFAULT '',
    rattad TEXT NOT NULL,
    rattad_kanal TEXT NOT NULL CHECK (rattad_kanal IN ('webb', 'cli', 'maya')),
    rattad_av INTEGER REFERENCES anvandare (id)
) STRICT;

-- Betalningarna som räknas i reskontran: de som inte är rättade.
CREATE VIEW betalning_i_reskontran AS
SELECT b.* FROM betalning b
WHERE NOT EXISTS (SELECT 1 FROM betalningsrattelse r WHERE r.betalning_id = b.id);

-- En betalning hör till en utfärdad eller registrerad faktura i samma företag och har ett
-- bokfört verifikat i företaget.
CREATE TRIGGER betalning_kraver_faktura_och_verifikat
BEFORE INSERT ON betalning
WHEN (NEW.kundfaktura_id IS NOT NULL AND NOT EXISTS (
        SELECT 1 FROM kundfaktura
        WHERE id = NEW.kundfaktura_id AND foretag_id = NEW.foretag_id AND status <> 'utkast'))
  OR (NEW.leverantorsfaktura_id IS NOT NULL AND NOT EXISTS (
        SELECT 1 FROM leverantorsfaktura
        WHERE id = NEW.leverantorsfaktura_id AND foretag_id = NEW.foretag_id
          AND status <> 'utkast'))
  OR NOT EXISTS (
        SELECT 1 FROM verifikat
        WHERE id = NEW.verifikat_id AND foretag_id = NEW.foretag_id AND bokford IS NOT NULL)
BEGIN
    SELECT RAISE(ABORT, 'en betalning kräver en utfärdad faktura och ett bokfört verifikat');
END;

CREATE TRIGGER betalning_andras_inte
BEFORE UPDATE ON betalning
BEGIN
    SELECT RAISE(ABORT, 'en betalning går inte att ändra');
END;

CREATE TRIGGER betalning_raderas_inte
BEFORE DELETE ON betalning
BEGIN
    SELECT RAISE(ABORT, 'en betalning går inte att radera');
END;

-- Rättelsen är en bokförd rättelse av just betalningens verifikat (R1).
CREATE TRIGGER betalningsrattelse_kraver_rattelsen
BEFORE INSERT ON betalningsrattelse
WHEN NOT EXISTS (
    SELECT 1 FROM verifikat v JOIN betalning b ON b.id = NEW.betalning_id
    WHERE v.id = NEW.verifikat_id AND v.rattar = b.verifikat_id AND v.bokford IS NOT NULL)
BEGIN
    SELECT RAISE(ABORT, 'rätta betalning kräver en bokförd rättelse av betalningens verifikat');
END;

CREATE TRIGGER betalningsrattelse_andras_inte
BEFORE UPDATE ON betalningsrattelse
BEGIN
    SELECT RAISE(ABORT, 'en rättad betalning går inte att ändra');
END;

CREATE TRIGGER betalningsrattelse_raderas_inte
BEFORE DELETE ON betalningsrattelse
BEGIN
    SELECT RAISE(ABORT, 'en rättad betalning går inte att ändra');
END;
