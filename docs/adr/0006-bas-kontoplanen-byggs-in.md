# BAS-kontoplanen byggs in i Maya, med grundkontona aktiva

Maya levererar BAS-kontoplanen ur BAS-kontogruppens fria utgåva (PDF/XLSX), med kontonummer och kontonamn, som en del av källkoden i det publika repot. Varje nytt företag får hela planen som mall. Bara grundkontona är aktiva från början, och övriga konton finns med men är inaktiva tills någon aktiverar dem. Grundkontona är de som BAS markerar med ■ i den senaste fria utgåvan som har markeringen (BAS 2020 eller senare), mappade mot den inbyggda årsversionens konton. Konton som har tillkommit efter den utgåvan är inaktiva. Den fria utgåvan saknar uttalad licens, och vi bygger in den utan att först fråga BAS-kontogruppen. Det är en medveten risk som vi tar för att en ny användare ska få en färdig kontoplan utan att behöva leta upp och läsa in en fil.

## Considered Options

- **Inläsning vid körning** (driftaren laddar ner BAS-filen själv, eller läser in SIE4 eller en egenköpt JSON-fil): det var bedömningen i utredningen om BAS-kontoplanen. Ingen BAS-data hamnar i repot, men varje driftare måste göra ett extra steg innan första företaget kan läggas upp.
- **Vänta på skriftligt besked från BAS** innan något byggs in: det säkraste, men svaret kan dröja och kan bli nej.
- **Köpa JSON-filen och checka in dess `isBasic`-urval**: urvalet är exakt för 2026, men punkt 4.5 i BAS allmänna villkor förbjuder spridning. Då vore det ett brott mot ett avtal vi själva har ingått, och inte bara en oklar licens.
- **Ett eget urval av aktiva konton för aktiebolag**: kan passa bättre, men då måste Maya underhålla listan själv vid varje ny årsversion.

## Consequences

- Om BAS-kontogruppen invänder måste kontonamnen tas bort ur repot. Då återstår inläsning vid körning, och företag som redan är uppsatta behåller sina konton.
- BAS 2026 markerar inte grundkonton i den fria utgåvan. Därför blir urvalet äldre än kontoplanen, och nya grundkonton måste aktiveras för hand.
- Den fria XLSX-filen har en utskriftslayout och måste tolkas om för varje årsversion när en ny BAS-version byggs in.
- Ingen köpt BAS-data (JSON-filen) får ligga i repot.
