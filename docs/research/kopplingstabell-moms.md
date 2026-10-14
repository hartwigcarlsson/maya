# Vilka konton hör till vilken ruta i kopplingstabellen för moms?

Underlag för kopplingstabellen för moms ([ADR 0008](https://github.com/hartwigcarlsson/maya/blob/main/docs/adr/0008-momsdeklarationen-raknas-ur-kontona.md)): vilka BAS 2026-konton som ger vilken ruta i momsdeklarationen, vilka konton som är tvetydiga och vilka rutor som inget BAS-konto täcker. Källorna läses 2026-10-14.

## Kort svar

1. **Det finns ingen fri, officiell koppling konto → ruta.** BAS säger själv att tabellerna "var varje enskilt konto hamnar i ... momsdeklarationen" finns i de betalda böckerna Bokföringsboken och Bokslutsboken [B3]. Varken den fria kontoplanen (PDF/XLSX) eller ändringsfilen för 2026 har någon kolumn för momsrutor [B1][B2]. Tabellen nedan är därför Mayas egen. Den härleds ur kontonamnen i BAS 2026 [B1] och Skatteverkets anvisningar till varje ruta [S1].
2. **De flesta rutor har entydiga BAS-konton.** Momskontona i 26xx delar upp utgående moms per momssats och per fall (försäljning/uttag/uthyrning, omvänd betalningsskyldighet, import, VMB). Därför ger 26xx rutorna 10–12, 30–32, 60–62 och 48 direkt. Underlagsrutorna 05, 06, 08, 20–24, 35, 36, 39–42 och 50 har egna intäkts- eller kostnadskonton.
3. **Rutorna 37 och 38 (trepartshandel) har inget BAS-konto.** Ruta 07 (VMB) kan inte tas ur ett enda kontosaldo. Ruta 41 täcks bara för byggtjänster (3231).
4. **Den största begränsningen är att underlaget för omvänd betalningsskyldighet och import bara syns om kostnaden bokförs i kontogrupp 40, 44 eller 45.** En SaaS-tjänst från USA på 5420 eller en dator från Tyskland på 1220 kommer inte med i ruta 21, 22 eller 20. BAS 2026 har inget justeringskonto (spegelkonto) för detta. Se avsnitt 4.
5. **Två fakta i tidigare beslut stämmer inte med BAS 2026.** I ADR 0008 står "3108 tjänster till EU", men 3108 är *Försäljning varor till annat EU-land, momsfri* (ruta 35). Tjänster till EU ligger på 3308 (ruta 39) [B1]. Momsavräkningen "2610–2649 mot 2650" i beslutet om moms från bokföring till deklaration omfattar också de vilande kontona 2618, 2628, 2638 och 2648, som inte hör till perioden. Se avsnitt 5.

## 1. Hur en ruta räknas ur kontona

- **Rörelse, inte saldo.** En ruta är summan av kontonas rörelse under momsperioden, inte utgående saldo. Ingående balans räknas inte, och inte heller momsavräkningen själv, som nollar 26xx mot 2650. Rörelsen läses alltså före avräkningsverifikatet.
- **Tecken.** För intäktskonton och utgående moms (kreditkonton) är rutan kredit − debet. För kostnadskonton och ingående moms är rutan debet − kredit. Rabatter (3730) och kundförluster (6351) minskar alltså ruta 05 automatiskt, eftersom de är debetposter i en kreditruta.
- **Hela kronor.** Filen tar bara heltal. Minustecken är tillåtet framför ett belopp [S2]. Ruta 48 får vara negativ när kreditfakturor ger tidigare avdragen moms att betala tillbaka [S2]. I e-tjänsten heter det fältet "Tidigare avdragen moms att betala tillbaka" [S1].
- **Parvisa kontroller.** Skatteverkets formella kontroller kräver att underlag och moms förekommer parvis: 05–08 ↔ 10–12, 20–24 ↔ 30–32 och 50 ↔ 60–62. Rimlighetskontrollerna varnar när momsen avviker från underlag × skattesats ([momsdeklarationsforskningen](https://github.com/hartwigcarlsson/maya/blob/research/momsdeklaration/docs/research/momsdeklaration.md), avsnitt 1.3). Momsfria förvärv (4078, 4518, 4538) får därför inte kopplas till 20–21. De ger underlag utan moms.
- **Ruta 49** räknas, den kopplas inte: (10 + 11 + 12 + 30 + 31 + 32 + 60 + 61 + 62) − 48 [S1].

## 2. Kopplingstabell för BAS 2026

Kontonamn ur BAS 2026 v 1.1 [B1]. Ruttexter ur Skatteverkets anvisning [S1]. Markeringen **(T)** betyder tvetydigt, se avsnitt 3.

### A. Momspliktig försäljning (05–08)

| Ruta | Konton | Kommentar |
|---|---|---|
| 05 Momspliktig försäljning | 3000–3003, 3106, 3500–3599 utom 3521, 3522, 3541 och 3542 (T), 3600–3669 (3612 T), 3680–3699, 3700–3739, 3790, 3914, 3920–3929 (T), 6351 (T) | 3106 "varor till annat EU-land, momspliktig" är försäljning till köpare utan giltigt VAT-nr, som Skatteverket säger ska stå i 05 [S1]. Kundförlust och rabatt ska minska 05 [S1]. Försäljning av inventarier ska också stå i 05 [S1], men den syns inte på något intäktskonto (avsnitt 4). |
| 06 Momspliktiga uttag | 3400–3403 | 3404 "Egna uttag, momsfria" ingår inte (T). |
| 07 Vinstmarginalbeskattning | inget rent intervall (T) | Underlaget är marginalen minus momsen på den, och 0 om marginalen är negativ [S1]. BAS har 3211/3212 (försäljning) och 4211/4212 (sålda varor) för VMB. Rutan blir alltså 3211 + 3212 − 4211 − 4212, lägst 0. Det är en formel, inte ett intervall. |
| 08 Hyra vid frivillig beskattning | 3913 | 3914 "Övriga momspliktiga hyresintäkter" går till 05. |

### B. Utgående moms på 05–08 (10–12)

| Ruta | Konton |
|---|---|
| 10 (25 %) | 2610, 2611, 2612, 2613, 2616 |
| 11 (12 %) | 2620, 2621, 2622, 2623, 2626 |
| 12 (6 %) | 2630, 2631, 2632, 2633, 2636 |

Uthyrning (26x3) och VMB (26x6) hör hit, eftersom 10–12 omfattar hyresinkomster vid frivillig beskattning och vinstmarginalbeskattning [S1].

### C–D. Inköp med omvänd betalningsskyldighet (20–24) och moms på dem (30–32)

| Ruta | Konton | Kommentar |
|---|---|---|
| 20 Varor från annat EU-land | 4070–4077, 4510–4517 | 4078 och 4518 (momsfri) kopplas inte (T). 4070–4078 är nya i BAS 2026 [B2]. |
| 21 Tjänster från EU, huvudregeln | 4535–4537 | 4538 (momsfri) kopplas inte. 4530 är huvudkonto för både 21 och 22 (T). |
| 22 Tjänster från land utanför EU | 4531–4533 | |
| 23 Varor i Sverige, köparen betalningsskyldig | 4060–4067, 4410–4417 | 4060–4067 är nya i BAS 2026 [B2]. |
| 24 Övriga tjänster i Sverige, köparen betalningsskyldig | 4420–4427 | Byggtjänster, vissa fastighetstjänster, utsläppsrätter [S1]. |
| 30 / 31 / 32 | 2614 / 2624 / 2634 | "Utgående moms omvänd betalningsskyldighet" per momssats [B1]. Samma konto bär moms för alla fem rutorna 20–24. |

### E. Momsfri försäljning m.m. (35–42)

| Ruta | Konton | Kommentar |
|---|---|---|
| 35 Varor till annat EU-land | 3108, 3521 (T), 3541 (T) | Kunden har giltigt VAT-nr och varan lämnar Sverige [S1]. |
| 36 Varor utanför EU | 3105, 3522 (T), 3542 (T) | |
| 37 Mellanmans inköp, trepartshandel | **inget BAS-konto** | |
| 38 Mellanmans försäljning, trepartshandel | **inget BAS-konto** | |
| 39 Tjänster till beskattningsbar person i annat EU-land | 3308 (T) | Bara huvudregeln. Undantagen (fastighet, persontransport m.fl.) hör till 40 [S1]. |
| 40 Övriga tjänster tillhandahållna utomlands | 3305 | |
| 41 Köparen betalningsskyldig i Sverige | 3231 | Bara byggsektorn har eget konto. Skrot, guld, utsläppsrätter och mobiler/datorer över 100 000 kr saknar konto (T). |
| 42 Övrig försäljning m.m. | 3004, 3910–3912, 3980–3989, 3994 | Momsfri försäljning, momsfri uthyrning, bidrag och försäkringsersättningar ska stå här [S1]. |

### F. Ingående moms (48)

| Ruta | Konton | Kommentar |
|---|---|---|
| 48 | 2640, 2641, 2642, 2645, 2646, 2647 | 2645 är beräknad ingående moms på förvärv från utlandet (20–22 och 50), 2647 på inhemsk omvänd betalningsskyldighet (23–24) [B1]. Utländsk moms (6998) får aldrig dras av [S1]. 2649 "blandad verksamhet" (T). |

### H–I. Import (50, 60–62)

| Ruta | Konton | Kommentar |
|---|---|---|
| 50 Beskattningsunderlag vid import | 4080–4087, 4540–4547 (T) | Underlaget är tullvärde plus tull, avgifter och bikostnader fram till bestämmelseorten [S1]. Tull och frakt bokförs i BAS på 5720–5729 respektive 57xx [B1], alltså inte på importkontot (avsnitt 4). 4080–4087 är nya i BAS 2026 [B2]. |
| 60 / 61 / 62 | 2615 / 2625 / 2635 | "Utgående moms import av varor" per momssats [B1]. |

### Konton i 26xx som inte kopplas

| Konto | Skäl |
|---|---|
| 2618, 2628, 2638 Vilande utgående moms | Momsen är inte redovisningspliktig ännu, t.ex. vid kontantmetoden eller förskott. Den kommer med när den flyttas till 26x1. |
| 2648 Vilande ingående moms | Samma sak för avdrag. |
| 2650 Redovisningskonto för moms | Motkonto för avräkningen, ruta 49 i bokföringen. |
| 2660 Punktskatter | Ingen ruta. |
| 2670 Utgående moms försäljning inom EU, OSS | OSS redovisas i en egen deklaration och inte i den vanliga [S1]. |
| 1650 Momsfordran | Fordran på skattekontot. |

## 3. Tvetydiga konton

| Konto | Problem | Förslag |
|---|---|---|
| 3100, 3200, 3300, 4000, 4400, 4500, 4530 (huvud- och gruppkonton) | Kontot täcker flera rutor (t.ex. 3300 både 39 och 40). | Koppla dem inte. Varna om ett okopplat 3xxx- eller 4xxx-konto har rörelse i momsperioden. |
| 3308 | Bara tjänster enligt huvudregeln hör till 39. Undantagen till en EU-företagare hör till 40, och tjänster till EU-privatpersoner har i regel svensk moms (05) [S1]. | 3308 → 39. Mayas fakturaflöde bokför bara B2B-huvudregeln på 3308. |
| 3521/3522, 3541/3542 (frakt och avgifter, EU/export) | Följer huvudleveransen, som kan vara varor (35/36) eller tjänster (39/40). | 3521/3541 → 35, 3522/3542 → 36. |
| 3612 Försäljning av skrot | Vissa metallskrot har omvänd betalningsskyldighet (41) [S1]. | 05. Omvänd skrotförsäljning bokförs på ett eget konto kopplat till 41. |
| 3920–3929 provisioner, licenser, royalties | Kan säljas till utlandet (39/40). | 05. Utländska kunder faktureras på 3308/3305. |
| 3404 Egna uttag, momsfria | Inte försäljning och inte momspliktigt uttag. | Ingen ruta. |
| 3671 Försäljning av värdepapper, 3672 utdelning | Ingen omsättning i momsens mening för ett normalt AB. | Ingen ruta. |
| 3950 Återvunna kundfordringar, 3996 Reklambidrag, 3999 Övriga rörelseintäkter | Momsen beror på vad ersättningen avser. | Ingen ruta. Momspliktiga belopp bokförs på 3xxx-konton som är kopplade. |
| 3970–3973 Vinst vid avyttring | Kontot visar vinsten, inte försäljningspriset som 05 kräver [S1]. | Ingen ruta. Se avsnitt 4. |
| 6351 Konstaterade förluster på kundfordringar | Ska minska 05 när utgående moms minskas [S1], men kontot kan också ha förluster utan momsjustering. | 6351 → 05 (minskande). 6352 (befarade) kopplas inte. |
| 4078, 4518, 4538 (momsfria förvärv från EU) | Underlag utan moms bryter den parvisa kontrollen 20–24 ↔ 30–32. | Ingen ruta. |
| 4540–4547, 4080–4087 (import) | Kontot har inköpspriset men inte tull och frakt, som ingår i underlaget [S1]. | Se avsnitt 4. |
| 2649 Ingående moms, blandad verksamhet | Bara den avdragsgilla delen får stå i 48. Resten förs till 6999 [B1]. | 48, under förutsättning att fördelningen bokförs före deklarationen. Blandad verksamhet kan ligga utanför v1. |
| 3211/3212, 4211/4212 (VMB) | Ruta 07 är en formel med golv 0, inte ett intervall. | VMB utanför v1, eller en uttrycklig formel i tabellen. |

## 4. Rutor som kontona inte kan ge

- **37, 38 (trepartshandel).** BAS har inga konton. Skatteverket redovisar trepartsförvärvet utan moms [S1]. Kräver egna konton, eller ligger utanför v1.
- **41 utanför bygg.** Bara 3231 finns.
- **20–24 när kostnaden inte bokförs i grupp 40/44/45.** Grupperna ligger under "Råvaror och förnödenheter" respektive "Handelsvaror" [B1]. En typisk tjänst från utlandet för ett tjänsteföretag (programvara 5420, IT-tjänster 6540, reklam 59xx), en inventarie från EU (1220–1229, 5410) eller en byggtjänst på fastigheten (5170, 1110) hamnar på ett konto som inte är kopplat. Då saknas underlaget i 20–24, medan momsen (2614/2645) kommer med. Det bryter den parvisa kontrollen. Det finns två vägar inom ADR 0008:
  1. Bokför kostnaden direkt på 4531–4537 och 4515–4517. Rutan blir rätt, men kostnaden hamnar under Råvaror och förnödenheter i resultaträkningen. I BAS-filen har 4535 SRU-koden 7500 (råvaror och förnödenheter), medan 5420 och 6540 har 7501 (övriga externa kostnader) [B2].
  2. Ett **justeringspar** (spegelkonto): kostnaden ligger på rätt konto, och samma belopp bokförs debet 45xx och kredit på ett justeringskonto i samma grupp. Rörelsen på 45xx ger då rutan, och nettot i gruppen blir 0. BAS 2026 har inget sådant konto. 4590–4599 är lediga, och 4790 är borttaget i BAS 2026 [B2]. Maya skulle få definiera ett inbyggt eget konto.
- **05 vid försäljning av inventarier.** Priset exklusive moms står inte på något intäktskonto. Avyttringen bokförs mot 12xx och vinsten eller förlusten på 3970–3973/7970–7973 [B1]. Antingen bokför Mayas avyttringsflöde priset på ett kopplat konto (t.ex. med ett justeringspar som ovan), eller också fattas beloppet i 05 medan momsen står i 10.
- **50 med tull och frakt.** Om tull och frakt bokförs på 5720–5729 eller 57xx blir 50 för lågt, och rimlighetskontrollen (moms jämfört med underlag × 25 %) kan varna. Ett alternativ är att Mayas importflöde bokför hela underlaget på 4545–4547/4085–4087. Ett annat är ett justeringspar som ovan.
- **Uttag ur skatteupplag (23)** och **överföringar till egen filial (20/35)** har inga konton [S1].
- **OSS-försäljning** får inte stå i den vanliga deklarationen [S1]. BAS har inget intäktskonto för den, bara momskontot 2670. Bokförs den på 3106 eller 3001 hamnar den felaktigt i 05.
- **Periodiseringar i bokslutet.** En upplupen intäkt på 3001 utan faktura ger underlag i 05 utan moms i 10. Det är en följd av att rutan bara är rörelsen på kontot.

## 5. Avvikelser från tidigare beslut

- **3108.** ADR 0008 räknar upp "3108 tjänster till EU". I BAS 2026, och redan i BAS 2020, är 3108 *Försäljning varor till annat EU-land, momsfri* och hör till ruta 35. *Försäljning tjänster till annat EU-land* är 3308 och hör till ruta 39 [B1][B4]. Kundfakturaflödet för EU-tjänster måste bokföra på 3308.
- **Kontogrupp 4 är omgjord i BAS 2026.** De nya kontona är 4060–4067, 4070–4078 och 4080–4087 för handelsvaror, med omvänd betalningsskyldighet, EU-förvärv och import. 4400–4547 har bytt namn till "råvaror och material". Numren 4415–4427 och 4515–4547 är oförändrade [B2]. Kopplingstabellen måste därför versioneras per BAS-år och inte bara per blankettutgåva.
- **Momsavräkningen.** Beslutet om moms från bokföring till deklaration avräknar 2610–2649 mot 2650. Vilande moms (2618, 2628, 2638, 2648) och den icke avdragsgilla delen av 2649 ska inte avräknas, eftersom de inte ingår i någon ruta. Annars stämmer 2650 inte med ruta 49. Avräkningen bör använda exakt de 26xx-konton som tabellen kopplar.
- **Ändringsfilen för 2026 innehåller basuppsättningen.** Det rör inte momsen men är värt att notera. Den fria [Kontoplansforandringar_2026.xlsx](https://www.bas.se/wp-content/uploads/2026/01/Kontoplansforandringar_2026.xlsx) har kolumnen "Lämplig för basuppsättning" för varje konto, och ett blad med JSON-filens fält (`isBasic`, `contraAccounts`, SRU- och INK2-koder) [B2]. ADR 0006 hämtar grundkontona ur BAS 2020-markeringen ■, eftersom forskningen om BAS-kontoplanen fann markeringen bara i JSON-filen för 2026.

## Källor

- [S1] Skatteverket, *Fylla i momsdeklarationen*. https://www.skatteverket.se/foretag/moms/deklareramoms/fyllaimomsdeklarationen.4.3a2a542410ab40a421c80004214.html
- [S2] Skatteverket, *Lämna momsdeklaration via fil i e-tjänsten* (teckenregler och ruta 48). https://www.skatteverket.se/foretag/moms/deklareramoms/skapaochskickainmomsdeklarationviafil.4.2fb39afe18dabf1e4d223cc.html
- [B1] BAS-kontogruppen, *Kontoplan BAS 2026*, v 1.1. [XLSX](https://www.bas.se/wp-content/uploads/2026/04/BAS_kontoplan_2026_v2.xlsx), [PDF](https://www.bas.se/wp-content/uploads/2026/04/BAS_kontoplan_2026_v2.pdf)
- [B2] BAS-kontogruppen, *Ändringar i kontoplanen för 2026*. https://www.bas.se/wp-content/uploads/2026/01/Kontoplansforandringar_2026.xlsx (länkad från [Jämför kontoplaner](https://www.bas.se/kontoplaner/jamfor-kontoplaner/))
- [B3] BAS-kontogruppen, *Skapa en egen kontoplan*. https://www.bas.se/kontoplaner/skapa-en-egen-kontoplan/
- [B4] BAS-kontogruppen, *Kontoplan BAS 2020*. https://www.bas.se/wp-content/uploads/2017/06/Kontoplan_Normal_2020.pdf
- Tidigare forskning: [Hur lämnas en momsdeklaration till Skatteverket digitalt?](https://github.com/hartwigcarlsson/maya/blob/research/momsdeklaration/docs/research/momsdeklaration.md) (rutor, XML-taggar, FK/RK) och [BAS-kontoplanen i Maya](https://github.com/hartwigcarlsson/maya/blob/research/bas-kontoplan/docs/research/bas-kontoplan.md).
