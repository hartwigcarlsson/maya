-- Kursdifferens vid betalning av leverantörsfaktura (etapp 2, #147).
--
-- En leverantörsfaktura med originalvaluta registreras i SEK (L6), och betalningen i SEK kan bli
-- en annan. Avvikelsen mellan det som återstod och det som betalades stängs i betalningens
-- verifikat mot 3960 (vinst) eller 7960 (förlust) och står här, i öre med tecken: positiv vid
-- vinst och negativ vid förlust. Den räknas som betald, som öresutjämningen, så att
-- det som återstår = total − Σ(belopp + oresutjamning + kursdifferens).
--
-- Kolumnen läggs till med ALTER TABLE, så att betalningarna och deras triggrar inte byggs om;
-- befintliga rader får 0. Vyn byggs om så att den säkert har den nya kolumnen.

ALTER TABLE betalning
ADD COLUMN kursdifferens INTEGER NOT NULL DEFAULT 0
CHECK (kursdifferens = 0 OR leverantorsfaktura_id IS NOT NULL);

DROP VIEW betalning_i_reskontran;

-- Betalningarna som räknas i reskontran: de som inte är rättade.
CREATE VIEW betalning_i_reskontran AS
SELECT b.* FROM betalning b
WHERE NOT EXISTS (SELECT 1 FROM betalningsrattelse r WHERE r.betalning_id = b.id);

-- Kursdifferens finns bara på en leverantörsfaktura med originalvaluta.
CREATE TRIGGER betalning_kursdifferens_kraver_originalvaluta
BEFORE INSERT ON betalning
WHEN NEW.kursdifferens <> 0 AND NOT EXISTS (
    SELECT 1 FROM leverantorsfaktura
    WHERE id = NEW.leverantorsfaktura_id AND trim(originalvaluta) <> '')
BEGIN
    SELECT RAISE(ABORT, 'kursdifferens kräver en leverantörsfaktura med originalvaluta');
END;
