-- Schemaepoken: '0' under 0.x-tiden. Maya 1.0 bär epok '1' och vägrar starta mot en
-- databas från epok '0', så att provdata aldrig blir skarp bokföring (A4).
CREATE TABLE schemaepok (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    epok TEXT NOT NULL
) STRICT;

INSERT INTO schemaepok (id, epok) VALUES (1, '0');
