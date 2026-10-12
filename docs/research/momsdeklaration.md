# Hur lämnas en momsdeklaration till Skatteverket digitalt?

Researchanteckning för issue #4 (map #1). Hämtad 2026-10-12. Endast primärkällor: skatteverket.se (inkl. Utvecklarportalens publicerade PDF:er) och riksdagen.se (SFS-text). Källor listas längst ned; hänvisningar i texten är [K1]–[K10].

## Kort svar

Det finns tre digitala vägar, och **alla slutar med att en behörig person signerar med e-legitimation hos Skatteverket**; ingen väg låter programvara skicka in deklarationen helt maskinellt:

1. **Manuell inmatning** i e-tjänsten *Lämna momsdeklaration* [K1].
2. **XML-fil (eSKDUpload 6.0)** som laddas upp i samma e-tjänst, granskas och signeras där [K1][K2].
3. **API Momsdeklaration** (partner-API, REST/JSON, OAuth2): programvaran läser in ett *deklarationsutkast* till den momsregistrerades *Eget utrymme*, låser det och får tillbaka en djuplänk där en behörig person granskar och skriver under [K4][K5]. Kräver avtal med Skatteverket som "Programvaruföretag" [K5].

Papper finns kvar men måste beställas [K1].

För Maya är fil-vägen den som går att bygga utan avtal: generera en eSKD-XML, användaren laddar upp och signerar. API-vägen ger en bättre upplevelse men kräver att någon (Maya-operatören?) blir avtalspart, se *Öppna frågor*.

## 1. Vägar att lämna

### 1.1 E-tjänsten Lämna momsdeklaration (manuellt)

- Skatteverket listar tre sätt: via bokföringsprogrammets API, i e-tjänsten *Lämna momsdeklaration* (ladda upp XML eller fyll i själv), eller på papper som numera måste beställas via *Beställ blanketter* [K1].
- Deklaration ska lämnas även när det inte finns något att redovisa; då redovisas 0 ("Deklarera noll") [K1][K3].
- Förseningsavgift vid sen deklaration: 625 kr [K1].
- Momsen ska vara betald och bokförd på skattekontot senast samma dag som deklarationen ska vara inne [K1][K6].

### 1.2 Filöverföring: eSKDUpload 6.0 (XML)

Källa för hela avsnittet: [K2] om inget annat anges.

- Kräver e-legitimation och behörighet att företräda den momsregistrerade. Filen laddas upp i *Lämna momsdeklaration* → *Deklarera via fil*, kontrolleras, visas i formuläret, kan rättas manuellt och signeras sedan; signering ger en kvittens. Uppgifterna kan också sparas utan att skickas.
- **Format:** XML, `encoding="ISO-8859-1"`, rotelement `<eSKDUpload Version="6.0">`. Sidan publicerar ingen DTD/XSD; formatet definieras av tabellerna och exemplen på sidan.
- **Header (obligatorisk):**
  - `<OrgNr>` – person-/samordnings-/organisations-/representantnummer, 10 siffror med bindestreck, `xxxxxx-xxxx`.
  - `<Moms>` öppnar momsdelen.
  - `<Period>ÅÅÅÅMM</Period>` – månad: aktuell månad; kvartal: kvartalets sista månad (t.ex. `202503`); helår: beskattningsårets sista månad (t.ex. `202512`).
