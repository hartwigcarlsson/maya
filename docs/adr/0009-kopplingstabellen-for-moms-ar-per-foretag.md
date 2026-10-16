# Kopplingstabellen för moms är per företag

Varje företag har en egen kopplingstabell för moms, som ersätter den gemensamma tabellen i ADR 0008. Tabellen kopieras från Mayas standardtabell när företaget läggs upp, och en användare kan ändra den på kontosidan. Varje konto bidrar till högst en ruta. Deklarationen räknas fortfarande bara ur saldona på kontona, utan momskoder. Med en egen tabell kan ett företag föra momsfall som Maya saknar flöden för, till exempel trepartshandel, på egna konton och koppla dem till rätt ruta. Det kräver inga momskoder och ingen ny standardtabell. En senare Maya-version lägger bara till kopplingar för nya BAS-konton och ändrar aldrig ett befintligt företags kopplingar, på samma sätt som ADR 0007 gör för kontoplanen.

## Considered Options

- **En gemensam inbyggd tabell (ADR 0008)**: enklast, och deklarationen är densamma för alla företag med samma huvudbok. Men ett konto som BAS inte har, eller ett momsfall som standarden inte täcker, går inte att redovisa alls.
- **Tabell per BAS-år**: följer BAS ändringar, men ett befintligt företags konton ändras aldrig av BAS (ADR 0007), så en version per år ger ingenting.
- **Momskoder per verifikatrad**: avvisades redan i ADR 0008.

## Consequences

- Kopplingen ingår i företagets räkenskapsinformation. Varje ändring loggas i behandlingshistoriken (H2), och den koppling som gällde arkiveras med momsrapporten och i årsarkivet (S2). Annars kan deklarationen inte återskapas ur SIE4 och arkivet.
- En ändring gäller alla momsperioder som inte är deklarerade, också en deklaration som öppnas igen. En deklarerad momsperiod räknas aldrig om.
- Kopplingen är låst för konton som Mayas flöden bokför moms eller underlag på, och för reserverade konton. Annars kan en ändring göra flödenas egna verifikat fel i deklarationen.
- Två företag med samma huvudbok kan få olika deklarationer. Momsrapporten visar därför vilka konton som gav varje ruta.
