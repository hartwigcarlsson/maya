-- "Registrera om" görs bara tillsammans med en rättelse (R1): ett utkast får vara en kopia av ett
-- bokfört verifikat bara när verifikatet har en rättelse (som `verifikat::rätta` skapar först, i
-- samma transaktion), och bara en kopia per rättelse.

CREATE TRIGGER verifikat_kopia_kraver_rattelse
BEFORE INSERT ON verifikat
WHEN NEW.kopia_av IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'ett verifikat registreras om bara med en rättelse, en gång')
    WHERE NOT EXISTS (SELECT 1 FROM verifikat WHERE rattar = NEW.kopia_av)
       OR EXISTS (SELECT 1 FROM verifikat WHERE kopia_av = NEW.kopia_av);
END;
