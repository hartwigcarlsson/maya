-- Kundregistret (#133): företagets kunder med de uppgifter som kundfakturan kopierar in när den
-- utfärdas. En kund kan ändras när som helst; en kund med fakturor arkiveras i stället för att
-- raderas. Varje skapande, ändring, arkivering och radering loggas i behandlingshistoriken
-- (H2), som pekar på kunden med dess id.

CREATE TABLE kund (
    -- AUTOINCREMENT: en raderad kunds id återanvänds aldrig, så behandlingshistoriken pekar
    -- bara på en kund.
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    foretag_id INTEGER NOT NULL REFERENCES foretag (id),
    namn TEXT NOT NULL CHECK (namn <> ''),
    -- Organisationsnummer eller personnummer med bindestreck, eller tom text.
    identitet TEXT NOT NULL,
    adress TEXT NOT NULL,
    epost TEXT NOT NULL,
    momsregnr TEXT NOT NULL,
    -- Dagar från fakturadatum till förfallodag.
    betalningsvillkor INTEGER NOT NULL CHECK (betalningsvillkor BETWEEN 0 AND 65535),
    omvand_skattskyldighet TEXT NOT NULL
        CHECK (omvand_skattskyldighet IN ('ingen', 'bygg', 'eu_tjanst')),
    -- aktiv: i registret och väljarna. arkiverad: i registret men inte i väljarna. ny: föreslagen
    -- från ett fakturautkast och ännu inte i registret (berättelse 7-8, läggs till med utkasten).
    status TEXT NOT NULL CHECK (status IN ('aktiv', 'arkiverad', 'ny')),
    skapad TEXT NOT NULL,
    skapad_kanal TEXT NOT NULL CHECK (skapad_kanal IN ('webb', 'cli', 'maya')),
    skapad_av INTEGER REFERENCES anvandare (id)
) STRICT;

CREATE INDEX kund_foretag ON kund (foretag_id, namn);