- **Belopp:** heltal kronor, endast siffror och ev. inledande `+`/`-`. `MomsBetala` (ruta 49) är obligatorisk; moms att få tillbaka anges med minustecken direkt före beloppet.
- **Ruta 48** anges utan tecken; minustecken bara om en minskning av tidigare avdrag ger netto ingående moms att betala tillbaka.
- `<TextUpplysningMoms>` – fri upplysning, max 300 tecken. Kontaktuppgifter kan inte lämnas via fil.
- **Avslut:** `</Moms>` och `</eSKDUpload>`.
- **Avvisande fel** (filen kan inte laddas upp): saknade obligatoriska uppgifter, fel format på OrgNr/period, OrgNr som uppladdaren inte är behörig för, period som inte är en redovisningsperiod för den registrerade, fel beloppsformat, saknad summering, ej XML, felaktiga mellanslag/tabbar, otillåtna tecken, felaktigt angiven tagg.
- **Stoppande fel** (motstridiga/saknade uppgifter) kan sparas men måste rättas före inlämning. **Rimlighetskontroller** varnar men hindrar inte.
- **Rättelse:** lämna en ny fil för perioden med *hela* deklarationen, inte bara ändrade fält.

Minimalt exempel (ur [K2], Exempel 1):

```xml
<?xml version="1.0" encoding="ISO-8859-1"?>
<eSKDUpload Version="6.0">
<OrgNr>556000-0175</OrgNr>
<Moms>
<Period>202401</Period>
<ForsMomsEjAnnan>100000</ForsMomsEjAnnan>
<MomsUtgHog>25000</MomsUtgHog>
<MomsIngAvdr>1000</MomsIngAvdr>
<MomsBetala>24000</MomsBetala>
</Moms>
</eSKDUpload>
```

**Obs, taggordning:** sidans tabell numrerar raderna i rutordning (05–12, 20–24, 30–32, 35–42, 50, 60–62, 48, 49), men sidans *Exempel 6* ("samtliga fält") har en annan ordning: alla underlag (05–08, 20–24, 50, 35–42) först och därefter alla momsbelopp (10–12, 30–32, 60–62, 48, 49) [K2]. Eftersom "en XML-tagg har angetts på ett annat sätt än det format som avsnittet Skapa en fil beskriver" är ett avvisande fel [K2], bör den ordning som faktiskt valideras verifieras i e-tjänsten (se *Öppna frågor*).

### 1.3 API Momsdeklaration (partner-API)

Källor: tjänstebeskrivning v1.0, dokumentversion 1.5 (2025-11-18) [K4], allmänna villkor v1.0 gällande fr.o.m. 2025-11-20 [K5], utvecklingsområdessidan [K7].

- **Status:** "Komplett testtjänst och driftsatt tjänst publicerad" [K4]. Typ: partner-API [K4].
- **Funktion:** utkast förbereds i användarens programvara och läses in till *Eget utrymme*; "Endast digital underskrift behöver därefter ske hos Skatteverket och tjänsten tillhandahåller en djuplänk dit" [K4 §2.2]. Det går inte att skriva under via API:et [K4 §2.5.3][K5 §1.1]. Uppgiftsskyldigheten är inte fullgjord förrän behörig person skrivit under, och programvaran är skyldig att tala om det för användaren [K5 §5a].
- **Utkastets livslängd:** tas bort 30 dagar efter inläsning om ingen arbetat med det eller signerat; tas bort vid underskrift [K4 §2.2].
- **Operationer** [K4 §4.1.1]:

  | Operation | Metod och resurs |
  |---|---|
  | Spara deklarationsutkast (valfritt med lås) | `POST /utkast/{redovisare}/{redovisningsperiod}` |
  | Hämta utkast (ett / flera) | `GET /utkast/{redovisare}/{period}` / `POST /utkast` |
  | Radera utkast | `DELETE /utkast/{redovisare}/{period}` |
  | Kontrollera utkast utan att spara | `POST /kontrollera/{redovisare}/{period}` |
  | Lås utkast (returnerar signeringslänk) | `PUT /las/{redovisare}/{period}` |
  | Lås upp utkast | `DELETE /las/{redovisare}/{period}` |
  | Hämta inlämnade uppgifter (kvittensnummer, tidpunkt, vem som signerat) | `GET /inlamnat/{redovisare}/{period}` / `POST /inlamnat` |
  | Hämta beslutade uppgifter (senaste beslut, endast fastställd moms) | `GET /beslutat/{redovisare}/{period}` / `POST /beslutat` |

