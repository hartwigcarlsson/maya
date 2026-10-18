-- Backupens och verifieringens körningar (beslut i #13), så att Att göra och Installation kan
-- visa den senaste lyckade backupen och den senaste verifieringen. Installationsövergripande:
-- backupen gäller alltid hela installationen.
CREATE TABLE backupkorning (
    id INTEGER PRIMARY KEY,
    typ TEXT NOT NULL CHECK (typ IN ('backup', 'verifiering')),
    tid TEXT NOT NULL,
    kanal TEXT NOT NULL CHECK (kanal IN ('webb', 'cli', 'maya')),
    -- Backupens namn, t.ex. maya-20261018T010000Z; NULL när backupen inte kom så långt.
    backup TEXT,
    lyckades INTEGER NOT NULL CHECK (lyckades IN (0, 1)),
    -- Felen på svenska, en per rad; tom när körningen lyckades.
    meddelande TEXT NOT NULL
) STRICT;

CREATE INDEX backupkorning_typ ON backupkorning (typ, lyckades, id);
