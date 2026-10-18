-- Programversionen som senast startade mot databasen, så att ett byte kan loggas i varje
-- företags behandlingshistorik (H1). Saknas raden har ingen version sparats än.
CREATE TABLE programversion (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    version TEXT NOT NULL
) STRICT;
