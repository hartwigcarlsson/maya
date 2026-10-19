-- Omvänd skattskyldighet och företag utan momsregistrering på kundfakturor (etapp 2, #136).
--
-- Ett utkast har den omvända skattskyldighet som användaren valde, med kundens förval. När
-- fakturan utfärdas skrivs den som gäller och om företaget var momsregistrerat på
-- fakturadatumet, så att den utfärdade fakturan bär sitt momsläge och inte räknas fram ur en
-- senare ändrad momsperiod (L1). Kolumnerna låses som resten av den utfärdade fakturan av
-- kundfaktura_utfardad_andras_inte.
--
-- Fakturor som utfärdades före den här migreringen har momsregistrerat NULL och läses som
-- momsregistrerade utan omvänd skattskyldighet, som de utfärdades.

ALTER TABLE kundfaktura ADD COLUMN omvand_skattskyldighet TEXT NOT NULL DEFAULT 'ingen'
    CHECK (omvand_skattskyldighet IN ('ingen', 'bygg', 'eu_tjanst'));

ALTER TABLE kundfaktura ADD COLUMN momsregistrerat INTEGER
    CHECK (momsregistrerat IN (0, 1));
