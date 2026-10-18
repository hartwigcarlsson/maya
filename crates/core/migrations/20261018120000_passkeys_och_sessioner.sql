-- Sessionslistan visar vilken enhet en session hör till, och CLI:t ger återställningslänkar.

-- User-Agent när sessionen skapades, så att användaren känner igen sina enheter.
ALTER TABLE session ADD COLUMN enhet TEXT;

-- SQLite kan inte ändra en CHECK, så tabellen byggs om med 'aterstallning' som nytt syfte.
CREATE TABLE engangslank_ny (
    hash BLOB PRIMARY KEY,
    syfte TEXT NOT NULL CHECK (syfte IN ('registrering', 'aterstallning')),
    anvandare_id INTEGER NOT NULL REFERENCES anvandare (id),
    skapad TEXT NOT NULL,
    giltig_till TEXT NOT NULL,
    anvand TEXT
) STRICT;

INSERT INTO engangslank_ny (hash, syfte, anvandare_id, skapad, giltig_till, anvand)
SELECT hash, syfte, anvandare_id, skapad, giltig_till, anvand FROM engangslank;

DROP TABLE engangslank;

ALTER TABLE engangslank_ny RENAME TO engangslank;