- **Identifierare:** `redovisare` = 12 siffror utan bindestreck, med sekelsiffror (`16` för juridiska personer, t.ex. `165020000013`); `redovisningsperiod` = `ÅÅÅÅMM`, periodens sista månad [K4 §4.1.1.2]. Perioder äldre än sex år avvisas [K4 §4.1.1.4].
- **Fältnamn:** API:et har egna JSON-namn per ruta (t.ex. `momspliktigForsaljning` = 05, `inkopTjansterEU` = 21, `summaMoms` = 49) med en mappning till XML-taggarna; se tabellen i avsnitt 2 [K4 §4.1.1.3]. Exakta JSON-scheman finns i RAML-filer på Utvecklarportalen, inte i den publika PDF:en [K4 §4.1.1.2, §5].
- **Kontroller:** svar innehåller `kontrollresultat` med status `OK`/`WARNING`/`ERROR`; utkast med ERROR kan inte låsas eller signeras [K4 §4.1.1.2]. Formella kontroller (ERROR) FK001–FK006 kräver att underlag och momsbelopp förekommer parvis (05–08 ↔ 10–12, 20–24 ↔ 30–32, 50 ↔ 60–62) och FK009 att ruta 49 = utgående moms − ingående moms. Rimlighetskontroller (WARNING) RK002–RK011 varnar bl.a. om moms på omvänd skattskyldighet/import är under 5 % eller över 30 % av underlaget, om momsen avviker mer än 4 000 kr från underlag × skattesats, eller om ett belopp överstiger 1 000 000 000 kr [K4 §4.1.1.5].
- **Tecken för ruta 48:** utan tecken för moms att återfå; minus endast om minskning av tidigare avdrag ger ingående moms att betala tillbaka [K4 §2.3.2].
- **Säkerhet:** OAuth2, scope `momsdeklaration`. Antingen Authorization Code Grant (Client ID + Client Secret, slutanvändaren med e-legitimation eller organisationscertifikat) eller Client Credentials Grant (Client ID + Secret med organisationscertifikat). Header `skv_client_correlation_id` vid alla anrop [K4 §1.3, §3.3].
- **Behörighet för att hantera utkast:** behörig firmatecknare, deklarationsombud, "Momsdeklaration, ombud" eller representant för enkla bolag/partrederi. Ombud med enbart läsbehörighet kan hämta men inte hantera utkast [K4 §2.3.1][K5 §4.2].
- **Gränser:** 4 anrop/s per konsument; max 100 redovisare och 3 perioder per POST-fråga [K4 §3.4].
- **Anslutning:** sandbox med test-API-nycklar och dynamisk testdata begärs via formulär; produktion kräver avtal med Skatteverket och tilldelade produktionsnycklar [K4 §3.1–3.2]. Kontakt: icc@skatteverket.se [K4 §3.6].
- **Krav på avtalsparten ("Programvaruföretaget")** [K5 §4.1–4.2]: registrerat i aktiebolags-/handelsregister (eller enskild näringsidkare med F-/FA-skatt), momsregistrerat, F-skattegodkänt, utan skulder hos Kronofogden; personuppgifter från API:et ska tas emot i en teknisk miljö inom EU/EES; avtalet får inte överlåtas [K5 §13.2].
- **Närliggande API:er:** *Kundhändelser* talar om när och för vilken period en deklaration ska lämnas och om den har lämnats; *Beskattningsengagemang* ger momsregistreringsuppgifter [K4 §2.5].

## 2. Rutorna i deklarationen

Rutornas innehåll är från [K3]; XML-taggar från [K2]; API-namn från [K4 §4.1.1.3]. Lagstöd: en mervärdesskattedeklaration ska innehålla uppgift om utgående skatt, ingående skatt samt leveranser, förvärv och överföringar av varor mellan EU-länder (26 kap. 21 § SFL) [K6].

