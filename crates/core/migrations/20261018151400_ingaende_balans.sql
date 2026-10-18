-- Ingående balans: saldot per balanskonto (klass 1–2) när ett räkenskapsår börjar, i öre med
-- debet som positivt (Å2). Den är inget verifikat. I etapp 1 skrivs den in för företagets första
-- år; den preliminära för senare år räknas fram vid läsning och sparas inte. Etapp 5 skriver den
-- slutgiltiga här vid stängningen. Bara saldon skilda från noll sparas, och summan per år är noll
-- (kontrolleras i `domain` före varje ändring). Varje ändring loggas i behandlingshistoriken.

CREATE TABLE ingaende_balans (
    rakenskapsar_id INTEGER NOT NULL REFERENCES rakenskapsar (id),
    konto INTEGER NOT NULL CHECK (konto BETWEEN 1000 AND 2999),
    saldo INTEGER NOT NULL CHECK (saldo <> 0),
    PRIMARY KEY (rakenskapsar_id, konto)
) STRICT;
