-- Kundfakturor (etapp 2, #135): utkast och utfärdade fakturor i kundreskontran, och företagets
-- fakturaserie.
--
-- Ett utkast kan ändras och raderas fritt. Att utfärda ger fakturan nästa nummer i företagets
-- obrutna fakturaserie, kopierar in kundens och företagets uppgifter, arkiverar PDF:en som
-- underlag (VF5) och bokför utfärdandets verifikat i samma transaktion. En utfärdad faktura, dess
-- rader, numret och den arkiverade PDF:en ändras och raderas aldrig (L1, A1); schemat avvisar
-- det. Serien är gemensam för fakturor och kreditfakturor och går över räkenskapsåren.
--
-- Belopp i öre. Antal och rabatt i hundradelar. Id:n återanvänds aldrig, eftersom
-- behandlingshistoriken pekar på dem.

-- Första fakturanumret, som väljs på det första utkastet. Saknas raden är det 1.
CREATE TABLE fakturaserie (
    foretag_id INTEGER PRIMARY KEY REFERENCES foretag (id),
    startnummer INTEGER NOT NULL CHECK (startnummer BETWEEN 1 AND 999999999)
) STRICT;

CREATE TABLE kundfaktura (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    foretag_id INTEGER NOT NULL REFERENCES foretag (id),
    status TEXT NOT NULL CHECK (status IN ('utkast', 'utfardad')),
    -- Kunden i kundregistret.
    kund_id INTEGER REFERENCES kund (id),
    fakturadatum TEXT,
    -- Leveransdatum eller period i fritext.
    leveransdatum TEXT NOT NULL DEFAULT '',
    forfallodag TEXT,
    er_referens TEXT NOT NULL DEFAULT '',
    -- Från utfärdandet: numret i fakturaserien och referensen (OCR eller numret).
    nummer INTEGER CHECK (nummer >= 1),
    referens TEXT,
    -- Kundens och företagets uppgifter som de var när fakturan utfärdades, som JSON.
    kunduppgifter TEXT CHECK (kunduppgifter IS NULL OR json_valid(kunduppgifter)),
    saljaruppgifter TEXT CHECK (saljaruppgifter IS NULL OR json_valid(saljaruppgifter)),
    netto INTEGER,
    moms INTEGER,
    total INTEGER,
    -- Den arkiverade PDF:en, ett underlag kopplat till verifikatet.
    underlag_id INTEGER REFERENCES underlag (id),
    -- Utfärdandets verifikat (fakturametoden).
    verifikat_id INTEGER REFERENCES verifikat (id),
    skapad TEXT NOT NULL,
    skapad_kanal TEXT NOT NULL CHECK (skapad_kanal IN ('webb', 'cli', 'maya')),
    skapad_av INTEGER REFERENCES anvandare (id),
    utfardad TEXT,
    utfardad_kanal TEXT CHECK (utfardad_kanal IN ('webb', 'cli', 'maya')),
    utfardad_av INTEGER REFERENCES anvandare (id),
    CHECK ((status = 'utkast') = (nummer IS NULL)),
    CHECK (status = 'utkast' OR (
        kund_id IS NOT NULL AND fakturadatum IS NOT NULL AND forfallodag IS NOT NULL
        AND referens IS NOT NULL AND kunduppgifter IS NOT NULL AND saljaruppgifter IS NOT NULL
        AND netto IS NOT NULL AND moms IS NOT NULL AND total IS NOT NULL
        AND underlag_id IS NOT NULL AND utfardad IS NOT NULL AND utfardad_kanal IS NOT NULL))
) STRICT;

CREATE INDEX kundfaktura_foretag ON kundfaktura (foretag_id, status);
CREATE INDEX kundfaktura_kund ON kundfaktura (kund_id);
CREATE INDEX kundfaktura_verifikat ON kundfaktura (verifikat_id);

-- Skyddsnätet för fakturaserien: ett nummer en gång per företag.
CREATE UNIQUE INDEX kundfaktura_nummer ON kundfaktura (foretag_id, nummer)
WHERE nummer IS NOT NULL;

CREATE TABLE kundfakturarad (
    faktura_id INTEGER NOT NULL REFERENCES kundfaktura (id),
    radnr INTEGER NOT NULL CHECK (radnr >= 1),
    beskrivning TEXT NOT NULL,
    antal INTEGER,
    enhet TEXT NOT NULL,
    a_pris INTEGER,
    rabatt INTEGER NOT NULL DEFAULT 0 CHECK (rabatt BETWEEN 0 AND 10000),
    momssats INTEGER NOT NULL CHECK (momssats IN (0, 6, 12, 25)),
    konto INTEGER,
    -- Radens netto, satt när fakturan utfärdas.
    netto INTEGER,
    PRIMARY KEY (faktura_id, radnr)
) STRICT;

-- En faktura skapas som utkast; bara utfärdandet gör den utfärdad.
CREATE TRIGGER kundfaktura_skapas_som_utkast
BEFORE INSERT ON kundfaktura
WHEN NEW.status <> 'utkast'
BEGIN
    SELECT RAISE(ABORT, 'en kundfaktura skapas som utkast');
END;

-- Numret är nästa i företagets serie: ett mer än det högsta, eller startnumret (VF3, VF4).
CREATE TRIGGER kundfaktura_nummer_i_serien
BEFORE UPDATE OF status, nummer ON kundfaktura
WHEN OLD.status = 'utkast' AND NEW.status <> 'utkast'
  AND NEW.nummer IS NOT (
      SELECT coalesce(
          max(nummer),
          (SELECT startnummer - 1 FROM fakturaserie WHERE foretag_id = NEW.foretag_id),
          0) + 1
      FROM kundfaktura WHERE foretag_id = NEW.foretag_id AND status <> 'utkast')
BEGIN
    SELECT RAISE(ABORT, 'fakturanumret är inte nästa nummer i fakturaserien');
END;

-- En utfärdad faktura ändras och raderas aldrig (L1, A1).
CREATE TRIGGER kundfaktura_utfardad_andras_inte
BEFORE UPDATE ON kundfaktura
WHEN OLD.status <> 'utkast'
BEGIN
    SELECT RAISE(ABORT, 'en utfärdad kundfaktura går inte att ändra');
END;

CREATE TRIGGER kundfaktura_utfardad_raderas_inte
BEFORE DELETE ON kundfaktura
WHEN OLD.status <> 'utkast'
BEGIN
    SELECT RAISE(ABORT, 'en utfärdad kundfaktura går inte att radera');
END;

CREATE TRIGGER kundfakturarad_utfardad_andras_inte
BEFORE UPDATE ON kundfakturarad
WHEN (SELECT status FROM kundfaktura WHERE id = OLD.faktura_id) <> 'utkast'
  OR (SELECT status FROM kundfaktura WHERE id = NEW.faktura_id) <> 'utkast'
BEGIN
    SELECT RAISE(ABORT, 'en utfärdad kundfaktura går inte att ändra');
END;

CREATE TRIGGER kundfakturarad_utfardad_raderas_inte
BEFORE DELETE ON kundfakturarad
WHEN (SELECT status FROM kundfaktura WHERE id = OLD.faktura_id) <> 'utkast'
BEGIN
    SELECT RAISE(ABORT, 'en utfärdad kundfaktura går inte att ändra');
END;

CREATE TRIGGER kundfakturarad_utfardad_laggs_inte_till
BEFORE INSERT ON kundfakturarad
WHEN (SELECT status FROM kundfaktura WHERE id = NEW.faktura_id) <> 'utkast'
BEGIN
    SELECT RAISE(ABORT, 'en utfärdad kundfaktura går inte att ändra');
END;

-- Startnumret ändras inte efter det första utfärdandet (H2: varje ändring före det loggas).
CREATE TRIGGER fakturaserie_andras_inte_efter_utfardande
BEFORE UPDATE ON fakturaserie
WHEN EXISTS (SELECT 1 FROM kundfaktura WHERE foretag_id = OLD.foretag_id AND status <> 'utkast')
BEGIN
    SELECT RAISE(ABORT, 'startnumret går inte att ändra efter första utfärdandet');
END;

CREATE TRIGGER fakturaserie_laggs_inte_till_efter_utfardande
BEFORE INSERT ON fakturaserie
WHEN EXISTS (SELECT 1 FROM kundfaktura WHERE foretag_id = NEW.foretag_id AND status <> 'utkast')
BEGIN
    SELECT RAISE(ABORT, 'startnumret går inte att ändra efter första utfärdandet');
END;

CREATE TRIGGER fakturaserie_raderas_inte_efter_utfardande
BEFORE DELETE ON fakturaserie
WHEN EXISTS (SELECT 1 FROM kundfaktura WHERE foretag_id = OLD.foretag_id AND status <> 'utkast')
BEGIN
    SELECT RAISE(ABORT, 'startnumret går inte att ändra efter första utfärdandet');
END;

-- En utfärdad kundfaktura är räkenskapsinformation: ett företag med en sådan raderas inte (A1).
-- Triggrarna från 20261019100000_radering_av_foretag.sql byggs om med det villkoret.
DROP TRIGGER foretag_radering_kraver_utan_rakenskapsinformation;

CREATE TRIGGER foretag_radering_kraver_utan_rakenskapsinformation
BEFORE INSERT ON foretag_radering
WHEN EXISTS (SELECT 1 FROM verifikat WHERE foretag_id = NEW.foretag_id AND bokford IS NOT NULL)
  OR EXISTS (SELECT 1 FROM underlag WHERE foretag_id = NEW.foretag_id)
  OR EXISTS (SELECT 1 FROM ingaende_balans b JOIN rakenskapsar r ON r.id = b.rakenskapsar_id
             WHERE r.foretag_id = NEW.foretag_id)
  OR EXISTS (SELECT 1 FROM kundfaktura
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
  OR EXISTS (SELECT 1 FROM kundfaktura WHERE foretag_id = OLD.id AND status <> 'utkast')
BEGIN
    SELECT RAISE(ABORT, 'ett företag raderas bara utan räkenskapsinformation, genom Maya');
END;
