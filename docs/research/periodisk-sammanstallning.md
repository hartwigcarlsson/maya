# Vilket filformat har den periodiska sammanställningen?

Researchanteckning för issue #58 (map #1), bakgrund i beslut 11 på [#18](https://github.com/hartwigcarlsson/maya/issues/18). Hämtad 2026-10-14. Endast primärkällor: skatteverket.se, Skatteverkets Utvecklarportal (API-katalogen) och riksdagen.se (SFS-text). Källor listas längst ned och hänvisas till som [P1]–[P8].

## Kort svar

- **Format:** en semikolonavgränsad textfil (`.txt` eller `.csv`), inte XML. Rad 1 är identifieraren `SKV574008;`, rad 2 är ett huvud (eget momsreg-nr, periodkod, kontaktperson, telefon, valfri e-post) och rad 3 och framåt har en rad per köpare: `VAT-nr;varor;trepartshandel;tjänster;` [P1].
- **Periodkod:** `ÅÅMM` för månad (t.ex. `2301`) och `ÅÅ-K` för kalenderkvartal (t.ex. `23-1`) [P1]. Tjänster redovisas per kalenderkvartal, varor per månad, och båda tillsammans per månad [P2][P6 35 kap. 3 §].
- **Teckenkodning och radslut** anges inte för den här filen. Det får testas (se *Öppna frågor*).
- **Inlämning:** e-tjänsten *Filöverföring*. Användaren loggar in med e-legitimation, väljer filtyp *Periodisk sammanställning*, lägger till filen och trycker *Överför*. Det finns inget separat signeringssteg. Överföringen är inlämningen och ger en mottagningskvittens [P1][P3].
- **Rättelse:** en ny fil för samma period. Raderna slås ihop per VAT-nummer med det som redan lämnats: en rad ersätter den tidigare raden med samma VAT-nummer, ett borttaget VAT-nummer rättas med beloppet `0`, och VAT-nummer som inte finns med i den nya filen ligger kvar oförändrade [P1][P4].
- **API:** det finns inget. Utvecklarportalens katalog har 27 API:er och inget av dem gäller periodisk sammanställning [P7]. Avtalsfrågan uppstår därför inte. Filöverföring kräver bara e-legitimation, och Skatteverket har en testtjänst för filer som inte kräver inloggning [P1].

## 1. Lagrum

Skatteförfarandelagen (2011:1244), 35 kap. [P6]:

- **2 §:** den som är momsregistrerad ska lämna periodisk sammanställning över (1) varor som transporteras mellan EU-länder och (2) tjänster som är skattepliktiga i köparens EU-land där köparen är betalningsskyldig enligt artikel 196 i mervärdesskattedirektivet. Uppgifterna tas upp för den period som omfattar tidpunkten i 7 kap. 29 eller 30 § ML (2023:200).
- **3 §:** kalendermånad för varor, kalenderkvartal för tjänster, och kalendermånad när både varor och tjänster ska redovisas. Den som går över från kvartal till månad ska behålla månad till och med det fjärde kalenderkvartalet efter bytet.
- **4–8 §§:** beslut om kvartal för varor (högst 500 000 kr per kvartal) och om månad för tjänster (på begäran eller av särskilda skäl). Ett beslut om månad gäller från ingången av det kvartal då beslutet fattades, och redan passerade månader ska då redovisas var för sig (8 §).
- **9 §:** sammanställningen ska ha kommit in senast den 25:e i månaden efter perioden om den lämnas elektroniskt, annars den 20:e.
- **48 kap. 1 och 6 §§:** förseningsavgiften är 1 250 kr.

Skatteverket skriver också att ingen sammanställning ska lämnas för en period utan försäljning [P2].

## 2. Filformat

Källa för hela avsnittet är [P1] (den engelska versionen [P5] säger samma sak) om inget annat anges.

### 2.1 Allmänt

- Textfil med semikolon som avgränsare. Den får bara innehålla text, och ändelsen är `.txt` eller `.csv`.
- **Filnamn:** valfritt, men bara `A–Z`, `a–z` och `0–9`. Det betyder inga bindestreck, understreck eller punkter i namnet utöver ändelsen.
- **Teckenkodning och radslut:** anges inte. Skatteverkets sida om Filöverföring för inkomstdeklarationer (SRU) kräver ISO 8859-1 [P8], men den gäller ett annat filformat och kan inte läsas in här.

### 2.2 Rad 1: identifierare

`SKV574008;` Saknas den eller är den fel avvisas filen. (Den engelska guiden skriver `SKV547008` i felmeddelandelistan [P5], vilket är en felskrivning. Formatbeskrivningen och exemplen har `SKV574008`.)

