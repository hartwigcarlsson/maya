-- Fakturaflödenas konton (etapp 2, #135) är aktiva från början och kan aldrig inaktiveras. Ett
-- nytt företag får dem aktiva när BAS kopieras; här aktiveras de för befintliga företag. Samma
-- lista som maya_domain::kontoplan::FLÖDENAS_KONTON.
--
-- En aktivering ändrar kontoplanen och loggas därför i företagets behandlingshistorik med Maya
-- som aktör (H2), en post per företag och konto, innan kontot aktiveras.

INSERT INTO behandlingshistorik (foretag_id, tid, kanal, anvandare_id, typ, objekt, beskrivning)
SELECT foretag_id, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'), 'maya', NULL, 'konto aktiverat',
       CAST(nummer AS TEXT), json_object('skäl', 'fakturaflödenas konto (0.2)')
FROM konto
WHERE aktiv = 0
  AND nummer IN (1510, 1930, 2440, 2611, 2621, 2631, 2614, 2624, 2634, 2641, 2645, 2647,
                 3001, 3002, 3003, 3004, 3231, 3308, 3740, 3960, 6351, 7960)
ORDER BY foretag_id, nummer;

UPDATE konto SET aktiv = 1
WHERE aktiv = 0
  AND nummer IN (1510, 1930, 2440, 2611, 2621, 2631, 2614, 2624, 2634, 2641, 2645, 2647,
                 3001, 3002, 3003, 3004, 3231, 3308, 3740, 3960, 6351, 7960);
