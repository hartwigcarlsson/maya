-- Vilka användare som hade åtkomst till ett raderat företag, så att raderingen i
-- installationsloggen syns för dem men inte för andra (story 36). Åtkomsten raderas med
-- företaget. Append-only som loggen (H3).
CREATE TABLE installationslogg_atkomst (
    logg_id INTEGER NOT NULL REFERENCES installationslogg (id),
    anvandare_id INTEGER NOT NULL REFERENCES anvandare (id),
    PRIMARY KEY (logg_id, anvandare_id)
) STRICT;

CREATE INDEX installationslogg_atkomst_anvandare ON installationslogg_atkomst (anvandare_id);

CREATE TRIGGER installationslogg_atkomst_andras_inte
BEFORE UPDATE ON installationslogg_atkomst
BEGIN
    SELECT RAISE(ABORT, 'installationsloggen går inte att ändra');
END;

CREATE TRIGGER installationslogg_atkomst_raderas_inte
BEFORE DELETE ON installationslogg_atkomst
BEGIN
    SELECT RAISE(ABORT, 'installationsloggen går inte att radera');
END;