### 2.3 Rad 2: uppgiftsskyldig och period

Fälten i ordning:

| # | Fält | Format |
|---|---|---|
| 1 | Den uppgiftsskyldiges momsregistreringsnummer | 12 siffror, med eller utan `SE` (t.ex. `556000016701`) |
| 2 | Månads- eller kvartalskod | `ÅÅMM` (t.ex. `2212`, `2301`) eller `ÅÅ-K` (t.ex. `22-4`, `23-1`) |
| 3 | Namn på den som ansvarar för uppgifterna | högst 35 tecken |
| 4 | Telefonnummer till den ansvarige | bara siffror, bindestreck efter riktnumret eller inledande `+` för utlandsnummer, högst 17 tecken |
| 5 | E-postadress till den ansvarige | frivillig |

### 2.4 Rad 3 och framåt: en rad per köpare

| # | Fält | Format |
|---|---|---|
| 1 | Köparens VAT-nummer med landskod | högst 14 tecken inklusive landskod |
| 2 | Värde av varuleveranser | högst 12 siffror inklusive minustecken |
| 3 | Värde av trepartshandel | som ovan |
| 4 | Värde av tjänster enligt huvudregeln | som ovan |

- Ett tomt fält (bara semikolonet) betyder ingen försäljning av det slaget.
- Ett negativt belopp, t.ex. vid kreditering, skrivs med minustecken framför. Blir summan 0 kr anges `0`, och så tas också en tidigare redovisad försäljning bort.
- Belopp är heltal: "andra tecken än siffror" i ett beloppsfält ger en varning (2.6). Värdet är det sammanlagda fakturerade beloppet i svenska kronor per köpare, inklusive ersättning för andra kostnader utom ränta [P2].
- Varje VAT-nummer ska bara förekomma en gång per sammanställning. Varor, trepartshandel och tjänster till samma köpare står på samma rad [P2].
- Avropslager (koderna `X`, `Y` och `Z` i fält 2, utan belopp) och överföring till egen verksamhet i ett annat EU-land rör bara varor och är inte aktuella för Maya.

### 2.5 Exempel

Ur [P1] Exempel 1 (varor, trepartshandel, tjänster och avropslager):

```
SKV574008;
556000016701;2001;Per Persson;0123-45690; post@filmkopia.se
FI01409351;16400;;;
DK31283522;21700;13600;;
ESA28480514;3200;;15300;
DE262875793;X;;
```

Varje rad slutar med semikolon. En sammanställning med bara tjänster för första kvartalet 2027 skulle därför se ut så här (konstruerat enligt reglerna ovan, inte ett exempel från Skatteverket):

```
SKV574008;
SE556000016701;27-1;Anna Andersson;08-123456;
DE123456789;;;15300;
FI01409351;;;-2000;
```

(Exemplet har ett blanksteg före e-postadressen. Om fält trimmas är okänt, så Maya bör inte skriva något blanksteg.)

### 2.6 Kontroller i e-tjänsten Filöverföring

Kontrollerna vid överföringen gäller bara att obligatoriska uppgifter finns och är formellt riktiga. Fler kontroller görs senare, när uppgifterna läses in i systemet för periodiska sammanställningar [P1].

**Fel som avvisar filen** (ingenting sparas):

- identifieraren `SKV574008` saknas eller är fel
- den uppgiftsskyldiges momsreg-nr är formellt fel eller saknas
- periodkoden är fel, saknas eller avser en framtida period
- kontaktnamnet saknas eller är längre än 35 tecken
- telefonnumret saknas, har andra tecken än `0–9`, `-` och `+`, eller är längre än 17 tecken
- ett VAT-nummer är längre än 14 tecken
- ett beloppsfält har fler än 12 siffror
- (bara avropslager) landskod utanför EU eller formellt felaktigt VAT-nummer

**Varningar** (användaren väljer att avbryta eller lämna in ändå):

- periodkoden är inte den förväntade, t.ex. en tidigare period än den senast avslutade månaden eller det senast avslutade kvartalet
- e-postadressen är formellt fel
- köparens landskod avser inte ett annat EU-land
- köparens VAT-nummer är formellt fel (fel längd eller kontrollsiffra enligt landets regler)
- dubblettrader, alltså samma VAT-nummer på mer än en rad
- belopp saknas på en rad, eller ett beloppsfält har andra tecken än siffror
- "Medgivande krävs" när varor redovisas per kvartal utan beslut

**Kontroller i efterhand** [P4]: Skatteverket kontrollerar att VAT-numren var giltiga under perioden och skickar en fråga om ett VAT-nummer inte följer det andra landets regler, saknar belopp eller har en landskod utanför EU. Frågan besvaras i e-tjänsten *Periodisk sammanställning för moms*.

