# Vilka krav ställer BFL och BFNAR på ett bokföringssystem?

Forskningsticket: [#2](https://github.com/hartwigcarlsson/maya/issues/2) (karta: #1). Underlag till beslutstickets #11, #12, #13 och till BFL-kontrollen i AGENTS.md.

Gäller ett aktiebolag som för sin egen bokföring i Maya.

## Källor

Bara primärkällor. Alla hänvisningar nedan pekar på någon av dessa:

- **BFL**: Bokföringslag (1999:1078), ändrad t.o.m. SFS 2024:342, hämtad från Riksdagen: <https://data.riksdagen.se/dokument/sfs-1999-1078.text> (motsvarar <https://www.riksdagen.se/sv/dokument-och-lagar/dokument/svensk-forfattningssamling/bokforingslag-19991078_sfs-1999-1078/>). SFS 2024:342 trädde i kraft 2024-07-01.
- **BFNAR 2013:2**: Bokföringsnämndens allmänna råd om bokföring, ändrade t.o.m. BFNAR 2024:1 (gäller från 2024-09-16). Citeras ur BFN:s vägledning *Bokföring – Vägledning till BFNAR 2013:2*, uppdaterad 2024-09-16: <https://www.bfn.se/wp-content/uploads/vl13-2-bokforing.pdf>. Hänvisningen "p. 5.9" avser punkt 5.9 i det allmänna rådet. "Komm. s. N" avser BFN:s kommentar på sidan N i vägledningen. Kommentaren är inte normerande på samma sätt som de allmänna råden, men den är BFN:s egen tolkning.

Konsoliderade versionen på bfn.se (`bfnar13-2-kons2.pdf`) slutar vid BFNAR 2017:6 och är inaktuell. Använd vägledningen från 2024.

## Sammanfattning

BFL anger *vad* bokföringen ska klara. BFNAR 2013:2 preciserar *hur*. För ett datoriserat system ger de tillsammans sju hårda krav:

1. **Bokförda poster ändras eller raderas aldrig.** Något räknas som bokfört först när det är låst. Ett fel rättas med en ny rättelsepost som har en egen verifikation och visar vem som rättade och när.
2. **Varje post har en verifikation.** Verifikationen har ett obrutet löpnummer i sin serie och innehåller de fält som lagen kräver.
3. **All bokföring kan presenteras både som grundbok och som huvudbok.** Grundboken visar posterna i registreringsordning, huvudboken per konto. I båda ska registreringsordning, period, verifikationsnummer, kontering och belopp gå att läsa ut.
4. **Det finns behandlingshistorik.** Den visar varje post med registreringsdatum och varje systemändring som påverkar hur poster behandlas.
5. **Det finns systemdokumentation.** Den innehåller kontoplan, samlingsplan, arkivplan, beskrivning av nummerserier och automatkonteringar, och en beskrivning av hur allt tas fram på papper.
6. **All räkenskapsinformation bevaras i 7 år.** Underlag sparas i det format de kom in i. Allt förvaras i Sverige, eller i ett annat EU-land om platsen anmäls till Skatteverket. Det finns säkerhetskopior som förvaras åtskilda, och allt kan skrivas ut omedelbart.
7. **Tidsfrister och bokslut hålls.** Kontanta betalningar bokförs senast nästa arbetsdag, övrigt så snart det kan ske. Räkenskapsåret är 12 månader. Aktiebolag avslutar med årsredovisning enligt ÅRL.

---

## 1. Grundläggande skyldigheter

Ett företag ska enligt BFL 4 kap. 1 §:

1. löpande bokföra alla affärshändelser (5 kap. 1–5 §§),
2. ha verifikationer för alla bokföringsposter, samt systemdokumentation och behandlingshistorik (5 kap. 6–11 §§),
3. bevara all räkenskapsinformation och den utrustning och de system som behövs för att presentera den,
4. upprätta öppningsbalansräkning, och
5. avsluta bokföringen enligt 6 kap.

Bokföringen ska följa god redovisningssed (BFL 4 kap. 2 §). BFN ansvarar för att utveckla god redovisningssed (BFL 8 kap. 1 §). Därför är BFNAR 2013:2 i praktiken bindande.

När bokföringsskyldigheten inträder ska en öppningsbalansräkning upprättas utan dröjsmål (BFL 4 kap. 3 §).

Redovisningsvalutan är SEK. Ett aktiebolag får välja euro, och byte får bara ske vid ett nytt räkenskapsår (BFL 4 kap. 6 §). Belopp i annan valuta räknas om till avistakursen den dag affärshändelsen inträffar (p. 2.4). Alternativt får man använda en approximation eller den kurs som mervärdesskattelagen anger (p. 2.5). Systemet behöver inte bokföra i SEK, men det måste kunna *presentera* varje händelse omräknad till redovisningsvalutan (komm. s. 21–22).

Räkenskapsinformation som företaget själv upprättar skrivs på svenska, danska, norska eller engelska (BFL 1 kap. 4 §).

### Definitioner (BFL 1 kap. 2 §)

- **Bokföringspost**: varje enskild notering i grundbokföringen eller huvudbokföringen.
- **Affärshändelse**: varje förändring av förmögenheten som beror på relationer med omvärlden.
- **Verifikation**: de uppgifter som dokumenterar en affärshändelse eller en justering.
- **Räkenskapsinformation** omfattar bland annat:
  - grund- och huvudbok,
  - sidoordnad bokföring,
  - verifikationer och handlingar de hänvisar till,
  - systemdokumentation och behandlingshistorik,
  - årsredovisning eller årsbokslut och specifikationer,
  - avtal av särskild betydelse,
  - övrigt som behövs för att följa posternas behandling.

  Även krypteringsnycklar räknas som räkenskapsinformation (komm. s. 72).

## 2. Varaktighet: inget får raderas

- **Bokföringen ska vara varaktig.** Det som har bokförts får inte kunna raderas eller på annat sätt göras oläsligt (p. 2.1).
- **En post är bokförd först när den är låst.** BFN skriver att preliminär registrering, som kan raderas eller ändras tills man aktivt låser den, inte är bokförd. Bokföringen måste låsas senast när tidsfristerna i kapitel 3 löper ut (komm. s. 15).
- **Låsning krävs i båda presentationerna.** En affärshändelse är bokförd först när noteringen är varaktig i både grundbok och huvudbok (komm. s. 15–16).
- **Varaktigheten gäller hela systemet.** Kravet omfattar också sidoordnad bokföring, till exempel anläggningsregistret (komm. s. 16).
- **Verifikationens obligatoriska uppgifter ska vara varaktiga.** Det gäller även verifikationsnumret (p. 5.4; komm. s. 52).

**Följd för Maya:** Ett utkastläge är tillåtet. Utkast är dock inte bokföring, och låsningen måste ske inom fristerna i avsnitt 5. Låsta poster ändras aldrig, så tabellerna ska vara append-only.

## 3. Grundbok, huvudbok och vad varje post ska visa

- **Två presentationer.** Affärshändelserna ska bokföras så att de kan presenteras i registreringsordning (grundbokföring) och i systematisk ordning (huvudbokföring). Det ska gå att kontrollera att bokföringen är fullständig och att överblicka förlopp, ställning och resultat (BFL 5 kap. 1 §).
- **Registreringsordning per konto och serie.** Inom huvudboken ska registreringsordningen framgå för varje konto. Med flera verifikationsserier ska ordningen framgå inom varje serie (p. 2.2).
- **Fem uppgifter per bokföringspost.** För varje post ska följande gå att läsa ut (p. 2.3):
  1. registreringsordning,
  2. redovisningsperiod (en kalendermånad eller liknande, p. 1.8),
  3. verifikationsnummer,
  4. kontering,
  5. bokfört belopp.
- **Registreringsdatum.** Det behöver inte synas i själva bokföringen, men i datorbaserad bokföring ska det alltid framgå av behandlingshistoriken (komm. s. 19; p. 9.16).
- **Ingen saldo i grundboken.** Grundboken visar ordningsföljd, inte saldon. Saldon visas i huvudboken (komm. s. 18).
- **En samlad huvudbok.** Delsystem, till exempel reskontra eller kassajournal, får finnas. De ska dock ingå i en gemensam systematisk presentation. Flera isolerade huvudböcker är inte tillåtet (komm. s. 18; p. 2.11, 2.20).
- **Sammandrag är tillåtet ibland.** Likartade händelser samma dag, eller händelser på samma mottagna faktura, får bokföras i en post om det utan svårighet går att se vilka händelser posten består av (p. 2.8–2.10).
- **Avräkning via kontokort och automatiska överföringar.** Kontokortsförsäljning med utbetalning inom tre bankdagar, och automatiska överföringar mellan bankkonton som nollas samma dag, får bokföras som en händelse (p. 2.12–2.13).
- **Sidoordnad bokföring.** Konton för tillgångar, avsättningar, skulder och eget kapital ska specificeras i sidoordnad bokföring (reskontra) i den utsträckning som behövs för kontroll och överblick (BFL 5 kap. 4 §).
- **Anläggningsregister.** Ett sådant krävs, om inte bolaget har få anläggningstillgångar och bokföringen ändå ger överblick (p. 4.2). Registret ska innehålla per tillgång:
  - identifiering,
  - anskaffningsvärde och anskaffningstidpunkt,
  - restvärde,
  - avskrivningsmetod om den inte är linjär,
  - nyttjandeperiod,
  - ackumulerade av-, ned- och uppskrivningar,
  - korrigeringar och omklassificeringar,
  - avyttringar.

  Krav på innehållet: p. 4.5–4.7. Registret ska uppdateras senast vid bokslutet (p. 4.4).
- **Avstämning.** Bokföringen ska stämmas av löpande enligt fasta rutiner. Avvikelser och hur de hanterats ska dokumenteras (p. 2.15–2.16).

## 4. Verifikationer

### Grundregler

- **En verifikation per affärshändelse.** Har företaget tagit emot ett underlag ska det användas som verifikation (BFL 5 kap. 6 §).
- **Verifikationer för justeringar.** Även bokslutstransaktioner och rättelser kräver verifikation (BFL 5 kap. 10 §; komm. s. 46).
- **Egen verifikation.** Saknas mottaget underlag ska företaget självt sammanställa en verifikation (p. 5.2).
- **Hänvisningsverifikation.** En sådan är tillåten. Det ska då framgå var originalet finns, antingen av hänvisningsverifikationen eller av systemdokumentationen (BFL 5 kap. 6 § andra st.; p. 5.13–5.14).
- **Verifikation och bokföring hålls isär.** En verifikation får inte samtidigt utgöra presentation i grundbok eller huvudbok. Ett program får ändå registrera uppgifterna en gång och sedan sammanställa dem som både verifikation och bokföring (p. 5.1; komm. s. 47).

### Obligatoriskt innehåll (BFL 5 kap. 7 §)

| Fält | Precisering |
|---|---|
| När verifikationen sammanställdes | Sammanställningsdatum |
| När affärshändelsen inträffade | Affärshändelsedatum |
| Vad den avser | Ska göra det möjligt att identifiera händelsen och förstå vad den innebär (p. 5.5) |
| Belopp | |
| Motpart | Ska göra det möjligt att identifiera motparten (p. 5.6). Får utelämnas vid kontant betalning med anonym kundkrets (p. 5.7; BFL 5 kap. 8 §) |
| Hänvisning till underliggande handlingar och var de finns | I förekommande fall |
| Verifikationsnummer eller annat identifieringstecken | |
| Övriga uppgifter som behövs för sambandet med den bokförda posten | Kontering ska stå på verifikationen om det behövs för sambandet (p. 5.12) |

### Komplettering

En komplettering får inte förändra de mottagna uppgifterna. Det ska framgå när kompletteringen gjordes, vem som gjorde den och vilka uppgifter som lades till (p. 5.8).

### Nummerserier

- **Obruten serie.** Varje serie ska vara obruten under en tidsperiod, utan att nummer hoppas över. Det ska gå att kontrollera i systemet att serien är obruten (p. 5.9).
- **Flera serier är tillåtna.** Kravet är att bokföringen är överskådlig och i god ordning, till exempel med separata serier för kundfakturor, leverantörsfakturor och lön (p. 5.10; komm. s. 52).
- **Seriens längd är fri.** En serie får löpa per räkenskapsår eller över flera år (komm. s. 52).
- **Ingen gemensam serie med andra bolag.** Bara vissa avkastningsstiftelser får dela serie med ett annat företag (komm. s. 53, till p. 5.11).
- **Dokumentation krävs.** Systemdokumentationen ska beskriva hur serierna är indelade och vilken tidsperiod de omfattar, om flera serier används (p. 9.6).

### Gemensam verifikation

Flera händelser får dokumenteras i en gemensam verifikation (BFL 5 kap. 6 § tredje st.; kap. 6 i BFNAR). Det gäller:

- återkommande händelser på en samlingsfaktura för högst en månad,
- automatiskt genererade händelser,
- likartade händelser samma dag (p. 6.1).

För automatiskt genererade händelser får fälten ersättas med datum, period, transaktionstyp, belopp per typ och totalbelopp. Verifikationen ska då hänvisa till systemsammanställningar som visar delbelopp per typ och motpart (p. 6.3).

### Rättelse av verifikation

Det ska anges när rättelsen skedde och vem som gjorde den (BFL 5 kap. 9 §). Den ursprungliga uppgiften ska klart framgå (p. 5.15). En ersatt verifikation är fortfarande räkenskapsinformation (p. 5.16).

## 5. Tidsfrister för löpande bokföring

**Huvudregel (BFL 5 kap. 2 §):**

- Kontanta in- och utbetalningar bokförs senast påföljande arbetsdag. Med det menas första dagen efter händelsen då arbete utförs i verksamheten (p. 3.1). Ligger det mer än några dagar dit gäller dagen efter (p. 3.2).
- Övriga affärshändelser bokförs så snart det kan ske, det vill säga när företaget har tillräckliga uppgifter (p. 3.3).
- Kontanta betalningar omfattar sedlar, mynt och checkar. Kortbetalningar räknas inte dit (p. 1.9).

**Tillåtna senareläggningar (BFNAR kap. 3):**

En förutsättning för alla senareläggningar är att verifikationerna under tiden ordnas i den takt de tas emot eller sammanställs (p. 3.5).

| Regel | Frist | Villkor |
|---|---|---|
| p. 3.4 | Fordringar och skulder bokförs när fakturan utfärdas eller tas emot | Tillämpas konsekvent under året, för både utfärdade och mottagna fakturor |
| p. 3.6 | 50 dagar efter månadens utgång | Gäller inte kontanta betalningar i grundboken |
| p. 3.7 | 50 dagar efter kvartalets utgång | Nettoomsättning normalt högst 3 mkr |
| p. 3.8 | 60 dagar efter räkenskapsårets utgång | Högst 50 verifikationer, högst 250 händelser och högst 1 mkr i omsättning per år |
| p. 3.13 | Senare än ovan | Bara vid särskilda hinder av tillfällig natur |

**Kontantmetoden:**

- Ett företag med en nettoomsättning på normalt högst 3 mkr får bokföra först vid betalning. Obetalda fordringar och skulder måste dock bokföras vid räkenskapsårets utgång (BFL 5 kap. 2 § tredje st.).
- Kontantmetoden kombineras med senareläggningsreglerna. Tiden räknas då från betalningstillfället (p. 3.15).

**Effekt på perioder:**

- Affärshändelser hänförs till rätt redovisningsperiod även om de registreras senare.
- När arbetet med en period är avslutat får den inte längre få nya poster eller rättelser (komm. s. 19–20).
- En period är avslutad när dess händelser är bokförda och avstämningar och rättelser är gjorda.

**Följd för Maya:** Systemet behöver en uttrycklig "stäng period"-åtgärd. Efter den hamnar nya rättelser i den period där felet upptäcktes (komm. s. 27).

## 6. Rättelser och behandlingshistorik

### Rättelser

- **Vem och när.** Den som rättar en bokföringspost ska ange när rättelsen skedde och vem som gjorde den. Görs det med en rättelsepost ska den rättade posten visa att den har rättats (BFL 5 kap. 5 §).
- **Rättelsepost i datoriserad bokföring.** En bokföringspost rättas genom en särskild rättelsepost (p. 2.17). Överstrykning är bara tillåten i manuell bokföring.
- **Egen verifikation.** En rättelsepost är en ny bokföringspost och ska ha en egen verifikation (p. 2.18).
- **Rätt period.** Rättelseposten bokförs i den period där felet upptäcktes. Om perioden fortfarande är öppen bokförs den i felets period (komm. s. 27–28).
- **Synlig koppling.** Kopplingen kan till exempel visas genom att rättelsens verifikationsnummer anges på den rättade postens verifikation (komm. s. 28).

### Behandlingshistorik

- **Lagkravet.** Det ska finnas beskrivningar av genomförda bearbetningar som gör det möjligt att följa och förstå varje bokföringsposts behandling (BFL 5 kap. 11 §).
- **Innehåll.** Historiken ska visa de poster som har tillförts systemet med registreringsdatum. Den ska också beskriva förändringar i systemet som påverkar postbehandlingen och när de infördes (p. 9.16).
- **Exempel på sådana förändringar** (komm. s. 96):
  - ny programversion,
  - nytt delsystem,
  - ändrad momssats eller procentsats för sociala avgifter i automatkonteringen,
  - nytt konto i kontoplanen.

  Ändringar i kundregistret räknas inte dit.
- **Registreringsdatum** är den dag posten tillfördes systemet. Det behöver inte vara samma dag som affärshändelsen (komm. s. 96).

**Följd för Maya:** Behandlingshistoriken ska loggas som en del av databasen. Det gäller både posternas registreringstidpunkt och konfigurationsändringar, som kontoplan, momssatser, automatkonteringsregler och programversion. Själva historiken är räkenskapsinformation och ska sparas i 7 år.

## 7. Systemdokumentation

**Lagkravet:** Det ska finnas beskrivningar av bokföringssystemets organisation och uppbyggnad som ger överblick över systemet (BFL 5 kap. 11 §). Bokföringssystem är system eller delar av system där bokföringsposter och räkenskapsinformation skapas eller bearbetas (p. 9.1).

**Innehåll (p. 9.2):**

1. kontoplan med namn och nummer (p. 9.3),
2. hur kontona sammanställs till årsredovisning, om det inte framgår av kontoplanen,
3. samlingsplan, alltså systemets uppbyggnad och sambanden mellan delarna (p. 9.4),
4. arkivplan (p. 8.3).

**Undantaget för enkla system gäller inte Maya.** Ett enkelt system behöver inte allt ovan. Men ett program med automatkonteringar räknas inte som enkelt (p. 9.5).

**Krävs dessutom när det är tillämpligt:**

- Hur verifikationer identifieras, om flera serier används (p. 9.6).
- Verifieringskedjan från verifikation via grundbok och huvudbok till årsredovisning och tillbaka, om den inte framgår direkt (p. 9.7–9.8).
- Behandlingsregler, om sådana automatiskt påverkar konto eller belopp eller skapar nya poster (p. 9.9–9.10). Exempel är automatkontering, momsuppdelning och periodiseringsmallar.
- Informationsflöden, om systemet består av flera datoriserade delar (p. 9.11). Exempel är bankimport, fakturamodul och e-faktura-växel. En fakturaväxel ska framgå av samlingsplanen (komm. s. 70).
- Hur elektronisk räkenskapsinformation tas fram på papper (p. 9.12). Det gäller även krypterad information (komm. s. 72).
- Hur och var behandlingshistoriken skapas (p. 9.15).
- Hur överföringar mellan format görs utan risk för förändring eller förlust (komm. s. 70).

**Följd för Maya:** En stor del av dokumentationen kan och bör genereras av systemet. Det gäller kontoplan, serieindelning, regler och flöden. Den genererade dokumentationen behöver versioneras, eftersom den också är räkenskapsinformation.

## 8. Arkivering

### Form och format (BFL 7 kap. 1 §)

**Pappers- eller elektronisk form.** Räkenskapsinformation bevaras som pappershandling eller som elektronisk handling. En elektronisk handling ska kunna tas fram på papper genom omedelbar utskrift. Sparad med teknik som inte längre används accepteras viss fördröjning (p. 7.1).

**Originalformatet bevaras.** Elektroniska handlingar ska bevaras i det format och med det innehåll de hade när de kom till företaget eller sammanställdes (ny lydelse genom SFS 2024:342). Formatet är en del av verifikationen. Exempel (komm. s. 46–47, 65–66):

- en mottagen PDF sparas som PDF,
- en e-faktura sparas i sitt originalformat,
- ett mejl eller sms sparas i sitt originalformat.

**Lagringsmedium och plats får bytas fritt.** Det gäller så länge formatet och innehållet är oförändrade (komm. s. 66).

**Egenupprättad information** anses sammanställd när den har fått den form och det format den ska presenteras i. Företaget måste därför ha bestämt presentationsform och format (p. 7.5; komm. s. 67). I datoriserad bokföring är grundbok, huvudbok och behandlingshistorik per automatik elektroniska (komm. s. 68).

**Information hämtad från webbplatser,** till exempel kontoutdrag från internetbanken, får sparas i det format den ska presenteras i, till exempel PDF (p. 7.3).

### Överföring mellan format (BFL 7 kap. 6 §)

**Originalet får förstöras efter överföring.** Det gäller både papper och elektroniska handlingar. Villkoret är att överföringen, med hänsyn till tekniska metoder, organisatoriska åtgärder och övriga omständigheter, inte innebär risk för att informationen förändras eller försvinner.

**Systembyte räknas som överföring.** Ändras formatet vid byte av bokföringssystem är det en överföring. Data i det gamla systemet får bara förstöras om villkoren uppfylls (komm. s. 68).

**BFN:s förtydliganden** (komm. s. 69–70):

- Konvertering i en fakturaväxel är en överföring.
- Före förstöring ska man kontrollera att allt kom med, att det är läsbart och att alla sidor finns.
- Uppgifter som krävs enligt BFL eller mervärdesskattelagen får aldrig gå förlorade. Logotyper, reklam och hälsningar får rensas bort.
- Kan överföringen inte säkerställas ska originalet sparas. Den överförda kopian blir då en hänvisningsverifikation.
- Överföringar ska normalt beskrivas i systemdokumentationen.

### Varaktighet, säkerhet och kryptering

- **Varaktiga och lätt åtkomliga.** Handlingarna ska vara det under hela bevarandetiden (BFL 7 kap. 2 §). Tas en handling emot som inte är varaktig ska den överföras till en som är det (p. 7.7).
- **Betryggande förvaring.** Informationen ska förvaras så att den inte riskerar att förstöras eller förkomma (p. 8.1).
- **Säkerhetskopior.** Elektronisk räkenskapsinformation ska säkerhetskopieras. Hur ofta beror på verksamheten, för vissa företag varje dag och för mycket små kan en gång i månaden räcka (komm. s. 74). **Kopian ska förvaras åtskild från originalet** (p. 8.2).
- **Arkivplan.** En sådan behövs om den krävs för överblick. Den visar vad som arkiverats, var det förvaras och vid behov hur arkivet är uppbyggt (p. 8.3).
- **Kryptering.** Räkenskapsinformation får krypteras bara om det är motiverat av säkerhetsskäl och informationen omedelbart kan visas i klartext (p. 7.8).
  - Före ett systembyte som skulle göra dekryptering omöjlig ska man dekryptera.
  - Krypteringsnycklar är räkenskapsinformation.
  - Kryptering som programvaran gör internt omfattas inte av punkten (komm. s. 72).

### Bevarandetid och plats

- **Bevarandetid.** Handlingarna ska bevaras till och med det **sjunde året efter utgången av det kalenderår då räkenskapsåret avslutades** (BFL 7 kap. 2 §). Exempel: räkenskapsåret 2025-07-01–2026-06-30 sparas till och med 2033-12-31.
- **Förvaras i Sverige.** Det gäller både handlingarna och den utrustning och de system som behövs för att skriva ut dem, i ordnat skick och på ett betryggande och överskådligt sätt (BFL 7 kap. 2 §).
- **Förvaring i ett annat EU-land.** Elektroniska handlingar och system får finnas där om tre villkor är uppfyllda (BFL 7 kap. 3 a §):
  1. platsen och varje ändring av den anmäls till Skatteverket,
  2. Skatteverket och Tullverket på begäran får omedelbar elektronisk åtkomst,
  3. informationen kan skrivas ut omedelbart i Sverige.

  BFN räknar uttryckligen internetbaserade bokföringsprogram med lagring utomlands hit (komm. s. 77). Länder utanför EU med likvärdiga avtal om ömsesidigt bistånd likställs med EU-länder. I övriga fall krävs tillstånd från Skatteverket (BFL 7 kap. 3 a § andra st., 4 §).
- **Förstöring före bevarandetidens slut** kräver tillstånd från Skatteverket (BFL 7 kap. 7 §).

**Följd för Maya:** Self-hosting i Sverige uppfyller platskravet direkt. En säkerhetskopia som synkas till en molntjänst utanför Sverige gör att räkenskapsinformation förvaras utomlands. Inom EU krävs då anmälan till Skatteverket, utanför EU krävs tillstånd. Det är ett beslut för backup-ticketen.

## 9. Räkenskapsår och avslutning

### Räkenskapsår (BFL 3 kap.)

- **Tolv månader.** Ett räkenskapsår omfattar tolv kalendermånader. Ett aktiebolag får ha brutet räkenskapsår (1 §).
- **Första eller sista året** får vara kortare, eller förlängas till högst 18 månader. Det gäller vid start, vid omläggning och när bokföringsskyldigheten upphör (3 §).
- **Omläggning** kräver Skatteverkets tillstånd. Undantaget är omläggning till kalenderår eller till koncerngemensamt år (6 §).
- **Koncerner.** Bolag i samma koncern ska ha gemensamt räkenskapsår (5 §).

### Avslutning

- **Bokslutstransaktioner.** När den löpande bokföringen avslutas bokförs de poster som behövs för att bestämma årets intäkter och kostnader och ställningen på balansdagen (BFL 5 kap. 3 §; p. 2.19). Exempel är periodiseringar och avskrivningar.
- **Årsredovisning.** Ett aktiebolag ska alltid avsluta med en årsredovisning som upprättas och offentliggörs enligt årsredovisningslagen (BFL 6 kap. 1 § 1, 2 §). Reglerna om årsbokslut i BFL 6 kap. 3–7 §§, bland annat sexmånadersfristen i 7 §, gäller företag som *inte* upprättar årsredovisning. Fristerna för aktiebolagets årsredovisning finns i ÅRL och ABL, som ligger utanför denna ticket.
- **Specifikation.** Varje sammandragen balanspost i årsredovisningen ska specificeras i en särskild förteckning, om sammansättningen inte framgår av bokföringen (BFL 6 kap. 8 §). Förteckningen behöver inte offentliggöras men är räkenskapsinformation.

---

## Checklista: krav på Maya

Formulerad för att kunna användas direkt i beslutstickets och som BFL-kontroll i AGENTS.md. "Måste" är krav enligt BFL eller BFNAR. "Bör" är BFN:s rekommendation eller en följd för Maya.

### Bokföringsposter och låsning

- [ ] **L1** En bokförd post (verifikation och dess rader) är oföränderlig. Ingen kodväg får uppdatera eller radera låsta poster (BFL 5:1, 7:2; p. 2.1).
- [ ] **L2** Utkast får finnas, men räknas inte som bokfört. Låsning sker senast inom tillämplig tidsfrist (komm. s. 15; BFL 5:2; BFNAR kap. 3).
- [ ] **L3** Varje post visar registreringsordning, redovisningsperiod, verifikationsnummer, kontering och belopp (p. 2.3).
- [ ] **L4** Systemet kan ta fram hela bokföringen i registreringsordning (grundbok) och per konto i registreringsordning (huvudbok) (BFL 5:1; p. 2.2).
- [ ] **L5** Det finns en enda samlad huvudbok. Delsystem som reskontra och kassa bokförs in i den (komm. s. 18; p. 2.11).
- [ ] **L6** Redovisningsvalutan är SEK, eller EUR. Utländska belopp kan alltid presenteras omräknade till avistakurs eller tillåten kurs (BFL 4:6; p. 2.4–2.5).
- [ ] **L7** Varje verifikation är balanserad, det vill säga debet är lika med kredit. Detta är en följd av dubbel bokföring och huvudboken, inte en uttrycklig regel.

### Verifikationer

- [ ] **V1** Varje bokföringspost har en verifikation, även bokslutstransaktioner och rättelser (BFL 5:6, 5:10; p. 2.18).
- [ ] **V2** Verifikationen har fälten sammanställningsdatum, affärshändelsedatum, beskrivning, belopp, motpart, verifikationsnummer och referenser till underlag och var de finns. Motpart får utelämnas enligt p. 5.7 (BFL 5:7–8; p. 5.5–5.7).
- [ ] **V3** Verifikationsnummer tilldelas obrutet inom varje serie, utan luckor. Systemet kan visa att serien är obruten (p. 5.9).
- [ ] **V4** Serierna och deras tidsperioder är konfigurerade och dokumenterade (p. 5.10, 9.6).
- [ ] **V5** Mottagna underlag, alltså originalfiler, kopplas till verifikationen och sparas oförändrade i originalformat (BFL 5:6, 7:1).
- [ ] **V6** En komplettering av en verifikation loggar när den gjordes, av vem och vad som lades till. Det mottagna innehållet förändras aldrig (p. 5.8).
- [ ] **V7** En rättad eller ersatt verifikation sparas. Ursprunget framgår, liksom vem som rättade och när (BFL 5:9; p. 5.15–5.16).

### Rättelser och perioder

- [ ] **R1** Rättelser görs alltid med en ny rättelsepost som har en egen verifikation. Det gäller även makulering (p. 2.17–2.18).
- [ ] **R2** Rättelsen anger vem som rättade och när. Den rättade posten länkar till rättelsen och visar att den har rättats (BFL 5:5).
- [ ] **R3** Redovisningsperioder, som månader, kan stängas. Efter stängning bokförs rättelser i den öppna period där felet upptäcktes (komm. s. 19–20, 27).
- [ ] **R4** Systemet stöder och dokumenterar avstämningar, och avvikelser kan noteras (p. 2.15–2.16).

### Tidsfrister

- [ ] **T1** Systemet hjälper till att hålla fristerna. Kontanta betalningar bokförs senast påföljande arbetsdag, övrigt så snart det kan ske eller enligt vald senareläggning i p. 3.6–3.8. Låsning sker senast vid fristen (BFL 5:2).
- [ ] **T2** Om kontantmetoden stöds ska obetalda fordringar och skulder bokföras vid räkenskapsårets slut (BFL 5:2 tredje st.).

### Behandlingshistorik

- [ ] **H1** Registreringstidpunkt och användare sparas för varje bokföringspost (p. 9.16; BFL 5:5).
- [ ] **H2** Ändringar som påverkar postbehandlingen loggas med tidpunkt. Det gäller kontoplan, momssatser, automatkonteringsregler, serieinställningar, nya delsystem och programversion (p. 9.16; komm. s. 96).
- [ ] **H3** Behandlingshistoriken går inte att ändra och sparas lika länge som övrig räkenskapsinformation (BFL 1:2 p. 9, 7:2).

### Systemdokumentation

- [ ] **S1** Det finns kontoplan, beskrivning av hur konton sammanställs till årsredovisningen, samlingsplan och arkivplan (p. 9.2–9.4, 8.3).
- [ ] **S2** Automatkonteringar och andra behandlingsregler, som moms och periodiseringar, är dokumenterade (p. 9.9–9.10). Maya räknas inte som "enkelt system" (p. 9.5).
- [ ] **S3** Verifieringskedjan och informationsflödena (import, e-faktura, bank) är beskrivna (p. 9.7–9.8, 9.11).
- [ ] **S4** Det är beskrivet hur all elektronisk räkenskapsinformation tas fram på papper (p. 9.12).
- [ ] **S5** Det är beskrivet hur överföringar och konverteringar mellan format görs utan risk för förändring (komm. s. 70; BFL 7:6).
- [ ] **S6** Systemdokumentationen är versionerad och arkiveras som räkenskapsinformation (BFL 1:2 p. 9).

### Arkivering

- [ ] **A1** All räkenskapsinformation bevaras till och med 31 december det sjunde året efter det kalenderår då räkenskapsåret slutade. Systemet får inte erbjuda radering före det (BFL 7:2, 7:7).
- [ ] **A2** Allt kan presenteras i läsbar form och skrivas ut omedelbart, till exempel som PDF- eller utskriftsexport av grundbok, huvudbok, verifikationer, underlag och behandlingshistorik (BFL 7:1).
- [ ] **A3** Systemet och utrustningen som behövs för presentationen hålls tillgängliga hela bevarandetiden. Arkiverade år ska därför vara läsbara även efter uppgraderingar och schemamigreringar (BFL 7:2).
- [ ] **A4** Format- och systembyten, inklusive schemamigrering som ändrar format och export eller import till ett annat system, behandlas som överföringar. De ska verifieras, till exempel med checksummor och avstämning av summor, innan original kasseras (BFL 7:6; komm. s. 68–70).
- [ ] **A5** Det finns automatisk säkerhetskopiering, och kopian förvaras åtskild från originalet (p. 8.2).
- [ ] **A6** Data och kopior förvaras i Sverige. Förvaring i ett annat EU-land kräver anmälan till Skatteverket, förvaring utanför EU kräver tillstånd. Skatteverket ska kunna få omedelbar elektronisk åtkomst (BFL 7:2, 7:3 a, 7:4).
- [ ] **A7** Kryptering av vilande data är tillåten bara om klartext kan tas fram omedelbart. Nycklarna bevaras som räkenskapsinformation och dekryptering är möjlig under hela bevarandetiden (p. 7.8; komm. s. 72).

### Räkenskapsår och bokslut

- [ ] **Å1** Räkenskapsåret är 12 månader. Kalenderår eller brutet år kan väljas, och det första året får vara mellan 1 och 18 månader (BFL 3:1, 3:3).
- [ ] **Å2** Öppningsbalans registreras när bokföringen startar (BFL 4:3).
- [ ] **Å3** Bokslutstransaktioner bokförs med verifikation. Årsredovisningen enligt ÅRL och specifikationer av sammandragna balansposter kan tas fram och arkiveras (BFL 5:3, 6:1–2, 6:8).
- [ ] **Å4** Anläggningsregister med fälten i p. 4.5–4.7, uppdaterat senast vid bokslutet. Det kan utelämnas bara om bolaget har få tillgångar och bokföringen ändå ger överblick (p. 4.2–4.7).
- [ ] **Å5** Räkenskapsinformation som Maya genererar är på svenska eller engelska (BFL 1:4).

## Öppna frågor (utanför denna ticket)

- **Bokslut och årsredovisning för aktiebolag:** frister, K2 eller K3 och digital inlämning till Bolagsverket. Det regleras i ÅRL, ABL och BFNAR 2016:10/2012:1, inte i BFL eller BFNAR 2013:2.
- **Moms, arbetsgivaravgifter och SIE-export:** krav från Skatteverket i mervärdesskattelagen (2023:200) och skatteförfarandelagen (2011:1244). SIE är branschstandard men inget lagkrav.
- **Hur "omedelbar utskrift" och "systemen hålls tillgängliga" ska tolkas vid self-hosting under 7+ år:** till exempel om det räcker med en fryst release och en exportbar PDF-eller-PDF/A-arkivfil per år.
- **Om en schemamigrering i SQLite som inte ändrar exporterat innehåll räknas som formatändring enligt 7 kap. 6 §, eller bara som byte av lagringsmedium.** BFN:s kommentar talar om "format" på handlingsnivå (komm. s. 66, 68). Maya bör ha en exportnivå som är stabil över versioner.
