-- no-transaction
-- Externa kundfakturor (etapp 2, #143): kundfakturor som har skapats i ett annat system och
-- registreras i Maya med sin PDF som underlag.
--
-- En extern faktura är en rad i `kundfaktura` med `extern = 1`, så att betalningar, kreditering,
-- kundförlust och det som återstår går genom samma tabeller och samma regler som för Mayas egna.
-- Den har ett eget fakturanummer i fritext (`fakturanummer`, unikt bland företagets externa
-- fakturor) och ingår inte i fakturaserien (`nummer` är NULL). Underlaget är originalet från det
-- andra systemet. Livscykeln är utkast → registrerad; registreringen bokför 1510 mot intäkt och
-- moms i samma transaktion (L5). En extern kreditfaktura har negativ total och en hänvisning
-- (`krediterar`) till en registrerad extern faktura.
--
-- Rätta registrering vänder registreringens verifikat med en rättelse (R1) och låser upp
-- fakturan: status `upplast`. Upplåsningen står i `kundfaktura_registrering`, som aldrig ändras
-- (R2). En registrerad faktura ändras fortfarande aldrig (L1), utom att den blir upplåst när en
-- rättelse av just dess registrering finns. En upplåst faktura skrivs av igen, men originalet,
-- företaget, hänvisningen och vem som skapade den ändras inte, och den raderas inte; den
-- registreras om med ett nytt verifikat som inte är rättat. Betalningarna ligger kvar.
--
-- SQLite kan inte ändra en CHECK, så `kundfaktura` byggs om, med mallen från
-- 20261018181100_rattelse.sql. Triggrarna och indexen på tabellen försvinner med den och skapas
-- om här, med villkoren från 20261020013501_kundfaktura.sql och 20261021140000_kreditfaktura.sql.
-- Triggrar på andra tabeller som räknade "inte utkast" som utfärdad byggs om så att de skiljer
-- på utfärdad, registrerad och upplåst.

PRAGMA foreign_keys = OFF;
PRAGMA legacy_alter_table = ON;

BEGIN IMMEDIATE;

CREATE TABLE kundfaktura_ny (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    foretag_id INTEGER NOT NULL REFERENCES foretag (id),
    status TEXT NOT NULL CHECK (status IN ('utkast', 'utfardad', 'registrerad', 'upplast')),
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
    -- Kundens och företagets uppgifter som de var när fakturan utfärdades, som JSON. En extern
    -- faktura har kundens uppgifter från registreringen.
    kunduppgifter TEXT CHECK (kunduppgifter IS NULL OR json_valid(kunduppgifter)),
    saljaruppgifter TEXT CHECK (saljaruppgifter IS NULL OR json_valid(saljaruppgifter)),
    netto INTEGER,
    moms INTEGER,
    total INTEGER,
    -- Den arkiverade PDF:en, eller originalet till en extern faktura.
    underlag_id INTEGER REFERENCES underlag (id),
    -- Utfärdandets eller registreringens verifikat (fakturametoden); efter Rätta registrering
    -- det senaste.
    verifikat_id INTEGER REFERENCES verifikat (id),
    skapad TEXT NOT NULL,
    skapad_kanal TEXT NOT NULL CHECK (skapad_kanal IN ('webb', 'cli', 'maya')),
    skapad_av INTEGER REFERENCES anvandare (id),
    utfardad TEXT,
    utfardad_kanal TEXT CHECK (utfardad_kanal IN ('webb', 'cli', 'maya')),
    utfardad_av INTEGER REFERENCES anvandare (id),
    omvand_skattskyldighet TEXT NOT NULL DEFAULT 'ingen'
        CHECK (omvand_skattskyldighet IN ('ingen', 'bygg', 'eu_tjanst')),
    momsregistrerat INTEGER CHECK (momsregistrerat IN (0, 1)),
    krediterar INTEGER REFERENCES kundfaktura (id),
    -- #143: fakturan kommer från ett annat system, med sitt eget nummer.
    extern INTEGER NOT NULL DEFAULT 0 CHECK (extern IN (0, 1)),
    fakturanummer TEXT NOT NULL DEFAULT '',
    -- Den senaste registreringen av en extern faktura.
    registrerad TEXT,
    registrerad_kanal TEXT CHECK (registrerad_kanal IN ('webb', 'cli', 'maya')),
    registrerad_av INTEGER REFERENCES anvandare (id),
    -- Bara Mayas egna fakturor utfärdas och har ett nummer i serien.
    CHECK ((nummer IS NOT NULL) = (status = 'utfardad')),
    CHECK (status <> 'utfardad' OR (
        extern = 0
        AND kund_id IS NOT NULL AND fakturadatum IS NOT NULL AND forfallodag IS NOT NULL
        AND referens IS NOT NULL AND kunduppgifter IS NOT NULL AND saljaruppgifter IS NOT NULL
        AND netto IS NOT NULL AND moms IS NOT NULL AND total IS NOT NULL
        AND underlag_id IS NOT NULL AND utfardad IS NOT NULL AND utfardad_kanal IS NOT NULL)),
    -- Bara externa fakturor registreras, och de har alltid sitt original.
    CHECK (extern = 1 OR status IN ('utkast', 'utfardad')),
    CHECK (extern = 0 OR underlag_id IS NOT NULL),
    CHECK (extern = 1 OR fakturanummer = ''),
    CHECK (status <> 'registrerad' OR (
        kund_id IS NOT NULL AND fakturanummer <> '' AND fakturadatum IS NOT NULL
        AND forfallodag IS NOT NULL AND kunduppgifter IS NOT NULL
        AND netto IS NOT NULL AND moms IS NOT NULL AND total IS NOT NULL
        AND registrerad IS NOT NULL AND registrerad_kanal IS NOT NULL)),
    -- En upplåst faktura har registrerats en gång.
    CHECK (status <> 'upplast' OR (registrerad IS NOT NULL AND registrerad_kanal IS NOT NULL)),
    -- En registrerad extern kreditfaktura har negativ total.
    CHECK (status <> 'registrerad' OR krediterar IS NULL OR total < 0),
    CHECK (krediterar IS NOT id)
) STRICT;

INSERT INTO kundfaktura_ny
    (id, foretag_id, status, kund_id, fakturadatum, leveransdatum, forfallodag, er_referens,
     nummer, referens, kunduppgifter, saljaruppgifter, netto, moms, total, underlag_id,
     verifikat_id, skapad, skapad_kanal, skapad_av, utfardad, utfardad_kanal, utfardad_av,
     omvand_skattskyldighet, momsregistrerat, krediterar)
SELECT id, foretag_id, status, kund_id, fakturadatum, leveransdatum, forfallodag, er_referens,
       nummer, referens, kunduppgifter, saljaruppgifter, netto, moms, total, underlag_id,
       verifikat_id, skapad, skapad_kanal, skapad_av, utfardad, utfardad_kanal, utfardad_av,
       omvand_skattskyldighet, momsregistrerat, krediterar
FROM kundfaktura;

DROP TABLE kundfaktura;
ALTER TABLE kundfaktura_ny RENAME TO kundfaktura;

-- Som i 20261020013501_kundfaktura.sql och 20261021140000_kreditfaktura.sql.
CREATE INDEX kundfaktura_foretag ON kundfaktura (foretag_id, status);
CREATE INDEX kundfaktura_kund ON kundfaktura (kund_id);
CREATE INDEX kundfaktura_verifikat ON kundfaktura (verifikat_id);
CREATE UNIQUE INDEX kundfaktura_nummer ON kundfaktura (foretag_id, nummer)
WHERE nummer IS NOT NULL;
CREATE INDEX kundfaktura_krediterar ON kundfaktura (krediterar) WHERE krediterar IS NOT NULL;
CREATE INDEX kundfaktura_underlag ON kundfaktura (underlag_id);

-- Skyddsnätet för dubblettspärren: ett externt fakturanummer en gång bland företagets externa
-- fakturor som inte är utkast.
CREATE UNIQUE INDEX kundfaktura_externt_nummer
ON kundfaktura (foretag_id, lower(fakturanummer))
WHERE extern = 1 AND status <> 'utkast';

-- En faktura skapas som utkast; bara utfärdandet eller registreringen gör den till något annat.
CREATE TRIGGER kundfaktura_skapas_som_utkast
BEFORE INSERT ON kundfaktura
WHEN NEW.status <> 'utkast'
BEGIN
    SELECT RAISE(ABORT, 'en kundfaktura skapas som utkast');
END;

-- Numret är nästa i företagets serie: ett mer än det högsta, eller startnumret (VF3, VF4).
-- Externa fakturor ingår inte i serien.
CREATE TRIGGER kundfaktura_nummer_i_serien
BEFORE UPDATE OF status, nummer ON kundfaktura
WHEN OLD.status = 'utkast' AND NEW.status <> 'utkast' AND NEW.extern = 0
  AND NEW.nummer IS NOT (
      SELECT coalesce(
          max(nummer),
          (SELECT startnummer - 1 FROM fakturaserie WHERE foretag_id = NEW.foretag_id),
          0) + 1
      FROM kundfaktura WHERE foretag_id = NEW.foretag_id AND status <> 'utkast')
BEGIN
    SELECT RAISE(ABORT, 'fakturanumret är inte nästa nummer i fakturaserien');
END;

-- En utfärdad faktura ändras och raderas aldrig (L1, A1), inte heller en registrerad eller
-- upplåst.
CREATE TRIGGER kundfaktura_utfardad_andras_inte
BEFORE UPDATE ON kundfaktura
WHEN OLD.status = 'utfardad'
BEGIN
    SELECT RAISE(ABORT, 'en utfärdad kundfaktura går inte att ändra');
END;

CREATE TRIGGER kundfaktura_utfardad_raderas_inte
BEFORE DELETE ON kundfaktura
WHEN OLD.status <> 'utkast'
BEGIN
    SELECT RAISE(ABORT, 'en utfärdad kundfaktura går inte att radera');
END;

-- Hänvisningen pekar på en faktura av samma slag i samma företag som inte är en kreditfaktura:
-- Mayas kreditfakturor på en utfärdad faktura, en extern kreditfaktura på en registrerad extern.
CREATE TRIGGER kreditfaktura_kraver_utfardat_original
BEFORE INSERT ON kundfaktura
WHEN NEW.krediterar IS NOT NULL AND NOT EXISTS (
    SELECT 1 FROM kundfaktura o
    WHERE o.id = NEW.krediterar AND o.foretag_id = NEW.foretag_id
      AND o.extern = NEW.extern AND o.status IN ('utfardad', 'registrerad')
      AND o.krediterar IS NULL)
BEGIN
    SELECT RAISE(ABORT, 'en kreditfaktura krediterar en utfärdad faktura som inte är en kreditfaktura');
END;

-- Hänvisningen sätts när utkastet skapas och ändras inte, inte heller på ett utkast.
CREATE TRIGGER kreditfaktura_hanvisningen_andras_inte
BEFORE UPDATE OF krediterar ON kundfaktura
WHEN NEW.krediterar IS NOT OLD.krediterar
BEGIN
    SELECT RAISE(ABORT, 'kreditfakturans hänvisning går inte att ändra');
END;

-- Skyddsnätet för spärrarna vid utfärdande och registrering: summan är negativ och högst det
-- som finns kvar att kreditera, utan kreditfakturan själv.
CREATE TRIGGER kreditfaktura_krediterar_hogst_det_som_finns_kvar
BEFORE UPDATE OF status ON kundfaktura
WHEN OLD.status IN ('utkast', 'upplast') AND NEW.status IN ('utfardad', 'registrerad')
  AND NEW.krediterar IS NOT NULL
  AND (NEW.total >= 0 OR -NEW.total > (
      SELECT o.total + coalesce((
          SELECT sum(k.total) FROM kundfaktura k
          WHERE k.krediterar = o.id AND k.id <> NEW.id
            AND k.status IN ('utfardad', 'registrerad')), 0)
      FROM kundfaktura o WHERE o.id = NEW.krediterar))
BEGIN
    SELECT RAISE(ABORT, 'kreditfakturan krediterar mer än som finns kvar att kreditera');
END;

-- Rätta registrering: upplåsningen av en registrering, med rättelsen av registreringens
-- verifikat (R1). `registrering` är tiden för registreringen som låstes upp. Verifikatet och
-- rättelsen är båda satta, eller båda tomma för en faktura vars fordran inte var bokförd
-- (kontantmetoden, #149).
CREATE TABLE kundfaktura_registrering (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    foretag_id INTEGER NOT NULL REFERENCES foretag (id),
    faktura_id INTEGER NOT NULL REFERENCES kundfaktura (id),
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

CREATE INDEX kundfaktura_registrering_faktura ON kundfaktura_registrering (faktura_id);
CREATE INDEX kundfaktura_registrering_verifikat ON kundfaktura_registrering (verifikat_id);

-- Upplåsningen gäller fakturans registrering som den är nu, och rättelsen är en bokförd
-- rättelse av just registreringens verifikat.
CREATE TRIGGER kundfaktura_registrering_kraver_registreringen
BEFORE INSERT ON kundfaktura_registrering
WHEN NOT EXISTS (
        SELECT 1 FROM kundfaktura
        WHERE id = NEW.faktura_id AND foretag_id = NEW.foretag_id AND status = 'registrerad'
          AND registrerad = NEW.registrering AND verifikat_id IS NEW.verifikat_id)
  OR (NEW.rattelse_id IS NOT NULL AND NOT EXISTS (
        SELECT 1 FROM verifikat
        WHERE id = NEW.rattelse_id AND rattar = NEW.verifikat_id AND bokford IS NOT NULL))
BEGIN
    SELECT RAISE(ABORT, 'rätta registrering kräver en registrerad faktura och en bokförd rättelse av registreringen');
END;

CREATE TRIGGER kundfaktura_registrering_andras_inte
BEFORE UPDATE ON kundfaktura_registrering
BEGIN
    SELECT RAISE(ABORT, 'en rättad registrering går inte att ändra');
END;

CREATE TRIGGER kundfaktura_registrering_raderas_inte
BEFORE DELETE ON kundfaktura_registrering
BEGIN
    SELECT RAISE(ABORT, 'en rättad registrering går inte att ändra');
END;

-- En registrerad faktura ändras aldrig (L1), utom att den blir upplåst, med allt annat som det
-- var, när en rättelse av just den här registreringen finns.
CREATE TRIGGER kundfaktura_registrerad_andras_inte
BEFORE UPDATE ON kundfaktura
WHEN OLD.status = 'registrerad' AND NOT (
    NEW.status = 'upplast'
    AND NEW.id = OLD.id AND NEW.foretag_id = OLD.foretag_id
    AND NEW.kund_id IS OLD.kund_id AND NEW.fakturadatum IS OLD.fakturadatum
    AND NEW.leveransdatum = OLD.leveransdatum AND NEW.forfallodag IS OLD.forfallodag
    AND NEW.er_referens = OLD.er_referens AND NEW.nummer IS OLD.nummer
    AND NEW.referens IS OLD.referens AND NEW.kunduppgifter IS OLD.kunduppgifter
    AND NEW.saljaruppgifter IS OLD.saljaruppgifter AND NEW.netto IS OLD.netto
    AND NEW.moms IS OLD.moms AND NEW.total IS OLD.total
    AND NEW.underlag_id IS OLD.underlag_id AND NEW.verifikat_id IS OLD.verifikat_id
    AND NEW.skapad = OLD.skapad AND NEW.skapad_kanal = OLD.skapad_kanal
    AND NEW.skapad_av IS OLD.skapad_av AND NEW.utfardad IS OLD.utfardad
    AND NEW.utfardad_kanal IS OLD.utfardad_kanal AND NEW.utfardad_av IS OLD.utfardad_av
    AND NEW.omvand_skattskyldighet = OLD.omvand_skattskyldighet
    AND NEW.momsregistrerat IS OLD.momsregistrerat AND NEW.krediterar IS OLD.krediterar
    AND NEW.extern = OLD.extern AND NEW.fakturanummer = OLD.fakturanummer
    AND NEW.registrerad IS OLD.registrerad AND NEW.registrerad_kanal IS OLD.registrerad_kanal
    AND NEW.registrerad_av IS OLD.registrerad_av
    AND EXISTS (SELECT 1 FROM kundfaktura_registrering
                WHERE faktura_id = OLD.id AND registrering = OLD.registrerad
                  AND verifikat_id IS OLD.verifikat_id))
BEGIN
    SELECT RAISE(ABORT, 'en registrerad kundfaktura går inte att ändra');
END;

-- En upplåst faktura behåller sitt original, sitt företag, sin hänvisning och sin skapare, blir
-- inte ett utkast igen och behåller registreringen tills den registreras om. Att registrera om
-- ger en ny registrering med ett verifikat som inte redan är rättat.
CREATE TRIGGER kundfaktura_upplast_andras_bara_i_registreringen
BEFORE UPDATE ON kundfaktura
WHEN OLD.status = 'upplast' AND (
    NEW.id <> OLD.id OR NEW.foretag_id <> OLD.foretag_id
    OR NEW.underlag_id IS NOT OLD.underlag_id OR NEW.extern <> OLD.extern
    OR NEW.nummer IS NOT NULL OR NEW.saljaruppgifter IS NOT OLD.saljaruppgifter
    OR NEW.skapad <> OLD.skapad OR NEW.skapad_kanal <> OLD.skapad_kanal
    OR NEW.skapad_av IS NOT OLD.skapad_av
    OR NEW.status NOT IN ('upplast', 'registrerad')
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
    SELECT RAISE(ABORT, 'en upplåst kundfaktura ändras bara i registreringen');
END;

-- Raderna på en utfärdad eller registrerad faktura ändras inte; på ett utkast och en upplåst
-- faktura skrivs de fritt. Ersätter triggrarna från 20261020013501_kundfaktura.sql.
DROP TRIGGER kundfakturarad_utfardad_andras_inte;
DROP TRIGGER kundfakturarad_utfardad_raderas_inte;
DROP TRIGGER kundfakturarad_utfardad_laggs_inte_till;

CREATE TRIGGER kundfakturarad_utfardad_andras_inte
BEFORE UPDATE ON kundfakturarad
WHEN (SELECT status FROM kundfaktura WHERE id = OLD.faktura_id) IN ('utfardad', 'registrerad')
  OR (SELECT status FROM kundfaktura WHERE id = NEW.faktura_id) IN ('utfardad', 'registrerad')
BEGIN
    SELECT RAISE(ABORT, 'en utfärdad kundfaktura går inte att ändra');
END;

CREATE TRIGGER kundfakturarad_utfardad_raderas_inte
BEFORE DELETE ON kundfakturarad
WHEN (SELECT status FROM kundfaktura WHERE id = OLD.faktura_id) IN ('utfardad', 'registrerad')
BEGIN
    SELECT RAISE(ABORT, 'en utfärdad kundfaktura går inte att ändra');
END;

CREATE TRIGGER kundfakturarad_utfardad_laggs_inte_till
BEFORE INSERT ON kundfakturarad
WHEN (SELECT status FROM kundfaktura WHERE id = NEW.faktura_id) IN ('utfardad', 'registrerad')
BEGIN
    SELECT RAISE(ABORT, 'en utfärdad kundfaktura går inte att ändra');
END;

-- Startnumret låses av det första utfärdandet; externa fakturor ingår inte i serien.
-- Ersätter triggrarna från 20261020013501_kundfaktura.sql.
DROP TRIGGER fakturaserie_andras_inte_efter_utfardande;
DROP TRIGGER fakturaserie_laggs_inte_till_efter_utfardande;
DROP TRIGGER fakturaserie_raderas_inte_efter_utfardande;

CREATE TRIGGER fakturaserie_andras_inte_efter_utfardande
BEFORE UPDATE ON fakturaserie
WHEN EXISTS (SELECT 1 FROM kundfaktura WHERE foretag_id = OLD.foretag_id AND status = 'utfardad')
BEGIN
    SELECT RAISE(ABORT, 'startnumret går inte att ändra efter första utfärdandet');
END;

CREATE TRIGGER fakturaserie_laggs_inte_till_efter_utfardande
BEFORE INSERT ON fakturaserie
WHEN EXISTS (SELECT 1 FROM kundfaktura WHERE foretag_id = NEW.foretag_id AND status = 'utfardad')
BEGIN
    SELECT RAISE(ABORT, 'startnumret går inte att ändra efter första utfärdandet');
END;

CREATE TRIGGER fakturaserie_raderas_inte_efter_utfardande
BEFORE DELETE ON fakturaserie
WHEN EXISTS (SELECT 1 FROM kundfaktura WHERE foretag_id = OLD.foretag_id AND status = 'utfardad')
BEGIN
    SELECT RAISE(ABORT, 'startnumret går inte att ändra efter första utfärdandet');
END;

-- Nya betalningar och kundförluster gäller en utfärdad eller registrerad kundfaktura, inte en
-- upplåst, vars registrering är vänd.
CREATE TRIGGER betalning_kraver_bokford_kundfaktura
BEFORE INSERT ON betalning
WHEN NEW.kundfaktura_id IS NOT NULL AND NOT EXISTS (
    SELECT 1 FROM kundfaktura
    WHERE id = NEW.kundfaktura_id AND status IN ('utfardad', 'registrerad'))
BEGIN
    SELECT RAISE(ABORT, 'en betalning kräver en utfärdad eller registrerad kundfaktura');
END;

CREATE TRIGGER kundforlust_kraver_bokford_kundfaktura
BEFORE INSERT ON kundforlust
WHEN NOT EXISTS (
    SELECT 1 FROM kundfaktura
    WHERE id = NEW.kundfaktura_id AND status IN ('utfardad', 'registrerad'))
BEGIN
    SELECT RAISE(ABORT, 'en kundförlust kräver en utfärdad eller registrerad kundfaktura');
END;

-- Bara Mayas egna fakturor skickas; en extern faktura skickades från det andra systemet.
CREATE TRIGGER kundfaktura_utskick_bara_mayas_fakturor
BEFORE INSERT ON kundfaktura_utskick
WHEN EXISTS (SELECT 1 FROM kundfaktura WHERE id = NEW.faktura_id AND extern = 1)
BEGIN
    SELECT RAISE(ABORT, 'bara en utfärdad faktura skickas');
END;

-- Inga främmande nycklar bröts under bytet; annars avbryts migreringen här.
CREATE TEMP TABLE externa_kundfakturor_kontroll (brott INTEGER NOT NULL CHECK (brott = 0));
INSERT INTO externa_kundfakturor_kontroll SELECT count(*) FROM pragma_foreign_key_check;
DROP TABLE externa_kundfakturor_kontroll;

COMMIT;

PRAGMA legacy_alter_table = OFF;
PRAGMA foreign_keys = ON;
