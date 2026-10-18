-- Leverantörsregistret (etapp 2, #144): företagets leverantörer med adress, momsreg-nr, land,
-- betaluppgifter, standardkonto och förvald omvänd skattskyldighet. Standardkontot är en
-- automatkontering (S2); varje skapande, ändring, arkivering och radering loggas i
-- behandlingshistoriken, standardkontot som egen typ (H2). En leverantör med fakturor kan
-- arkiveras men inte raderas (A1); leverantörsfakturorna (#145) pekar hit.
--
-- Status: `aktiv` och `arkiverad`. `ny` är en leverantör som skapats från ett
-- leverantörsfakturautkast och blir aktiv när fakturan registreras (#145); den visas inte i
-- registret. Id:n återanvänds aldrig, eftersom historiken pekar på dem.

CREATE TABLE leverantor (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    foretag_id INTEGER NOT NULL REFERENCES foretag (id),
    status TEXT NOT NULL CHECK (status IN ('ny', 'aktiv', 'arkiverad')),
    namn TEXT NOT NULL CHECK (trim(namn) <> ''),
    -- NNNNNN-NNNN för en svensk leverantör, annars som det skrevs; tom när det saknas.
    orgnr TEXT NOT NULL DEFAULT '',
    gatuadress TEXT NOT NULL DEFAULT '',
    postnummer TEXT NOT NULL DEFAULT '',
    postort TEXT NOT NULL DEFAULT '',
    -- Landskod enligt ISO 3166.
    land TEXT NOT NULL DEFAULT 'SE' CHECK (length(land) = 2),
    momsregnr TEXT NOT NULL DEFAULT '',
    bankgiro TEXT NOT NULL DEFAULT '',
    plusgiro TEXT NOT NULL DEFAULT '',
    iban TEXT NOT NULL DEFAULT '',
    bic TEXT NOT NULL DEFAULT '',
    -- Kontot som leverantörens fakturarader får förvalt; NULL när inget är valt.
    standardkonto INTEGER,
    omvand_skattskyldighet TEXT NOT NULL DEFAULT 'ingen'
        CHECK (omvand_skattskyldighet IN ('ingen', 'eu_tjanst', 'eu_vara', 'bygg', 'utanfor_eu')),
    skapad TEXT NOT NULL,
    skapad_kanal TEXT NOT NULL CHECK (skapad_kanal IN ('webb', 'cli', 'maya')),
    skapad_av INTEGER REFERENCES anvandare (id),
    CHECK ((iban = '') = (bic = '')),
    FOREIGN KEY (foretag_id, standardkonto) REFERENCES konto (foretag_id, nummer)
) STRICT;

CREATE INDEX leverantor_foretag ON leverantor (foretag_id, status, namn);
