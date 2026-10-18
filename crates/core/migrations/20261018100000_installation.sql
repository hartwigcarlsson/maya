-- Installationens relying party för passkeys: sparas vid första start. Passkeys är bundna till
-- rp_id, så Maya vägrar starta om den publika URL:en sedan ger något annat.
CREATE TABLE installation (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    rp_id TEXT NOT NULL,
    origin TEXT NOT NULL
) STRICT;
