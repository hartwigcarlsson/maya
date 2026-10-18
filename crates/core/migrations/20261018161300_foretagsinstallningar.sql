-- Företagets övriga uppgifter under Inställningar → Företag, och radering av ett företag utan
-- räkenskapsinformation. Tom text betyder att uppgiften saknas.

ALTER TABLE foretag ADD COLUMN gatuadress TEXT NOT NULL DEFAULT '';
ALTER TABLE foretag ADD COLUMN postnummer TEXT NOT NULL DEFAULT '';
ALTER TABLE foretag ADD COLUMN postort TEXT NOT NULL DEFAULT '';
ALTER TABLE foretag ADD COLUMN epost TEXT NOT NULL DEFAULT '';
ALTER TABLE foretag ADD COLUMN telefon TEXT NOT NULL DEFAULT '';
ALTER TABLE foretag ADD COLUMN sate TEXT NOT NULL DEFAULT '';
ALTER TABLE foretag ADD COLUMN momsregnr TEXT NOT NULL DEFAULT '';
ALTER TABLE foretag ADD COLUMN f_skatt INTEGER NOT NULL DEFAULT 0 CHECK (f_skatt IN (0, 1));

-- Behandlingshistoriken för ett företag som finns går inte att radera (H3). När ett företag
-- utan räkenskapsinformation raderas tas företagsraden bort först, med främmande nycklar
-- uppskjutna till COMMIT, och sedan dess historik. Raderingen står i installationsloggen.
DROP TRIGGER behandlingshistorik_raderas_inte;

CREATE TRIGGER behandlingshistorik_raderas_inte
BEFORE DELETE ON behandlingshistorik
WHEN EXISTS (SELECT 1 FROM foretag WHERE id = OLD.foretag_id)
BEGIN
    SELECT RAISE(ABORT, 'behandlingshistoriken går inte att radera');
END;

-- Ett konto raderas aldrig medan företaget finns; på samma sätt som historiken ovan.
DROP TRIGGER konto_raderas_inte;

CREATE TRIGGER konto_raderas_inte
BEFORE DELETE ON konto
WHEN EXISTS (SELECT 1 FROM foretag WHERE id = OLD.foretag_id)
BEGIN
    SELECT RAISE(ABORT, 'ett konto raderas aldrig');
END;