| Ruta | Innehåll (förkortat) | XML-tagg | API-namn |
|---|---|---|---|
| 05 | Momspliktig försäljning i Sverige exkl. moms (som inte ingår i 06–08); även EU-försäljning till privatpersoner/köpare utan giltigt VAT-nr | `ForsMomsEjAnnan` | `momspliktigForsaljning` |
| 06 | Momspliktiga uttag | `UttagMoms` | `momspliktigaUttag` |
| 07 | Beskattningsunderlag vid vinstmarginalbeskattning | `UlagMargbesk` | `vinstmarginal` |
| 08 | Hyresinkomster vid frivillig beskattning | `HyrinkomstFriv` | `hyresInkomst` |
| 10 / 11 / 12 | Utgående moms 25 / 12 / 6 % på 05–08 | `MomsUtgHog` / `MomsUtgMedel` / `MomsUtgLag` | `momsForsaljningUtgaende{Hog,Medel,Lag}` |
| 20 | Inköp av varor från annat EU-land (unionsinternt förvärv) | `InkopVaruAnnatEg` | `inkopVarorEU` |
| 21 | Inköp av tjänster från annat EU-land enligt huvudregeln | `InkopTjanstAnnatEg` | `inkopTjansterEU` |
| 22 | Inköp av tjänster från land utanför EU | `InkopTjanstUtomEg` | `inkopTjansterUtanforEU` |
| 23 | Inköp av varor i Sverige där köparen är betalningsskyldig | `InkopVaruSverige` | `inkopVarorSE` |
| 24 | Övriga inköp av tjänster i Sverige där köparen är betalningsskyldig | `InkopTjanstSverige` | `inkopTjansterSE` |
| 30 / 31 / 32 | Utgående moms 25 / 12 / 6 % på 20–24 (köparen räknar själv) | `MomsInkopUtgHog` / `MomsInkopUtgMedel` / `MomsInkopUtgLag` | `momsInkopUtgaende{Hog,Medel,Lag}` |
| 35 | Varuförsäljning till annat EU-land (köpare med giltigt VAT-nr) | `ForsVaruAnnatEg` | `forsaljningVarorEU` |
| 36 | Varuförsäljning utanför EU (export) | `ForsVaruUtomEg` | `forsaljningVarorUtanforEU` |
| 37 | Mellanmans inköp vid trepartshandel | `InkopVaruMellan3p` | `inkopVaror3pHandel` |
| 38 | Mellanmans försäljning vid trepartshandel | `ForsVaruMellan3p` | `forsaljningVaror3pHandel` |
| 39 | Tjänsteförsäljning till beskattningsbar person i annat EU-land, huvudregeln | `ForsTjSkskAnnatEg` | `forsaljningTjansterEU` |
| 40 | Övrig försäljning av tjänster tillhandahållna utomlands | `ForsTjOvrUtomEg` | `ovrigForsaljningTjansterUtanforSE` |
| 41 | Försäljning där köparen är betalningsskyldig i Sverige (t.ex. byggtjänster) | `ForsKopareSkskSverige` | `forsaljningBskKopareSE` |
| 42 | Övrig momsfri försäljning m.m. | `ForsOvrigt` | `momsfriForsaljning` |
| 50 | Beskattningsunderlag vid import | `MomsUlagImport` | `import` |
| 60 / 61 / 62 | Utgående moms 25 / 12 / 6 % på import | `MomsImportUtgHog` / `MomsImportUtgMedel` / `MomsImportUtgLag` | `momsImportUtgaende{Hog,Medel,Lag}` |
| 48 | Ingående moms att dra av (aldrig utländsk moms) | `MomsIngAvdr` | `ingaendeMomsAvdrag` |
| 49 | Moms att betala (+) eller få tillbaka (−) = (10+11+12+30+31+32+60+61+62) − 48 | `MomsBetala` | `summaMoms` |
| – | Upplysning, max 300 tecken | `TextUpplysningMoms` | – |

