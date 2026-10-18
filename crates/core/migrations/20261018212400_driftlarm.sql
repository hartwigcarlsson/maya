-- Larmen till driften (beslut 13 i #70): ett larm per fel som har skickats och inte har lösts än,
-- så att Maya skickar högst ett per dygn och fel och ett när felet är löst. Raden tas bort när
-- larmet om att felet är löst har skickats.
CREATE TABLE driftlarm (
    fel TEXT PRIMARY KEY
        CHECK (fel IN ('backup_misslyckades', 'backup_gammal', 'verifiering_misslyckades')),
    -- När det senaste larmet om felet skickades.
    skickat TEXT NOT NULL
) STRICT;
