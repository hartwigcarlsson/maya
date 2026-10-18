-- Företagets fakturauppgifter under Inställningar → Företag (#134): betalsätten, om fakturorna
-- har OCR-referens, logotypen, svarsadressen och standardmeddelandet för fakturamejl. En rad
-- per företag, skapad vid första ändringen; utan rad gäller tomma uppgifter och OCR av. Varje
-- ändring loggas i behandlingshistoriken (H2). En utfärdad faktura kopierar in det den
-- använder, så raden kan ändras och logotypen bytas utan att en gammal faktura ändras.

CREATE TABLE fakturauppgifter (
    foretag_id INTEGER PRIMARY KEY REFERENCES foretag (id),
    -- Kontrollerade av maya_domain::fakturauppgifter; tom text betyder att uppgiften saknas.
    bankgiro TEXT NOT NULL DEFAULT '',
    plusgiro TEXT NOT NULL DEFAULT '',
    iban TEXT NOT NULL DEFAULT '',
    bic TEXT NOT NULL DEFAULT '',
    ocr INTEGER NOT NULL DEFAULT 0 CHECK (ocr IN (0, 1)),
    svarsadress TEXT NOT NULL DEFAULT '',
    standardmeddelande TEXT NOT NULL DEFAULT '',
    -- Logotypen till fakturamallen: PNG, JPG eller SVG, eller ingen.
    logotyp BLOB,
    logotyp_mime TEXT CHECK (logotyp_mime IN ('image/png', 'image/jpeg', 'image/svg+xml')),
    logotyp_filnamn TEXT,
    logotyp_sha256 TEXT,
    CHECK ((logotyp IS NULL) = (logotyp_mime IS NULL)
       AND (logotyp IS NULL) = (logotyp_filnamn IS NULL)
       AND (logotyp IS NULL) = (logotyp_sha256 IS NULL))
) STRICT;