### 2.1 Omvänd betalningsskyldighet (köparen redovisar momsen)

- Rutorna 20–24 är för inköp med omvänd betalningsskyldighet; vanliga svenska inköp med debiterad moms ska inte redovisas där [K3]. Köparen redovisar underlaget i 20–24, räknar ut utgående moms i 30–32, och drar av motsvarande ingående moms i 48 om avdragsrätt finns (Skatteverkets exempel 4 visar 600 kr i både ruta 31 och 48 vid full avdragsrätt) [K2][K3].
- **Tjänster från EU (ruta 21) och utanför EU (ruta 22):** huvudregeln gäller tjänster som anses tillhandahållna i Sverige; undantag för fastighetstjänster, persontransporter, evenemang, restaurang/catering och korttidsuthyrning av transportmedel [K3]. Lagstöd: en tjänst till en beskattningsbar person med säte i Sverige är tillhandahållen här (6 kap. 33 § ML), och när en ej etablerad säljare tillhandahåller sådan tjänst är förvärvaren betalningsskyldig (16 kap. 9 § ML) [K8].
- **Varor från EU (ruta 20):** den som gör ett unionsinternt förvärv är betalningsskyldig (16 kap. 17 § ML) [K8]. Utgående skatt redovisas för perioden då fakturan utfärdats, men senast för perioden som omfattar den 15:e i månaden efter förvärvet (7 kap. 26 § ML) [K8]. Trepartshandel går i 37/38 [K3].
- **Inhemsk omvänd betalningsskyldighet (23/24 resp. 41):** t.ex. byggtjänster, guld, avfall/skrot, utsläppsrätter, mobiltelefoner/datorer m.m. över 100 000 kr per faktura [K3]; lagstöd 16 kap. 6, 8, 10–16 §§ ML [K8].
- **Periodisering:** utgående skatt vid omvänd betalningsskyldighet redovisas när förvärvet enligt god redovisningssed bokförts eller borde ha bokförts (7 kap. 14 § 2 ML), och den ingående skatten för samma period som den utgående (7 kap. 34 § ML vid bokslutsmetod) [K8].
- **Import (50, 60–62):** utgående skatt redovisas för perioden då Tullverket ställt ut tullräkning/tullkvitto (7 kap. 27 § ML) [K8]; underlaget är tullvärde + tull/avgifter + bikostnader [K3].

### 2.2 Försäljning till EU och periodisk sammanställning

Försäljning i ruta 35, 38 och (i regel) 39 ingår även i *periodisk sammanställning* [K3], som är en separat redovisning med egna datum: varor per kalendermånad, tjänster per kvartal, senast den 25:e i månaden efter perioden via e-tjänst (20:e på papper), förseningsavgift 1 250 kr [K9].

### 2.3 Tidpunkt för redovisning (faktura- vs bokslutsmetod)

- Huvudregel: utgående skatt redovisas för den period då transaktionen enligt god redovisningssed bokförts eller borde ha bokförts (7 kap. 14 § ML) [K8].
- Bokslutsmetod (kontantmetod) är tillåten om årsomsättningen normalt är högst 3 mkr; obetalda fordringar ska ändå redovisas i periodens sista redovisningsperiod för beskattningsåret (7 kap. 16 § ML) [K8][K6b].
- Byte från faktura- till bokslutsmetod kräver ansökan hos Skatteverket (7 kap. 17 § ML) [K8].

## 3. Redovisningsperioder och frister

### 3.1 Perioder (26 kap. 10–13 §§ SFL) [K6]

- Huvudregel: kalendermånad (10 §).
- Beskattningsunderlag (exkl. unionsinterna förvärv och import) högst 40 mkr/år → kalenderkvartal; högst 1 mkr → beskattningsår (11 §).
- Skatteverket ska på begäran besluta om kortare period (månad i stället för kvartal/år, kvartal i stället för år); ett sådant beslut gäller minst 24 månader (13 §).
- Skatteverkets sammanfattning: ≤1 mkr år/kvartal/månad, ≤40 mkr kvartal/månad, >40 mkr månad [K6b].

