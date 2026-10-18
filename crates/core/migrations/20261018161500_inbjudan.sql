-- Inbjudningar till ett företag (ADR 0005). Användaren skapas först när länken används, så att
-- en inbjudan som aldrig används inte lämnar en användare efter sig. Bara SHA-256 av token sparas.
CREATE TABLE inbjudan (
    hash BLOB PRIMARY KEY,
    foretag_id INTEGER NOT NULL REFERENCES foretag (id),
    epost TEXT NOT NULL,
    -- WebAuthn user handle för användaren som skapas: 16 slumpade byte.
    handle BLOB NOT NULL CHECK (length(handle) = 16),
    skapad TEXT NOT NULL,
    skapad_av INTEGER NOT NULL REFERENCES anvandare (id),
    giltig_till TEXT NOT NULL,
    anvand TEXT,
    -- Användaren som länken skapade.
    anvandare_id INTEGER REFERENCES anvandare (id),
    CHECK ((anvand IS NULL) = (anvandare_id IS NULL))
) STRICT;

CREATE INDEX inbjudan_foretag ON inbjudan (foretag_id);
