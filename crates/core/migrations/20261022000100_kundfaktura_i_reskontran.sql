-- Det som återstår av varje kundfaktura som inte är ett utkast, i öre (etapp 2, granskningen av
-- #93), som `leverantorsfaktura_i_reskontran` för leverantörsfakturorna: totalen minus det som
-- utfärdade eller registrerade kreditfakturor har krediterat, det som är betalt (med
-- öresutjämning) och det som en gällande kundförlust har skrivit bort. Negativt är ett
-- tillgodohavande. En kreditfaktura räknas av mot sitt original och har 0 kvar.
--
-- Regeln fanns två gånger, i Rust (`betalning::återstår_i`) och i listans SQL
-- (`kundfaktura::lista_i`); nu läser båda vyn. Vyn ändrar inga bokförda poster (A3, A4).
CREATE VIEW kundfaktura_i_reskontran AS
SELECT x.id, x.foretag_id, x.status, x.total, x.krediterat, x.betalt, x.forlorat,
       CASE WHEN x.krediterar IS NOT NULL THEN 0
            ELSE x.total - x.krediterat - x.betalt - x.forlorat
       END AS aterstar
FROM (
    SELECT f.id, f.foretag_id, f.status, f.total, f.krediterar,
           coalesce((SELECT -sum(c.total) FROM kundfaktura c
                     WHERE c.krediterar = f.id AND c.status IN ('utfardad', 'registrerad')), 0)
               AS krediterat,
           coalesce((SELECT sum(b.belopp + b.oresutjamning + b.kursdifferens)
                     FROM betalning_i_reskontran b WHERE b.kundfaktura_id = f.id), 0)
               AS betalt,
           coalesce((SELECT sum(l.belopp) FROM kundforlust_i_reskontran l
                     WHERE l.kundfaktura_id = f.id), 0)
               AS forlorat
    FROM kundfaktura f
    WHERE f.status <> 'utkast'
) x;
