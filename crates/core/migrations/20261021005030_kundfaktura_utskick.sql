-- Utskick av kundfakturor (etapp 2, #138): varje mejl som Maya skickade eller försökte skicka,
-- och varje markering att fakturan lämnades på annat sätt (post, personligen, kundens portal).
--
-- Att skicka är en loggad händelse och inget tillstånd på fakturan. Ett utskick sparas med tid,
-- kanal och vem som skickade (H1), mottagaren och svarsadressen, och resultatet: skickat, eller
-- misslyckat med felet. Utskicket görs efter att utfärdandet har committats, så ett misslyckat
-- utskick ändrar aldrig fakturan. Utskicken är fakturahändelser och ändras eller raderas aldrig
-- (L1, H3); schemat avvisar det.
CREATE TABLE kundfaktura_utskick (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    foretag_id INTEGER NOT NULL REFERENCES foretag (id),
    faktura_id INTEGER NOT NULL REFERENCES kundfaktura (id),
    tid TEXT NOT NULL,
    kanal TEXT NOT NULL CHECK (kanal IN ('webb', 'cli', 'maya')),
    av INTEGER REFERENCES anvandare (id),
    -- Med e-post från Maya, eller lämnad på annat sätt.
    satt TEXT NOT NULL CHECK (satt IN ('epost', 'post', 'personligen', 'portal')),
    mottagare TEXT,
    svarsadress TEXT,
    meddelande TEXT,
    resultat TEXT NOT NULL CHECK (resultat IN ('skickat', 'misslyckat', 'lamnad')),
    -- SMTP-serverns eller anslutningens fel när utskicket misslyckades.
    fel TEXT,
    CHECK ((satt = 'epost') = (mottagare IS NOT NULL AND meddelande IS NOT NULL)),
    CHECK ((satt = 'epost') = (resultat <> 'lamnad')),
    CHECK ((resultat = 'misslyckat') = (fel IS NOT NULL))
) STRICT;

CREATE INDEX kundfaktura_utskick_faktura ON kundfaktura_utskick (foretag_id, faktura_id);

-- Bara en utfärdad faktura i samma företag skickas.
CREATE TRIGGER kundfaktura_utskick_kraver_utfardad
BEFORE INSERT ON kundfaktura_utskick
WHEN coalesce(
    (SELECT status FROM kundfaktura WHERE id = NEW.faktura_id AND foretag_id = NEW.foretag_id),
    'utkast') = 'utkast'
BEGIN
    SELECT RAISE(ABORT, 'bara en utfärdad faktura skickas');
END;

CREATE TRIGGER kundfaktura_utskick_andras_inte
BEFORE UPDATE ON kundfaktura_utskick
BEGIN
    SELECT RAISE(ABORT, 'ett utskick går inte att ändra');
END;

CREATE TRIGGER kundfaktura_utskick_raderas_inte
BEFORE DELETE ON kundfaktura_utskick
BEGIN
    SELECT RAISE(ABORT, 'ett utskick går inte att ändra eller radera');
END;
