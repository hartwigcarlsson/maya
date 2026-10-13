# Webb, MCP, CLI och bakgrundsjobb når data bara via `core`

All läsning och skrivning av bokföringsdata går genom användningsfallen i craten `core`. Leptos server functions, MCP-verktygen, CLI:t och bakgrundsjobben är tunna adaptrar som anropar dem och kör aldrig sqlx själva. Varje användningsfall tar en aktör och ett företag och gör behörighetskontroll, BFL-regler, numrering och behandlingshistorik i en och samma SQLite-transaktion. Därför kan ingen kanal gå förbi kraven: en ny MCP-funktion kan inte råka uppdatera ett bokfört verifikat (L1) eller skriva utan aktör i behandlingshistoriken (H1). Middleware autentiserar och gör om sessionscookie eller OAuth-token till en aktör. `core` auktoriserar, även regler som att kanalen MCP bara får skapa utkast. Reglerna som inte kräver IO (belopp, kontonummer, balanskontroll, moms) ligger i craten `domain`, som även kompilerar till WASM, så att formulären validerar med samma regler som servern.

## Considered Options

- **Adaptrarna anropar sqlx direkt med delade hjälpfunktioner**: mindre kod per funktion, men varje ny server function eller MCP-verktyg måste själv komma ihåg behörighet, låsning och behandlingshistorik. Valdes bort eftersom BFL-kraven då hänger på disciplin i varje adapter.
- **Behörighet i middleware**: valdes bort eftersom middleware inte vet vilket företag eller verifikat ett anrop rör, och CLI och bakgrundsjobb går inte genom HTTP.
