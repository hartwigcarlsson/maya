-- Kreditfakturor till kund (etapp 2, #140).
--
-- En kreditfaktura är en kundfaktura i samma tabell och samma fakturaserie (VF4), med negativa
-- rader och en hänvisning till originalet i `krediterar`. Den skapas som utkast ur ett utfärdat
-- original som inte självt är en kreditfaktura, och hänvisningen ändras aldrig. När den utfärdas
-- är summan negativ och högst det som finns kvar att kreditera: originalets total minus de
-- kreditfakturor som redan är utfärdade. Det som återstår, krediterad och tillgodo räknas fram
-- ur fakturorna och betalningarna och lagras inte. En utfärdad kreditfaktura låses som andra
-- utfärdade fakturor av kundfaktura_utfardad_andras_inte (L1, A1).

ALTER TABLE kundfaktura ADD COLUMN krediterar INTEGER REFERENCES kundfaktura (id);

CREATE INDEX kundfaktura_krediterar ON kundfaktura (krediterar) WHERE krediterar IS NOT NULL;

-- Hänvisningen pekar på en utfärdad faktura i samma företag som inte är en kreditfaktura.
CREATE TRIGGER kreditfaktura_kraver_utfardat_original
BEFORE INSERT ON kundfaktura
WHEN NEW.krediterar IS NOT NULL AND NOT EXISTS (
    SELECT 1 FROM kundfaktura o
    WHERE o.id = NEW.krediterar AND o.foretag_id = NEW.foretag_id
      AND o.status <> 'utkast' AND o.krediterar IS NULL)
BEGIN
    SELECT RAISE(ABORT, 'en kreditfaktura krediterar en utfärdad faktura som inte är en kreditfaktura');
END;

-- Hänvisningen sätts när utkastet skapas och ändras inte, inte heller på ett utkast.
CREATE TRIGGER kreditfaktura_hanvisningen_andras_inte
BEFORE UPDATE OF krediterar ON kundfaktura
WHEN NEW.krediterar IS NOT OLD.krediterar
BEGIN
    SELECT RAISE(ABORT, 'kreditfakturans hänvisning går inte att ändra');
END;

-- Skyddsnätet för spärrarna vid utfärdande: summan är negativ och högst det som finns kvar.
CREATE TRIGGER kreditfaktura_krediterar_hogst_det_som_finns_kvar
BEFORE UPDATE OF status ON kundfaktura
WHEN OLD.status = 'utkast' AND NEW.status <> 'utkast' AND NEW.krediterar IS NOT NULL
  AND (NEW.total >= 0 OR -NEW.total > (
      SELECT o.total + coalesce((
          SELECT sum(k.total) FROM kundfaktura k
          WHERE k.krediterar = o.id AND k.status <> 'utkast'), 0)
      FROM kundfaktura o WHERE o.id = NEW.krediterar))
BEGIN
    SELECT RAISE(ABORT, 'kreditfakturan krediterar mer än som finns kvar att kreditera');
END;