**Testa filer:** en testversion av tjänsten där ingen e-legitimation behövs och som annars är identisk med Filöverföring [P1][P5].

### 2.7 Inlämning och kvittens

Efter inloggning med e-legitimation väljer användaren filtyp (*Periodisk sammanställning* eller *Inkomstdeklaration*), därefter *Överföra filer* → *Välj fil* → *Lägg till* → *Överför*. Filerna laddas upp en i taget. Kvittensen visar datum och klockslag, filnamn, momsreg-nr, periodkod och eventuella felanmärkningar. Under *Kvittenslista* finns tidigare överförda filer [P5][P1].

Det finns inget separat signeringssteg, till skillnad från momsdeklarationen via fil. Den som vill att någon annan ska lämna sammanställningen behöver inte anmäla något ombud. Behörigheten *Periodisk sammanställning, svara på förfrågan* behövs bara för att svara på Skatteverkets frågor och se detaljerade kvittenser [P9].

## 3. Rättelse

- Den som har lämnat fel uppgifter ska så fort som möjligt lämna en ny periodisk sammanställning för samma period [P4]. Via fil betyder det en ny fil med samma periodkod [P1].
- **Raderna slås ihop, de ersätts inte som helhet** [P4]:
  - Ett nytt belopp för ett VAT-nummer anges som periodens **totala** belopp, inte som en skillnad. Den rättade raden ersätter den tidigare raden med samma VAT-nummer.
  - En rad tas bort genom att `0` anges i den kolumn som tidigare hade ett belopp. Gäller rättelsen en annan köpare läggs dessutom en ny rad till.
  - Glömd försäljning lämnas i en ny sammanställning med bara de nya raderna. Tidigare lämnade uppgifter ska inte skickas in igen, eftersom de nya läggs ihop med de gamla.
- Sidan beskriver den sammanslagna modellen för e-tjänsten och blanketten, och filsidan hänvisar dit. Att en fil som innehåller alla rader igen gör samma sak följer av att varje rad ersätter raden med samma VAT-nummer, men det står inte uttryckligen.
- Är perioden ett kvartal går det inte att i efterhand lämna en månad inom kvartalet via e-tjänsten. Det får då göras på papper eller via fil. Lämnas en månad av misstag i stället för kvartalet ska alla månader i det kvartalet lämnas [P4].
- Rättas den periodiska sammanställningen kan momsdeklarationen också behöva rättas, eftersom beloppen i de flesta fall ska stämma överens [P4].

## 4. API

- Utvecklarportalens katalog (`/portal-wapi/open/apier-och-oppna-data/utvecklarportalen/v1/api`, hämtad 2026-10-14) har 27 API:er, bland dem Momsdeklaration, Arbetsgivardeklaration – inlämning, Skattekonto och Kundhändelser, men inget för periodisk sammanställning [P7].
- Det finns alltså ingen API-väg och ingen avtalsfråga. Filöverföring och den manuella e-tjänsten är de enda digitala vägarna, och blanketten SKV 5740 finns kvar för papper (den beställs) [P1][P2].

## 5. Konsekvenser för Maya

1. **Filen är enkel CSV**, inte eSKD-XML. Maya skriver `SKV574008;`, huvudraden och en rad per köpare med summerat tjänstebelopp i fält 4 (fält 2 och 3 tomma). Varje rad avslutas med semikolon.
2. **Huvudraden kräver kontaktperson och telefon.** Namn (högst 35 tecken) och telefon (högst 17 tecken, `0–9`, `-`, `+`) är obligatoriska och avvisar filen om de saknas. Maya behöver de uppgifterna per företag eller användare och måste validera dem innan filen skapas.
3. **Aggregera per VAT-nummer och period.** Summera kundfakturor och kreditnotor per köparens VAT-nummer i hela kronor. Avrundningen till hela kronor är inte beskriven, så Maya väljer en regel. Rader med summan noll behöver inte skrivas vid en första inlämning.
4. **Validera lokalt** det som e-tjänsten avvisar eller varnar för: VAT-nummer högst 14 tecken, landskod i ett annat EU-land, inga dubbletter, belopp högst 12 siffror, ingen period i framtiden. VAT-nummerkontrollen bör ske redan när kunden läggs upp i kundreskontran.
5. **Rättelse kräver diff-semantik.** Eftersom raderna slås ihop per VAT-nummer kan Maya inte bara skicka den nya sammanställningen. Fungerar det som beskrivs i avsnitt 3 räcker det att skriva hela den nya mängden **plus en `0`-rad för varje VAT-nummer som fanns i den förra inlämningen men inte längre finns**. Maya måste därför spara vad som lämnades in senast för perioden (den arkiverade filen räcker).
6. **Ingen signering:** beslut 11 i #18 säger "laddar upp och signerar", men i Filöverföring är *Överför* själva inlämningen. Statusen *inlämnad* markeras fortfarande manuellt, och mottagningskvittensen kan sparas som underlag.
7. **Periodlogiken:** standard är kvartalskoden `ÅÅ-K`. Har företaget ett beslut om månadsredovisning för tjänster (35 kap. 5 § SFL) gäller `ÅÅMM`. Skulle Maya senare hantera varor blir perioden månad för allt (35 kap. 3 § andra stycket).
8. **Ingen API-integration** att planera för. Testa filer-tjänsten fungerar utan inloggning och kan användas för att verifiera formatet manuellt under implementationen. Den är en webbsida, inte ett API.

