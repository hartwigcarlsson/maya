-- Återställningar med `maya restore` (A5, #123). Varje användare som fanns vid återställningen
-- ser en varning under Att göra tills hen kvitterar den: det som registrerades efter backupen
-- finns inte längre och måste registreras om.
CREATE TABLE aterstallning (
    id INTEGER PRIMARY KEY,
    -- När återställningen gjordes.
    tid TEXT NOT NULL,
    -- Backupens namn, t.ex. maya-20261018T010000Z, och dess tidpunkt.
    backup TEXT NOT NULL,
    backup_tid TEXT NOT NULL
) STRICT;

CREATE TABLE aterstallningsvarning (
    aterstallning_id INTEGER NOT NULL REFERENCES aterstallning (id),
    anvandare_id INTEGER NOT NULL REFERENCES anvandare (id),
    -- När användaren kvitterade varningen; NULL tills dess.
    kvitterad TEXT,
    PRIMARY KEY (aterstallning_id, anvandare_id)
) STRICT;

CREATE TRIGGER aterstallning_andras_inte
BEFORE UPDATE ON aterstallning
BEGIN
    SELECT RAISE(ABORT, 'en återställning går inte att ändra');
END;

CREATE TRIGGER aterstallning_raderas_inte
BEFORE DELETE ON aterstallning
BEGIN
    SELECT RAISE(ABORT, 'en återställning går inte att radera');
END;

-- En kvittens kan inte tas tillbaka.
CREATE TRIGGER aterstallningsvarning_kvitteras_en_gang
BEFORE UPDATE ON aterstallningsvarning
WHEN OLD.kvitterad IS NOT NULL OR NEW.kvitterad IS NULL
    OR NEW.aterstallning_id IS NOT OLD.aterstallning_id
    OR NEW.anvandare_id IS NOT OLD.anvandare_id
BEGIN
    SELECT RAISE(ABORT, 'en kvitterad varning går inte att ändra');
END;
