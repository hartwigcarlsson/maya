-- Företagets övriga uppgifter under Inställningar → Företag, och radering av ett företag utan
-- räkenskapsinformation. Tom text betyder att uppgiften saknas.

ALTER TABLE foretag ADD COLUMN gatuadress TEXT NOT NULL DEFAULT '';
ALTER TABLE foretag ADD COLUMN postnummer TEXT NOT NULL DEFAULT '';
ALTER TABLE foretag ADD COLUMN postort TEXT NOT NULL DEFAULT '';
ALTER TABLE foretag ADD COLUMN epost TEXT NOT NULL DEFAULT '';
ALTER TABLE foretag ADD COLUMN telefon TEXT NOT NULL DEFAULT '';
ALTER TABLE foretag ADD COLUMN sate TEXT NOT NULL DEFAULT '';
ALTER TABLE foretag ADD COLUMN momsregnr TEXT NOT NULL DEFAULT '';
ALTER TABLE foretag ADD COLUMN f_skatt INTEGER NOT NULL DEFAULT 0 CHECK (f_skatt IN (0, 1));