## Öppna frågor

1. **Teckenkodning.** Anges inte för den här filen. Namnet i huvudraden kan innehålla å/ä/ö. Testa ISO 8859-1 och UTF-8 (med och utan BOM) i Testa filer innan formatet låses.
2. **Radslut och blanksteg.** CRLF eller LF, avslutande radbrytning, och om blanksteg runt fälten trimmas (Skatteverkets eget exempel har ett blanksteg före e-postadressen). Testa i Testa filer.
3. **`SE`-prefix på eget momsreg-nr.** Båda formerna är tillåtna. Välj en och verifiera den.
4. **Rättelse via fil, ersätts den eller slås den ihop?** Sidan om rättelse beskriver sammanslagning per VAT-nummer, men filsidan säger bara "en ny fil för samma period". Bekräfta med Skatteverket eller i kvittensen att en rättelsefil fungerar som beskrivet, särskilt att `0` tar bort en rad.
5. **Öresavrundning.** Fälten tar bara heltal. Ska Maya avrunda per faktura eller per köpare och period? Och ska summan stämma mot ruta 39 efter samma avrundning (rimlighetsvarningen i beslut 11)?
6. **Kontaktuppgifterna.** Vem står som ansvarig, inloggad användare eller en fast kontaktperson per företag, och var lagras telefonnumret?
7. **Kvittensen.** Den visas i webbläsaren. Går den att spara som fil (PDF) så att den kan arkiveras som underlag till statusen *inlämnad*?

## Källor

- [P1] Skatteverket, *Lämna periodisk sammanställning med filöverföring*. https://www.skatteverket.se/foretag/moms/deklareramoms/periodisksammanstallning/lamnaperiodisksammanstallningmedfiloverforing.4.7eada0316ed67d72822104.html
- [P2] Skatteverket, *Periodisk sammanställning för varor och tjänster*. https://www.skatteverket.se/foretag/moms/deklareramoms/periodisksammanstallningforvarorochtjanster.4.58d555751259e4d661680001093.html
- [P3] Skatteverket, e-tjänsten Filöverföring (inloggning och Testa filer). https://www1.skatteverket.se/fv/fv_web/login.do
- [P4] Skatteverket, *Rätta en periodisk sammanställning*. https://www.skatteverket.se/foretag/moms/deklareramoms/rattaenperiodisksammanstallning.4.7856a2b411550b99fb7800088048.html
- [P5] Skatteverket, *How to submit an EC Sales List (recapitulative statement) via the file transfer e-service*. https://www.skatteverket.se/sdginstruktioner/howtosubmitarecapitulativestatementviathefiletransfereservice.4.7da1d2e118be03f8e4f52df.html
- [P6] Skatteförfarandelag (2011:1244), 35 kap. 1–9 §§ och 48 kap. 1, 6 §§. https://www.riksdagen.se/sv/dokument-och-lagar/dokument/svensk-forfattningssamling/skatteforfarandelag-20111244_sfs-2011-1244/
- [P7] Skatteverket, Utvecklarportalen, API-katalog (JSON). https://www7.skatteverket.se/portal-wapi/open/apier-och-oppna-data/utvecklarportalen/v1/api
- [P8] Skatteverket, *Filöverföring* (Inkomstdeklaration 1 och 2, SRU-format). https://www.skatteverket.se/foretag/etjansterochblanketter/allaetjanster/tjanster/filoverforing.4.1f604301062bf0c47e8000527.html
- [P9] Skatteverket, *Ombudsbehörigheter för ett företag*. https://www.skatteverket.se/foretag/drivaforetag/ombudforettforetag/ombudsbehorigheterforettforetag.4.16f588619a39f2c14a1920.html
