# BAS-kontoplanen i Maya: licens, maskinläsbar källa och kopplingar

Research för [#7](https://github.com/hartwigcarlsson/maya/issues/7). Underlaget hämtades 2026-10-12. Källorna är bas.se (BAS-kontogruppen i Stockholm AB), Skatteverket och riksdagen.se. Påståenden utan källhänvisning är markerade som **bedömning**.

## Sammanfattning

1. **Gratisfilerna (PDF/XLSX) har ingen uttalad licens.** BAS publicerar kontoplanen gratis, men varken sidan eller filerna säger något om rätten att sprida eller bygga in den. Den enda licenstext som finns gäller den köpta JSON-filen.
2. **Den maskinläsbara JSON-filen kostar 4 000 kr exkl. moms per årsversion.** Licensen tillåter att den byggs in i ett affärssystem för redovisning som vidarelicensieras. Den förbjuder däremot att filen sprids på annat sätt. Ett publikt git-repo med filen i går alltså inte.
3. **JSON-filen innehåller SRU-fältkoder per konto (INK2 m.fl.) men inga momsrutor.** Någon officiell, gratis koppling mellan BAS-konton och momsdeklarationens rutor finns inte. Den kopplingen får Maya definiera själv.
4. **SRU-fältkoderna är raderna i INK2R, och INK2R följer ÅRL:s uppställning.** BAS gratis-tabell för INK2 kan därför användas som både SRU-koppling och grov gruppering för balans- och resultatrapport.

## 1. Får BAS-kontoplanen användas och distribueras i Maya?

### Gratisfilerna

- På sidan "Kontoplaner" går BAS 2026 att ladda ner gratis som PDF och XLSX ("Fullständig", för alla företagsformer utom de som upprättar förenklat årsbokslut). Kontoplanen 2026 finns också på engelska som PDF. Sidan säger att kontoplanen "(kontonummer och kontonamn) inom BAS-konceptet kan laddas ner här". [bas.se/kontoplaner](https://www.bas.se/kontoplaner/)
- Varken sidan, PDF:en eller XLSX-filen innehåller någon licens-, upphovsrätts- eller användningstext. Sidfoten anger bara utgivaren, BAS-kontogruppen i Stockholm AB. Detta kontrollerades genom textextraktion ur [BAS_kontoplan_2026_v2.pdf](https://www.bas.se/wp-content/uploads/2026/04/BAS_kontoplan_2026_v2.pdf) och [BAS_kontoplan_2026_v2.xlsx](https://www.bas.se/wp-content/uploads/2026/04/BAS_kontoplan_2026_v2.xlsx). Inte heller "Frågor och svar" tar upp licens. Där står bara att kontoplanen "kan du ladda ner gratis". [bas.se/fragor-och-svar](https://www.bas.se/fragor-och-svar/)
- BAS beskriver kontoplanen som en utgångspunkt som varje företag anpassar: "Utifrån BAS-kontoplanen kan varje företag skapa en egen kontoplan efter sina särskilda behov och önskemål". [Skapa en egen kontoplan](https://www.bas.se/kontoplaner/skapa-en-egen-kontoplan/)
- **Bedömning:** Att något är gratis att ladda ner betyder inte att det får spridas vidare. Utan en licens gäller upphovsrättslagens normala regler, och en sammanställning som en kontoplan kan ha katalogskydd ([URL 49 §](https://www.riksdagen.se/sv/dokument-och-lagar/dokument/svensk-forfattningssamling/lag-1960729-om-upphovsratt-till-litterara-och_sfs-1960-729/)). Om Maya ska skeppa BAS:s kontonamn i ett publikt repo krävs ett skriftligt besked från BAS-kontogruppen. Det är inte juridisk rådgivning.

### Den maskinläsbara JSON-filen (Allmänna villkor 2026-04-14)

Produktsidan säger att filen säljs till programvaruföretag, levereras som JSON enligt en bifogad specifikation och kostar "4 000 SEK (ex.25% moms)". [Kontoplanen i maskinläsbart format](https://www.bas.se/kontoplaner/kontoplanen-i-maskinlasbart-format/)

Villkoren ([Allmanna-Villkor-2026.pdf](https://www.bas.se/wp-content/uploads/2026/04/Allmanna-Villkor-2026.pdf)) säger följande:

| Punkt | Innehåll |
|---|---|
| 1.2 | Avtalet ingås med ett företag ("Användaren") genom ett webbformulär. |
| 2.1 | Filen får användas "för eget bruk i Användarens interna verksamhet eller vidarelicenseras till tredje part genom integrering med av Användaren tillhandahållna affärssystem". All annan användning är förbjuden. |
| 3.1–3.2 | Betalning sker via Stripe och leverans via e-post. När en ny årsversion kommer får den gamla inte användas för att utveckla nya produkter eller i produkter där den nya versionen ska användas. |
| 4.1–4.2 | Rätten att använda filen kommersiellt är tidsbegränsad enligt 3.2. Rätten till eget bruk i den egna verksamheten gäller utan tidsgräns. Båda rätterna är icke-exklusiva och icke-överlåtbara. |
| 4.3 | Den som tillhandahåller affärssystem för redovisning får integrera filen för vidarelicensering. Mindre anpassningar, till exempel förkortade kontonamn, och egna underkonton är tillåtna. |
| 4.4 | Rätten gäller bara Användaren, inte andra bolag i samma koncern. |
| 4.5 | Användaren får inte "obehörigen kopiera, sprida, eller bearbeta" innehållet, och får inte låta obehöriga tredje parter få tillgång till filen utöver vad 4.3 tillåter. |
| 4.6 | BAS-kontogruppen får granska användningen. |
| 6.1–6.2 | BAS-kontogruppen äger alla rättigheter ("Ingen överlåtelse av upphovsrätt sker"). Upphovsrättsmärkningar får inte tas bort. |
| 8–9 | Filen levereras i befintligt skick. Det finns inget löfte om underhåll eller uppdateringar, och BAS ansvarar inte för skador. |

**Bedömning för Maya:** Maya är ett publikt GitHub-repo utan licens och self-hostas av andra än upphovspersonen. Att checka in JSON-filen i repot vore att "sprida" den till vem som helst, vilket punkt 4.5 förbjuder. Det skulle inte räknas som "integrering med av Användaren tillhandahållna affärssystem" för licensierade kunder (4.3). Det är bara säkert att använda JSON-filen om en enskild driftare köper den själv och läser in den i sin egen instans (4.2, eget bruk). Den läses då in vid körning och finns aldrig i repot.

## 2. Finns det en maskinläsbar källa per år?

| Källa | Format | Pris | Struktur | Version |
|---|---|---|---|---|
| BAS_kontoplan_2026_v2.xlsx | XLSX, ett blad "Kontoplan 2026" | gratis | Utskriftslayout med fyra kolumner: huvudkonto-nr, namn, underkonto-nr, namn. Kontoklass och kontogrupp står som rubrikrader. `#` efter numret betyder "Konton som inte ska användas när K2 tillämpas". | "v 1.1", ändrad 2026-04-14 |
| listOfAccounts.json | JSON, UTF-8 | 4 000 kr exkl. moms | 1 283 konton med 14 fält per konto | Årsversion |
| "Jämför kontoplaner" | Excel med skillnader mot föregående år | gratis | — | Per år, från BAS 2005 |

Källor: [bas.se/kontoplaner](https://www.bas.se/kontoplaner/), XLSX och PDF enligt ovan. Hänvisningen till ändringsfilen står överst i [BAS 2026-PDF:en](https://www.bas.se/wp-content/uploads/2026/04/BAS_kontoplan_2026_v2.pdf).

Gratis-XLSX:en går att tolka maskinellt, men den är en utskriftslayout och inte en datafil. Den saknar till exempel en kolumn för typ (huvud-/underkonto). Det framgår bara av vilken kolumn numret står i och av rubrikrader. Den markerar inte heller basuppsättningen. BAS beskriver basuppsättningen som de konton som är "markerade med en fyrkant ■" ([Skapa en egen kontoplan](https://www.bas.se/kontoplaner/skapa-en-egen-kontoplan/)), och äldre utgåvor har den markeringen (t.ex. [BAS 2020](https://www.bas.se/wp-content/uploads/2017/06/Kontoplan_Normal_2020.pdf)). Varken 2026-PDF:en eller 2026-XLSX:en innehåller däremot något ■. Det kontrollerades med textsökning. I 2026 finns basuppsättningen bara som `isBasic` i JSON-filen. **Bedömning:** En tolkare måste skrivas om för varje år.

### JSON-formatet ([Kontoplan-Dataformat.pdf](https://www.bas.se/wp-content/uploads/2026/04/Kontoplan-Dataformat.pdf), mars 2026)

- Filen är `{"accounts": [...]}` med 1 283 kontoobjekt sorterade på kontonummer.
- Varje konto har dessa fält:
  - `accountNumber`, `swedishName`, `englishName`
  - `isBasic` (380 konton), `allowedInK2` (1 253 konton)
  - `validForOrganizationTypes`, till exempel "Alla företagsformer" eller "Aktiebolag"
  - `normalTransaction` och `normalBalance` ("Debet", "Kredit" eller "Debet och kredit")
  - `newInstruction`, `isSubAccount`
  - `subAccounts`, `relatedAccounts`, `contraAccounts` (motkonton att föreslå vid bokföring)
  - `SRU`, ett objekt `{NE, INK2, INK3, INK4}` med fyrsiffriga fältkoder, eller `null` om koden inte gäller.
- Fälten är `null` när värde saknas. Tomma arrayer förekommer aldrig.
- Formatet har **inga fält för momsdeklarationens rutor**.

## 3. Kopplingar

### 3a. BAS-konto till SRU-fältkod (inkomstdeklaration 2)

- BAS och Skatteverket har gemensamt tagit fram kopplingsscheman: "Av kopplingsschemat framgår det till vilken rad i räkenskapsschemat som saldot på ett BAS-konto ska föras." Kopplingen görs med fyrsiffriga *fältkoder*, som tidigare kallades SRU-koder och då var tresiffriga. Ett schema är knutet till företagsform och eventuellt en granskningsperiod. [Vad är SRU-kopplingar?](https://www.bas.se/kontoplaner/sru/vad-ar-sru-kopplingar/)
- BAS publicerar kopplingstabellerna gratis som PDF och XLSX för INK2, INK3, INK4 och NE, både med enskilda konton och med kontointervall. [bas.se/kontoplaner/sru](https://www.bas.se/kontoplaner/sru/) Den aktuella INK2-filen är [INK2_P1-231002_241119.xlsx](https://www.bas.se/wp-content/uploads/2024/11/INK2_P1-231002_241119.xlsx). Den har kolumnerna "Fält-kod", "Rad Ink 2", "Benämning" och "Konton i BAS 2023". Exempel:
  - `7201`, rad 2.1, "10xx (exkl. 1088)"
  - `7281`, rad 2.26 "Kassa, bank och redovisningsmedel", "19xx"
  - `7410`, rad 3.1 "Nettoomsättning", "30xx-37xx"
  - `7450` och `7550`, rad 3.26 och 3.27, "+ 899x" respektive "– 899x". Vilket fält som används beror på saldots tecken. Samma mönster med ett plusfält och ett minusfält finns i INK2R för bland annat rad 3.2, 3.23 och 3.24.
  
  Tabellen anger kontonas version som BAS 2023, trots att den senast uppdaterades 2024-11-19. **Bedömning:** Den följer alltså inte varje kontoplansår.
- Skatteverket äger fältkoderna. Fältnamnstabellen för `INK2R` (blankett SKV2002, "Sid 2-3 (räkenskapsschema)") listar till exempel `7201` som "2.1 Immateriella anläggningstillgångar …" och `7281` som "2.26 … Kassa, bank och redovisningsmedel". Tabellerna ligger i den zip som publiceras för varje beskattningsperiod ([2025P4](https://www.skatteverket.se/download/18.4e10387a1997767288d809/1762857356185/_Nyheter_from_beskattningsperiod_2025P4.zip), som innehåller `INK2R_SKV2002-33-01-24-04.xls`). Länkar per år finns på [Teknisk information om filöverföring](https://www.skatteverket.se/foretag/inkomstdeklaration/forredovisningsbyraer/tekniskinformationomfiloverforing.4.13948c0e18e810bfa0cca8.html).
- SRU-filerna lämnas som `INFO.SRU` och `BLANKETTER.SRU`. Den senare har posterna `#BLANKETT`, `#IDENTITET`, `#UPPGIFT <fältkod> <värde>`, `#BLANKETTSLUT` och `#FIL_SLUT`. INK2 är ett "Utkast … som sedan måste signeras på Mina Sidor". Källor: samma sida och Nyheter-PDF:en i zip-filen.

### 3b. BAS-konto till balans- och resultatrapport

- BAS är byggt för "automatiserad framställning av resultaträkning och balansräkning enligt årsredovisningslagens uppställningsformer". [Allmänt om kontoplanen](https://www.bas.se/kontoplaner/allmant-om-kontoplanen/)
- Enligt ÅRL 3 kap. 3 § ska balansräkningen upprättas enligt bilaga 1 och resultaträkningen enligt bilaga 2 eller 3. [ÅRL (1995:1554)](https://www.riksdagen.se/sv/dokument-och-lagar/dokument/svensk-forfattningssamling/arsredovisningslag-19951554_sfs-1995-1554/)
- BAS egna tabeller som visar "var varje enskilt konto hamnar i årsredovisningen samt i inkomst- och momsdeklarationen" finns i de betalda e-böckerna Bokföringsboken och Bokslutsboken. [Skapa en egen kontoplan](https://www.bas.se/kontoplaner/skapa-en-egen-kontoplan/), [Produkter](https://www.bas.se/produkter/)
- **Bedömning:** INK2R:s räkenskapsschema (rader 2.1–2.50 och 3.1–3.27) har samma rubriker och ordning som balansräkningen och den kostnadsslagsindelade resultaträkningen: immateriella, materiella och finansiella anläggningstillgångar, varulager, nettoomsättning och så vidare. Den gratis INK2-kopplingen räcker därför till en balans- och resultatrapport för ett AB, med rubriker och delsummor ur ÅRL:s bilagor. Den räcker inte till en fullständig årsredovisning, men årsredovisning ligger utanför Mayas scope enligt #1.

### 3c. BAS-konto till momsdeklarationens rutor

- Skatteverket definierar rutorna och deras XML-taggar för filinlämning. Exempel: ruta 05 `ForsMomsEjAnnan`, 10 `MomsUtgHog`, 11 `MomsUtgMedel`, 12 `MomsUtgLag`, 20–24 (förvärv med omvänd skattskyldighet), 30–32 (utgående moms på dessa), 35–42 (momsfri och omvänd försäljning), 48 `MomsIngAvdr` och 49 `MomsBetala`. Filen är XML, `eSKDUpload Version="6.0"`, ISO-8859-1, och beloppen anges i hela kronor. Sidan nämner inte BAS-konton. [Lämna momsdeklaration via fil](https://www.skatteverket.se/foretag/moms/deklareramoms/skapaochskickainmomsdeklarationviafil.4.2fb39afe18dabf1e4d223cc.html)
- BAS:s kontonamn speglar momssatserna, till exempel 2611 "Sverige, 25 %" under 2610 "Utgående moms, 25 %", 2650 "Redovisningskonto för moms" och 3001 "Försäljning inom Sverige, 25 % moms" ([BAS 2026-PDF](https://www.bas.se/wp-content/uploads/2026/04/BAS_kontoplan_2026_v2.pdf)). Någon fri, maskinläsbar tabell från konto till ruta publicerar dock varken BAS eller Skatteverket. Den finns bara i Bokföringsboken (se 3b).
- **Bedömning:** Maya måste äga momskopplingen själv. Det kan ske som en egen tabell från konto (eller kontointervall) till ruta, eller som en momskod per konteringsrad. Ett konto räcker inte för vissa rutor. Till exempel delar ruta 05 och rutorna 35–42 ofta försäljningskonton i 30xx, och ruta 20–24 avser underlag och inte momskonton.

## Konsekvenser för Maya (bedömning)

- Checka inte in BAS-data (JSON eller kontonamn ur XLSX) i repot förrän BAS-kontogruppen skriftligt har godkänt spridning i ett öppet repo.
- Det realistiska alternativet är en import vid körning. Driftaren läser in kontoplanen från någon av dessa källor:
  - en SIE4-fil från tidigare system (`#KONTO`), som är företagets egen kontoplan
  - BAS gratis-XLSX som driftaren själv har laddat ner
  - en egenköpt JSON-fil
- Om JSON-filen köps ger den SRU-koppling, K2-flagga, basuppsättning och förslag på motkonton gratis på köpet. Momskopplingen och rapportgrupperingen behöver ändå vara Mayas egna.
- SRU-kopplingen bör versioneras per beskattningsperiod och blankettutgåva, till exempel INK2R SKV2002-33-01-24-04. Kontoplanen bör versioneras per BAS-år. De två ändras i olika takt.

## Öppna frågor

1. Tillåter BAS-kontogruppen att BAS-kontonamn (gratisutgåvan) ingår i ett publikt, gratis och self-hostat program? Kräver ett mejl till BAS (kontaktuppgifter på [bas.se](https://www.bas.se/)).
2. Räknas ett self-hostat open source-projekt utan säljande part som "affärssystem för redovisning" enligt punkt 4.3, så att licensen kan köpas en gång av projektet?
3. Vilken kontoplanskälla ska Maya förvalt använda för ett nytt företag: SIE-import, användarens uppladdade XLSX eller en egen minimal kontoplan med bara kontonummer?
4. Ska momsrutorna kopplas till konto eller till momskod per konteringsrad? Påverkar #18.