### 3.2 Deklarationsdagar för ett aktiebolag

| Period | Sista dag | Lagrum [K6] |
|---|---|---|
| Kvartal (underlag ≤40 mkr) | 12:e i andra månaden efter kvartalet; Q2 17 augusti (dvs. 12 maj, 17 aug, 12 nov, 12 feb) | 26 kap. 26 § SFL |
| Månad (underlag ≤40 mkr, efter beslut) | 12:e i andra månaden efter perioden; 17:e i januari och augusti (t.ex. januari → 12 mars) | 26 kap. 27 § jfr 26 § (punktskattekolumnen) |
| Månad (underlag ≤40 mkr) på begäran tidigare | 26:e i månaden efter perioden, beslut gäller minst 12 perioder | 26 kap. 28 § jfr 30 § |
| Månad (underlag >40 mkr) | 26:e i månaden efter perioden; november → 27 december | 26 kap. 30 § SFL |
| Helår, AB utan EU-handel | Knutet till räkenskapsårets slut: 31 dec → 12 juli, 30 apr → 12 nov, 30 jun → 27 dec, 31 aug → 12 mars; vid elektronisk inlämning en månad senare (17:e i jan/aug, 12:e i apr/dec), dvs. för kalenderår 17 augusti | 26 kap. 33 b § SFL |
| Helår, AB med EU-handel (varor mellan EU-länder eller tjänster med omvänd betalningsskyldighet enligt art. 196) | 26:e i andra månaden efter beskattningsåret (27:e om det blir december), dvs. 26 februari för kalenderår | 26 kap. 33 § jfr 33 b § tredje st. och 33 a § andra st. |

Skatteverkets egen sammanställning bekräftar datumen (t.ex. AB helår med bokslut sep–dec: 12 juli på papper, 17 augusti digitalt; med EU-handel: 26:e i andra månaden) [K6b].

Viktigt för Maya: ett helårsredovisande AB som köper en enda tjänst från ett annat EU-land med omvänd betalningsskyldighet (t.ex. SaaS-abonnemang) flyttas från den sena dagen i 33 b § till 26 februari (33 § via 33 b § tredje stycket och 33 a § andra stycket 2) [K6].

## 4. Konsekvenser för Maya

- **Minsta byggbara väg:** generera eSKDUpload 6.0-XML från bokföringen, låt användaren ladda upp och signera i *Lämna momsdeklaration*. Inget avtal behövs; samma BankID-signering som vid manuell inmatning [K2].
- **Datamodell:** rutorna är heltal kronor och ruta 49 måste vara exakt (10+11+12+30+31+32+60+61+62) − 48 [K3][K4 FK009]. Maya behöver en mappning från konto/momskod → ruta; de formella kontrollerna FK001–FK006 och rimlighetskontrollerna RK002–RK011 i [K4] kan återanvändas som valideringar lokalt även för fil-vägen.
- **Periodkalender:** periodtyp (månad/kvartal/år), räkenskapsårets slut och flaggan "EU-handel/omvänd betalningsskyldighet under perioden" avgör sista dag [K6].
- **API-vägen** förutsätter ett avtal där avtalsparten är ett registrerat företag som tillhandahåller programvaran [K5 §1.2, §4.1]; det är oklart om en självhostad installation kan ansluta med användarens eget AB som "Programvaruföretag".

## Öppna frågor

