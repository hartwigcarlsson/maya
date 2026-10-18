-- Radering av ett företag utan räkenskapsinformation, skyddad i schemat (H3, A1).
--
-- Förut släppte historikens och kontoplanens DELETE-triggers igenom allt för ett företag som
-- inte längre fanns, och inget hindrade `DELETE FROM foretag`. Med främmande nycklar avstängda
-- (sqlite3:s standard) kunde ett företag med bokförda verifikat då raderas och dess historik
-- tömmas. Nu gäller:
--
-- * `foretag_radering` markerar den radering som `foretag::radera` gör. Raden läggs in och
--   tas bort i samma transaktion.
-- * En markering går bara att lägga in för ett företag utan räkenskapsinformation (bokförda
--   verifikat eller underlag), samma regel som `har_räkenskapsinformation_i` i core.
-- * Företaget, dess kontoplan och dess behandlingshistorik går bara att radera när
--   markeringen finns.

CREATE TABLE foretag_radering (
    foretag_id INTEGER PRIMARY KEY
) STRICT;

CREATE TRIGGER foretag_radering_kraver_utan_rakenskapsinformation
BEFORE INSERT ON foretag_radering
WHEN EXISTS (SELECT 1 FROM verifikat WHERE foretag_id = NEW.foretag_id AND bokford IS NOT NULL)
  OR EXISTS (SELECT 1 FROM underlag WHERE foretag_id = NEW.foretag_id)
BEGIN
    SELECT RAISE(ABORT, 'ett företag med räkenskapsinformation går inte att radera');
END;

CREATE TRIGGER foretag_raderas_inte
BEFORE DELETE ON foretag
WHEN NOT EXISTS (SELECT 1 FROM foretag_radering WHERE foretag_id = OLD.id)
BEGIN
    SELECT RAISE(ABORT, 'ett företag raderas bara utan räkenskapsinformation, genom Maya');
END;

DROP TRIGGER behandlingshistorik_raderas_inte;

CREATE TRIGGER behandlingshistorik_raderas_inte
BEFORE DELETE ON behandlingshistorik
WHEN NOT EXISTS (SELECT 1 FROM foretag_radering WHERE foretag_id = OLD.foretag_id)
BEGIN
    SELECT RAISE(ABORT, 'behandlingshistoriken går inte att radera');
END;

DROP TRIGGER konto_raderas_inte;

CREATE TRIGGER konto_raderas_inte
BEFORE DELETE ON konto
WHEN NOT EXISTS (SELECT 1 FROM foretag_radering WHERE foretag_id = OLD.foretag_id)
BEGIN
    SELECT RAISE(ABORT, 'ett konto raderas aldrig');
END;
