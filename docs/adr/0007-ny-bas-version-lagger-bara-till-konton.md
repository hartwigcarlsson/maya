# En ny BAS-version lägger bara till konton

Varje företag får en egen kopia av den inbyggda BAS-kontoplanen när det läggs upp. När en senare Maya-version bygger in en ny BAS-årsversion får befintliga företag bara de konton som är nya i BAS, och de är inaktiva. BAS namnbyten och borttagna konton rör aldrig ett befintligt företags konton. Kontoplanen är företagets räkenskapsinformation (S1), och den ska inte ändras i smyg av en uppgradering. Varje tillägg loggas i behandlingshistoriken med Maya som aktör (H2).

## Considered Options

- **Visa en diff som användaren godkänner** (namnbyten, borttagna och nya konton): ger användaren full kontroll, men kräver ett eget flöde för varje uppgradering och för varje företag. Det kan läggas till senare.
- **Rör inte befintliga företag alls**: enklast, men nya BAS-konton blir då oåtkomliga utan att användaren skapar dem som egna konton.
- **Spara bara avvikelser mot den inbyggda versionen**: mindre data, men då ändras företagets kontoplan tyst varje gång BAS gör det.

## Consequences

- Ett företag som lades upp med en äldre BAS-version behåller äldre kontonamn tills någon byter namn på dem själv.
- Ett nummer som någon redan använder för ett eget konto kan senare komma tillbaka i BAS. Då behålls det egna kontot, och BAS-kontot läggs inte till.
