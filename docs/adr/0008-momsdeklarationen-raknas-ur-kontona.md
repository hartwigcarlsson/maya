# Momsdeklarationen räknas ur kontona och spärrar momsperioden

Maya räknar varje ruta i momsdeklarationen ur saldona på kontona via en kopplingstabell (konto → ruta). Tabellen var först densamma för alla företag, men är sedan ADR 0009 företagets egen. Verifikatrader har inga momskoder och konton inga momsattribut. Ett kontos förvalda moms förifyller bara momsraden vid konteringen och påverkar aldrig rutorna. BAS har redan skilda konton för varje momsfall (3001–3003 per momssats, 3108 varor och 3308 tjänster till EU, 4535–4537 tjänster från EU, 2614 och 2645 osv.), och Mayas egna fakturaflöden bokför på dem. Därför räcker huvudboken för att återskapa deklarationen, också ur SIE4-exporten och årsarkivet, som saknar momskoder. När en momsperiod *deklareras* bokför Maya momsavräkningen på periodens sista dag och skapar filen, men låser inga månader. Avräkningen nollar exakt de 26xx-konton som är kopplade till en momsruta, så att redovisningskontot för moms stämmer mot ruta 49. I stället kan inget verifikat som berör ett kopplat konto bokföras i momsperioden förrän en användare uttryckligen *öppnar deklarationen igen* i webben, och då vänds avräkningen genom en rättelse. En månad kan inte låsas förrän dess momsperiod är deklarerad. När den väl är låst (ADR 0001) kan deklarationen inte öppnas igen, och fel som upptäcks senare hamnar i nästa momsperiod.

## Considered Options

- **Momskod per verifikatrad**: exakt även för udda fall, men varje manuellt verifikat, varje utkast från AI och varje importerat verifikat måste kodas, och SIE4 bär inga koder.
- **Deklarera låser momsperiodens månader**: enklare, men en bortglömd faktura kan då aldrig komma med i en ny deklaration för samma period.
- **Ändrad deklaration för låsta perioder**, där verifikat märks "avser momsperiod X": då är deklarationen inte längre bara saldot på kontona, och nästa period måste räkna bort samma belopp.

## Consequences

- Ett verifikat på ett ovanligt konto hamnar i den ruta som intervallet anger. Kopplingstabellen och dess versioner är systemdokumentation (S2).
- Med helårsmoms kan inga månader låsas förrän året är deklarerat, för ett kalenderår alltså tidigast i februari året efter.
- Att öppna en deklaration igen bokför en rättelse och följer därför kanalpolicyn: det görs bara i webben, aldrig av AI.

## Rättelser

- [Hur fyller Maya momsrutorna som kontona inte ger?](https://github.com/hartwigcarlsson/maya/issues/72) rättade två fel: 3108 är varor till ett annat EU-land (ruta 35), och tjänster till EU bokförs på 3308 (ruta 39). Avräkningen omfattade 2610–2649, alltså också vilande moms. Samma beslut gjorde kopplingstabellen företagsspecifik (ADR 0009).
