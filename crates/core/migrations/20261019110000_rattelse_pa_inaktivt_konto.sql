-- En rättelse vänder originalets rader också på ett konto som har inaktiverats sedan originalet
-- bokfördes (R1 kräver inga aktiva konton). Bokföringstriggern från rättelsens migrering
-- skapas om med det undantaget; kontot måste fortfarande finnas i kontoplanen.

DROP TRIGGER verifikat_bokfors;

CREATE TRIGGER verifikat_bokfors
BEFORE UPDATE OF bokford ON verifikat
WHEN OLD.bokford IS NULL AND NEW.bokford IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'verifikationsnumret lämnar en lucka eller en dubblett i serien')
    WHERE NEW.nummer IS NOT (
        SELECT coalesce(max(nummer), 0) + 1 FROM verifikat
        WHERE foretag_id = NEW.foretag_id AND rakenskapsar_id = NEW.rakenskapsar_id
    );
    SELECT RAISE(ABORT, 'ett bokfört verifikat behöver minst två rader')
    WHERE (SELECT count(*) FROM verifikatrad WHERE verifikat_id = NEW.id) < 2;
    SELECT RAISE(ABORT, 'varje rad har ett belopp antingen i debet eller i kredit')
    WHERE EXISTS (
        SELECT 1 FROM verifikatrad
        WHERE verifikat_id = NEW.id AND (debet > 0) = (kredit > 0)
    );
    SELECT RAISE(ABORT, 'debet är inte lika med kredit')
    WHERE (SELECT sum(debet) - sum(kredit) FROM verifikatrad WHERE verifikat_id = NEW.id) <> 0;
    SELECT RAISE(ABORT, 'ett konto finns inte eller är inaktivt')
    WHERE EXISTS (
        SELECT 1 FROM verifikatrad r
        LEFT JOIN konto k ON k.foretag_id = NEW.foretag_id AND k.nummer = r.konto
        WHERE r.verifikat_id = NEW.id
          AND (k.nummer IS NULL OR (k.aktiv = 0 AND NEW.kalla IS NOT 'rattelse'))
    );
    SELECT RAISE(ABORT, 'verifikatdatumet ligger inte i räkenskapsåret')
    WHERE NOT EXISTS (
        SELECT 1 FROM rakenskapsar
        WHERE id = NEW.rakenskapsar_id AND foretag_id = NEW.foretag_id
          AND NEW.datum BETWEEN start AND slut
    );
    SELECT RAISE(ABORT, 'rättelsen är daterad före verifikatet som den rättar')
    WHERE NEW.rattar IS NOT NULL
      AND NEW.datum < (SELECT datum FROM verifikat WHERE id = NEW.rattar);
END;
