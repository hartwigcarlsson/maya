-- Omvänd skattskyldighet på leverantörsfakturor (etapp 2, #146).
--
-- En leverantörsfaktura kan ha omvänd skattskyldighet för EU-tjänst, EU-vara, bygg eller tjänst
-- från land utanför EU. Registreringen bokför då fiktiv ingående moms (2645, 2647 för bygg) och
-- fiktiv utgående moms (2614/2624/2634 efter momssats). NULL i ett utkast betyder att inget är
-- valt och att leverantörens förval föreslås; en registrerad faktura får värdet som bokfördes
-- när den registreras, i samma UPDATE som låser den (L1). Fakturor registrerade före den här
-- migreringen har NULL, vilket läses som ingen. Kolumnen läggs till utan ombyggnad, så att
-- triggrarna som låser en registrerad faktura står kvar.

ALTER TABLE leverantorsfaktura ADD COLUMN omvand_skattskyldighet TEXT
    CHECK (omvand_skattskyldighet IN ('ingen', 'eu_tjanst', 'eu_vara', 'bygg', 'utanfor_eu'));