1. **Kan en självhostad Maya använda API:et?** Villkoren riktar sig till "Programvaruföretag" som slutit avtal [K5]. Kan varje användares eget AB teckna avtal och få egna Client ID/Secret, eller måste ett centralt Maya-företag vara avtalspart och hantera nycklar? Fråga till icc@skatteverket.se [K4 §3.6].
2. **Taggordning i eSKD-filen:** tabellen och Exempel 6 i [K2] har olika ordning, och ingen DTD/XSD är publicerad. Verifiera mot e-tjänsten vilken ordning som accepteras (eller om ordningen är fri).
3. **Teckenkodning:** filen deklareras som ISO-8859-1 och "otillåtna tecken" avvisas [K2]; hur hanteras å/ä/ö i `TextUpplysningMoms`?
4. **Mappning BAS-konton/momskoder → rutor** täcks inte av Skatteverkets källor; behöver egen research (BAS-kontoplanen) och ett beslut om hur Maya representerar momskoder.
5. **Periodisk sammanställning** (EU-försäljning) är en separat inlämning med eget filformat [K9]; ska den ingå i Mayas scope?
6. **API Kundhändelser/Beskattningsengagemang** [K4 §2.5] kunde ge Maya redovisningsperiod och deadlines automatiskt; samma avtalsfråga som punkt 1.
7. **RAML-specifikationen** för API:ets JSON finns bara på Utvecklarportalen under *Komplett testtjänst* [K4 §5]; inte granskad här.

## Källor

- [K1] Skatteverket, *Deklarera moms*. https://www.skatteverket.se/foretag/moms/deklareramoms.4.7459477810df5bccdd480006935.html
- [K2] Skatteverket, *Lämna momsdeklaration via fil i e-tjänsten*. https://www.skatteverket.se/foretag/moms/deklareramoms/skapaochskickainmomsdeklarationviafil.4.2fb39afe18dabf1e4d223cc.html
- [K3] Skatteverket, *Fylla i momsdeklarationen*. https://www.skatteverket.se/foretag/moms/deklareramoms/fyllaimomsdeklarationen.4.3a2a542410ab40a421c80004214.html
- [K4] Skatteverket, *Beskrivning av tjänsten API för Momsdeklaration*, tjänstens version 1.0, dokumentversion 1.5 (2025-11-18). https://www7.skatteverket.se/portal-wapi/open/apier-och-oppna-data/utvecklarportalen/v1/getFile/tjanstebeskrivning-momsdeklaration-v1-0/pdf/1.0.7/Tjanstebeskrivning-Momsdeklaration-v1.pdf
- [K5] Skatteverket, *Bilaga A – Allmänna villkor för Skatteverkets API Momsdeklaration*, version 1.0 (gäller fr.o.m. 2025-11-20). https://www7.skatteverket.se/portal-wapi/open/apier-och-oppna-data/utvecklarportalen/v1/getFile/allmanna-villkor-momsdeklaration
- [K6] Skatteförfarandelag (2011:1244), 26 kap. 10–13, 21, 26–30, 33–33 b §§. https://data.riksdagen.se/dokument/sfs-2011-1244.text (även https://www.riksdagen.se/sv/dokument-och-lagar/dokument/svensk-forfattningssamling/skatteforfarandelag-20111244_sfs-2011-1244/)
- [K6b] Skatteverket, *När ska jag deklarera moms*. https://www.skatteverket.se/foretag/moms/deklareramoms/narskajagdeklareramoms.4.6d02084411db6e252fe80008988.html
- [K7] Skatteverket, *API-utveckling – Moms*. https://www.skatteverket.se/omoss/digitalasamarbeten/utvecklingsomraden/moms.4.7c708f0e16bed42cd0553bd.html
- [K8] Mervärdesskattelag (2023:200), 6 kap. 33 §, 7 kap. 14–17, 26–27, 34 §§, 16 kap. 5–17 §§. https://data.riksdagen.se/dokument/sfs-2023-200.text
- [K9] Skatteverket, *Periodisk sammanställning för varor och tjänster*. https://www.skatteverket.se/foretag/moms/deklareramoms/periodisksammanstallningforvarorochtjanster.4.58d555751259e4d661680001093.html
