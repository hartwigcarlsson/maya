-- En utfärdad kundfaktura (#135) är räkenskapsinformation: ett företag med en sådan raderas
-- inte (A1). Triggrarna från 20261019100000_radering_av_foretag.sql byggdes om med
-- leverantörsfakturorna i 20261020014500_leverantorsfaktura.sql; här byggs de om igen med båda
-- villkoren. Filen ligger efter 014500 så att den gäller, fast den hör till #135.

DROP TRIGGER foretag_radering_kraver_utan_rakenskapsinformation;

CREATE TRIGGER foretag_radering_kraver_utan_rakenskapsinformation
BEFORE INSERT ON foretag_radering
WHEN EXISTS (SELECT 1 FROM verifikat WHERE foretag_id = NEW.foretag_id AND bokford IS NOT NULL)
  OR EXISTS (SELECT 1 FROM underlag WHERE foretag_id = NEW.foretag_id)
  OR EXISTS (SELECT 1 FROM ingaende_balans b JOIN rakenskapsar r ON r.id = b.rakenskapsar_id
             WHERE r.foretag_id = NEW.foretag_id)
  OR EXISTS (SELECT 1 FROM leverantorsfaktura
             WHERE foretag_id = NEW.foretag_id AND status <> 'utkast')
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
  OR EXISTS (SELECT 1 FROM leverantorsfaktura WHERE foretag_id = OLD.id AND status <> 'utkast')
  OR EXISTS (SELECT 1 FROM kundfaktura WHERE foretag_id = OLD.id AND status <> 'utkast')
BEGIN
    SELECT RAISE(ABORT, 'ett företag raderas bara utan räkenskapsinformation, genom Maya');
END;
