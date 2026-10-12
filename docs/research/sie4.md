# SIE4: innehåll, minimikrav för export och fallgropar vid import

Forskningsanteckning för [issue #8](https://github.com/hartwigcarlsson/maya/issues/8). Datum: 2026-10-12.

**Fråga:** Vad innehåller SIE typ 4 enligt aktuell specifikation (posttyper, teckenkodning, verifikationsposter, IB/UB/RES, dimensioner), vad krävs minimalt för en giltig export, och vilka fallgropar finns vid import av filer från andra system (Fortnox, Visma, Bokio)?

## Kortsvar

- Aktuell specifikation är **SIE filformat utgåva 4C (2025-08-06)**. Den ändrar inte själva formatet jämfört med 4B (2008), den förtydligar bara [S4C §1.7, kap. 13].
- SIE4 finns i två varianter: **4E** (export från bokföringsprogram, filändelse `.SE`, innehåller kontoplan + saldon + verifikationer) och **4I** (import till bokföringsprogram, filändelse `.SI`, i praktiken bara verifikationer) [S4C §5.1, kap. 6].
- Filen är radorienterad klartext i **Codepage 437** (`#FORMAT PC8`), med `#ETIKETT fält fält ...`, fält separerade av blanksteg, citattecken vid behov, belopp med punkt och högst två decimaler, datum `ÅÅÅÅMMDD`, kredit som negativt belopp [S4C §5.5–5.10, #FORMAT, #IB].
- En minimal giltig 4E-export för Maya: `#FLAGGA 0`, `#PROGRAM`, `#FORMAT PC8`, `#GEN`, `#SIETYP 4`, `#FNAMN`, `#RAR` (0 och -1 om det finns), `#KONTO` för alla använda konton, `#IB`/`#UB`/`#RES` för år 0 (och -1), samt `#VER { #TRANS ... }`. `#SRU` är *inte* obligatorisk i typ 4 [S4C kap. 6].
- Importfällor: teckenkodning, fältlängdsgränser i mottagande system (Fortnox: 50 tecken), räkenskapsår som måste finnas i förväg, dubbletter vid återimport, konton/objekt som inte finns, krav på kostnadsställe/projekt, och att importen i vissa system avbryts vid första fel.

## 1. Vad SIE4 är

SIE (Standard Import och Export) är en öppen svensk de-facto-standard för att flytta redovisningsdata mellan program, förvaltad av SIE-Gruppen [S4C §1.1, §1.6]. Typerna 1–3 är saldoexporter (årssaldon, periodsaldon, objektsaldon). Typ 4 är "import/export av transaktioner", alltså verifikationer [S4C §4.2, §4.7]. Import- och exportfilen bygger på samma specifikation men har olika obligatoriska poster [S4C §4.10].

sie.se listar också SIE 5, ett XML-baserat format från 2018, men beskriver SIE 1–4 som branschens de-facto-standard sedan 1992 [SIE-format]. Den här anteckningen gäller bara SIE 1–4.

Det finns en officiell testtjänst för SIE-filer på https://sietest.sie.se/ [S4C §3.1], och en officiell SIE4-exempelfil (exporterad från Visma Administration) [SIE-exempel].

## 2. Filformatet

| Regel | Källa |
|---|---|
| Varje post inleds med en etikett som börjar med `#`. | S4C §5.3 |
| Underposter, till exempel `#TRANS` under `#VER`, står inom `{` och `}`, och varje klammer står ensam på sin egen rad. | S4C §5.4 |
| Varje post avslutas med LF (ASCII 10). CR före LF är tillåtet men inget krav. | S4C §5.5 |
| Tomma rader är tillåtna överallt och ska ignoreras. | S4C §5.6 |
| Fält separeras av ett eller flera blanksteg eller tabbar. Citattecken behövs bara om fältet innehåller blanksteg. Ett `"` inuti ett fält skrivs som `\"`. Kontrolltecken (ASCII 0–31 och 127) får inte förekomma i strängar. | S4C §5.7 |
| Teckenuppsättningen ska vara IBM PC 8-bit extended ASCII, Codepage 437. `#FORMAT PC8` är det enda tillåtna värdet. | S4C §5.8, #FORMAT |
| Belopp skrivs med punkt som decimaltecken och högst två decimaler. Minustecken är tillåtet, plustecken inte. Ingen övre gräns för belopp, så den som läser måste själv kontrollera sina gränser. | S4C §5.9 |
| Datum skrivs `ÅÅÅÅMMDD`, perioder `ÅÅÅÅMM`. | S4C §5.10, #PSALDO |
| Fälten är positionsbestämda. Ett tomt fält som följs av fält med värden skrivs som `""`. Tomma fält i slutet av en rad kan utelämnas. | S4C kap. 12 |
| Posterna kommer i ordningen flaggpost, identifikationsposter, kontoplan, saldon/verifikationer. Inom varje grupp är ordningen fri. | S4C §5.12–5.13 |
| En läsare ska tåla okända etiketter och okända extra fält i slutet av en post, och ignorera dem. En skrivare får bara använda etiketter som finns i standarden. | S4C §7.1–7.3 |
| Saldo med kredit skrivs som negativt belopp. | S4C #IB, #UB, #RES |

Den officiella exempelfilen är Codepage 437 med CRLF [SIE-exempel].

## 3. Posttyper i SIE4

### Obligatoriska och frivilliga poster för 4I och 4E

Från tabellen i S4C kap. 6. ● = obligatorisk, o = frivillig, – = får inte förekomma. † = ska finnas för både innevarande och föregående år, om föregående år finns.

| Post | Betydelse | 4I | 4E |
|---|---|---|---|
| `#FLAGGA` | Importflagga (0 = ej inläst) | ● | ● |
| `#PROGRAM` | Programnamn och version | ● | ● |
| `#FORMAT` | `PC8` | ● | ● |
| `#GEN` | Datum (obligatoriskt fält) och signatur | ● | ● |
| `#SIETYP` | `4` | ● | ● |
| `#FNAMN` | Företagets juridiska namn | ● | ● |
| `#RAR` | Räkenskapsår (årsnr, start, slut) | o | ●† |
| `#KONTO` | Kontonummer (numeriskt) och namn | o | ● |
| `#IB` / `#UB` | Ingående/utgående balans för balanskonto | – | ●† |
| `#RES` | Saldo för resultatkonto | – | ●† |
| `#VER` / `#TRANS` | Verifikation med transaktionsrader | o | o |
| `#RTRANS` / `#BTRANS` | Tillagd/borttagen rad (behandlingshistorik) | o | o |
| `#PROSA`, `#FTYP`, `#FNR`, `#ORGNR`, `#ADRESS`, `#TAXAR`, `#KPTYP`, `#VALUTA`, `#KTYP`, `#ENHET`, `#SRU`, `#DIM`, `#UNDERDIM`, `#OBJEKT`, `#KSUMMA` | Frivilliga | o | o |
| `#OIB` / `#OUB` / `#PSALDO` / `#PBUDGET` | Objekt- och periodsaldon | – | o |
| `#OMFATTN` | Hur långt periodsaldona sträcker sig | – | o |
| `#BKOD` | SNI-kod | – | o |

Saldoposter med nollsaldo får utelämnas. Kravet betyder att om det finns värden så ska de skrivas [S4C §5.17–5.18]. För IB/UB gäller: `#UB` år 0 ska alltid finnas, minst ett av `#UB` år -1 och `#IB` år 0 ska finnas, och `#IB` år -1 är frivilligt men rekommenderat [S4C §5.16].

Det är ett brott mot standarden att bara exportera de poster man tror att mottagaren behöver [S4C §2.1–2.2].

### Identifikations- och kontoplansposter som är relevanta för ett AB

- `#ORGNR 556334-3689`: organisationsnumret ska ha bindestreck efter sjätte siffran [S4C #ORGNR].
- `#FTYP AB`: företagsform enligt Bolagsverkets lista [S4C #FTYP].
- `#RAR 0 20250101 20251231` / `#RAR -1 ...`: filen har bara en kontoplan, den för år 0, och jämförelseårens data måste normaliseras till den [S4C #RAR §3].
- `#KPTYP`: om posten saknas antas BAS95. Ett värde som börjar med `BAS2`, till exempel `BAS2025`, ska behandlas som `EUBAS97` [S4C #KPTYP]. Exempelfilen använder `EUBAS97` [SIE-exempel].
- `#VALUTA`: ISO 4217. Om posten saknas antas SEK [S4C #VALUTA].
- `#KONTO`: vid export ska alla *använda* konton exporteras. Vid import ska ett konto som finns i filen men saknas i kontoplanen läggas upp. Om namnet skiljer sig bör användaren varnas [S4C #KONTO].
- `#KTYP`: T/S/K/I. Kontot måste ha deklarerats med `#KONTO` tidigare i filen [S4C #KTYP].
- `#SRU konto kod` och `#TAXAR ÅÅÅÅ`: frivilliga i typ 4, men används för deklarationen [S4C #SRU, #TAXAR, kap. 6].

### Verifikationer: `#VER` och `#TRANS`

```
#VER serie vernr verdatum vertext regdatum sign
{
   #TRANS kontonr {objektlista} belopp transdat transtext kvantitet sign
}
```

- `#VER` ska alltid följas av `#TRANS`-poster inom klamrar. `vertext`, `regdatum` och `sign` är frivilliga [S4C #VER §1–3, §8].
- Serien kan vara bokstäver (A…), siffror (1…) eller en alfanumerisk sträng, till exempel `LEV1`. En importerande läsare ska hantera alla dessa [S4C #VER §4–5].
- I 4I-filer kan `serie` och/eller `vernr` vara tomma (`""`). Då numrerar det mottagande programmet [S4C #VER §6].
- Numrerade verifikationer i samma serie ska ligga i stigande nummerordning [S4C #VER §7].
- `#TRANS` får bara förekomma under `#VER`. Summan av alla belopp i en verifikation ska vara noll. Saknas `transdat` gäller verifikationsdatumet [S4C #TRANS].
- `#RTRANS` (tillagd rad) ska alltid direkt följas av en identisk `#TRANS` för bakåtkompatibilitet. En läsare som hanterar `#RTRANS` ska hoppa över den efterföljande `#TRANS`. En läsare som inte hanterar dem ignorerar `#RTRANS`/`#BTRANS` [S4C #RTRANS §4–6].
- `#BTRANS` (borttagen rad) har ingen motsvarande `#TRANS`. En rad som först lagts till och sedan tagits bort skrivs som `#BTRANS` [S4C #BTRANS].

Exempel ur den officiella exempelfilen [SIE-exempel]:

```
#VER A 1 20210105 Kaffebröd 20210310
{
   #TRANS 1910 {} -195.00
   #TRANS 2641 {} 20.88
   #TRANS 7690 {} 174.12
}
```

### Dimensioner och objekt

- `#DIM nr namn` deklarerar en dimension. `#UNDERDIM nr namn överdim` deklarerar en hierarkisk underdimension. `#OBJEKT dim objektnr namn` deklarerar ett objekt [S4C kap. 8, #DIM, #UNDERDIM, #OBJEKT].
- Reserverade dimensionsnummer: 1 kostnadsställe/resultatenhet, 2 kostnadsbärare (underdimension till 1), 6 projekt, 7 anställd, 8 kund, 9 leverantör, 10 faktura. 3–5 och 11–19 är reserverade för framtida bruk. 20 och uppåt är fria [S4C §8.17]. Reserverade dimensioner behöver inte deklareras [S4C §8.4].
- Objektlistan på en transaktion består av par av dimension och objekt, till exempel `{"1" "456" "7" "47"}`. Bara relevanta objekt behöver anges, och `{}` betyder att det inte finns några [S4C §8.21–8.22].
- För hierarkiska dimensioner konkateneras koderna vid export. Koderna förutsätts ha enhetlig längd, annars kan de bli tvetydiga [S4C §8.12–8.14]. Vid import anges bara underobjektets egen kod, och överobjektet måste alltid finnas i listan [S4C §8.23].
- Konton och objekt är frivilliga att deklarera i 4I. I så fall förutsätts de redan finnas i mottagaren. Specifikationen rekommenderar att de alltid deklareras [S4C kap. 6, slutnot]. En mottagare får välja att inte lägga upp odeklarerade objekt [S4C #OBJEKT §2].

### Övrigt

- **Kvantiteter:** alla saldo- och transaktionsposter har ett frivilligt kvantitetsfält. `#ENHET` anger enheten [S4C kap. 9].
- **`#FLAGGA`:** skrivs som 0. Det importerande programmet sätter den till 1 efter lyckad import, för att förhindra dubbelimport. Flaggan behöver bara hanteras vid ackumulerande import, till exempel av verifikationer [S4C #FLAGGA, §7.4]. Den officiella exempelfilen har `#FLAGGA 1` [SIE-exempel], så en exportfil kan i praktiken redan vara "markerad som inläst".
- **`#KSUMMA`:** frivillig CRC-32-kontrollsumma med polynomet `EDB88320`, startvärdet `FFFFFFFF` och slutlig bitinvertering. Summan beräknas över etiketter och fältinnehåll, utan separatorer, omslutande citattecken eller klamrar, och på CP437-bytes. Om summan inleds men inte avslutas bör läsaren vägra filen [S4C kap. 10]. Detta är samma parametrar som vanlig CRC-32 (zlib), men indata är den normaliserade teckenströmmen och inte filens bytes. Den slutsatsen är min tolkning av algoritmen i S4C kap. 10.

## 4. Minimal giltig 4E-export från Maya (ett AB)

Härlett ur S4C kap. 6 och §5.16:

```
#FLAGGA 0
#PROGRAM "Maya" 0.1
#FORMAT PC8
#GEN 20261012
#SIETYP 4
#ORGNR 559999-9991            (frivillig men bör med)
#FTYP AB                      (frivillig men bör med)
#FNAMN "Exempel AB"
#RAR 0 20260101 20261231
#RAR -1 20250101 20251231     (om föregående år finns)
#KPTYP BAS2025                (frivillig; tolkas som EUBAS97)
#KONTO 1930 "Företagskonto"   (alla använda konton)
...
#IB 0 1930 10000.00           (icke-noll saldon)
#UB 0 1930 12500.00
#UB -1 1930 10000.00
#RES 0 3001 -50000.00
...
#VER A 1 20260105 "Text" 20260105
{
#TRANS 1930 {} 1250.00
#TRANS 3001 {} -1000.00
#TRANS 2611 {} -250.00
}
```

Saker att tänka på i Mayas exportör:

- Koda om från UTF-8 till CP437. Tecken som saknas i CP437 måste ersättas, och kontrolltecken måste tas bort [S4C §5.7–5.8].
- `#UB`/`#RES` för år -1 och `#IB` för år 0 måste räknas fram om föregående år finns [S4C §5.16].
- Avrunda belopp till högst två decimaler [S4C §5.9].
- Verifikationer i varje serie ska sorteras i stigande nummerordning [S4C #VER §7].
- Om Maya ska kunna skriva en `#BTRANS`/`#RTRANS`-historik avgörs av hur rättelser modelleras. En rättelse via ny verifikation kräver inte dessa poster.
- Tänk på mottagarens begränsningar: Fortnox avvisar verifikations- och transaktionstexter över 50 tecken [Fortnox-fel].

## 5. Import: fallgropar från leverantörernas egna dokumentation

### Tolkning och format

- **Teckenkodning:** Fortnox anger "ogiltiga tecken" som ett vanligt importfel och rekommenderar manuell rättning i en textredigerare [Fortnox-fel]. Maya bör läsa CP437 enligt standarden, men eftersom vissa filer i praktiken kan vara Latin-1/Windows-1252 eller UTF-8 behöver läsaren en heuristik för att upptäcka det och en varning till användaren. Det sistnämnda är en designslutsats, inte något en källa säger.
- **Citattecken runt fält som inte är strängar:** Fortnox ger felet "Fail to format date" när `#SIETYP`, `#FLAGGA`, `#ORGNR` eller `#FORMAT` har citattecken, och rättningen är att ta bort dem [Fortnox-fel]. Specifikationen tillåter att alla fält omsluts av citattecken [S4C §5.7], så Mayas parser bör acceptera det.
- **Exempelfilens egenheter:** `#FNR` innehåller en Windows-sökväg, `#FLAGGA` är 1, och textfält saknar citattecken när de inte innehåller blanksteg [SIE-exempel]. En robust parser behöver alltså en riktig tokenizer med citattecken och `\"`, inte en split på blanksteg.
- **Okända etiketter och extra fält** ska ignoreras [S4C §7.1–7.3].
- **Mycket stora värden och långa strängar:** standarden har nästan inga längdgränser, så läsaren måste hantera överskridanden explicit [S4C §7.5–7.6].

### Semantik och data

- **Räkenskapsår:** Bokio kräver att räkenskapsåren skapas i förväg, med rätt datum för förlängda, förkortade eller brutna år. Ett år får vara högst 18 månader, åren måste följa på varandra utan luckor, och filerna importeras ett år i taget i kronologisk ordning [Bokio-SI]. Bokio blockerar import om räkenskapsåren i filen inte stämmer med inställningarna [Bokio-fel]. Fortnox rekommenderar att räkenskapsåret skapas innan import [Fortnox-imp].
- **En fil per räkenskapsår:** en SIE-fil från Fortnox innehåller alltid ett helt räkenskapsår eller del av ett, aldrig flera [Fortnox-exp]. Det stämmer med `#RAR`-modellen, där år 0 är exportåret och -1, -2 bara är jämförelsesaldon [S4C #RAR].
- **Ingående balanser:** om första importerade året har IB som är noll eller saknas, och inte alla tidigare år importeras, måste IB anges manuellt [Bokio-SI]. Fortnox låter användaren välja om IB ska uppdateras [Fortnox-imp]. Visma Compact skriver över IB med filens värden [Visma-Compact].
- **Dubbletter:** Visma lägger till importerade verifikationer efter befintliga, så samma fil två gånger ger dubbla verifikationer. Om filens numrering används ersätts befintliga verifikationer med samma serie och nummer [Visma-Compact]. Visma Bokföring markerar filer som "Redan inläst" [Visma-SIE4]. Detta är hela poängen med `#FLAGGA` [S4C #FLAGGA].
- **Verifikationsserier och numrering:** mottagaren måste kunna mappa filens serier till sina egna. Både Fortnox och Visma låter användaren välja mappning [Fortnox-imp][Visma-SIE4]. Visma kräver omnumrering eller avbrott vid nummerkrockar [Visma-SIE4].
- **Konton:** Fortnox låter användaren koppla filens konton till befintliga, skapa nya eller ignorera dem, och välja om kontonamn och SRU ska tas från filen eller från den befintliga kontoplanen [Fortnox-imp]. Standarden säger att okända konton ska läggas upp och namnavvikelser flaggas [S4C #KONTO].
- **Momskoder finns inte i SIE.** Fortnox låter användaren sätta dem vid import [Fortnox-imp]. Mayas momsrapportering kan alltså inte bygga på något i en importerad SIE-fil utöver kontonumren.
- **Objekt:** Fortnox stoppar import om kontot kräver kostnadsställe men filen saknar det, om ett projekt i filen inte har status "Pågår", eller om projektkoder är dubbletter [Fortnox-fel][Fortnox-imp].
- **Felhantering:** Fortnox avbryter importen vid första felet, så användaren kan behöva importera flera gånger för att se alla fel [Fortnox-fel]. Bokio kör i stället en kontroll först och visar alla fel och varningar före import [Bokio-SI]. Bokios modell är att föredra för Maya.
- **Ångra:** Fortnox "Ångra SIE" fungerar bara på den senaste importen, tar bara bort verifikationer, och fungerar inte om något bokförts i samma serie efteråt [Fortnox-imp].
- **Underlag och bilagor följer inte med i SIE.** Bokio exporterar kvitton separat [Bokio-exp].
- **Missuppfattning om innehållet i SIE4:** Bokio och Fortnox beskriver SIE4 som "SIE3 plus verifikationer" [Bokio-vad][Fortnox-exp]. Enligt specifikationen är `#PSALDO`/`#OIB`/`#OUB` bara *frivilliga* i 4E [S4C kap. 6]. En importör kan alltså inte räkna med periodsaldon eller objektsaldon i en SIE4-fil, och ska härleda dem från verifikationerna.

### Leverantörernas SIE4-utbud i korthet

| Leverantör | Export | Import | Källa |
|---|---|---|---|
| Fortnox | SIE 1–4, val av datumintervall och verifikationsserier, en fil per räkenskapsår | SIE 1–4 med kontoplan, IB, verifikationer, kostnadsställen, projekt och budget | Fortnox-exp, Fortnox-imp |
| Bokio | SIE4 (`.se`) per räkenskapsår | `.se` (balanser + verifikat) och `.si` (bara verifikat) | Bokio-exp, Bokio-vad, Bokio-SI |
| Visma/Spiris (Compact, Bokföring) | SIE-export finns (typutbud ej verifierat här) | Compact: endast typ 4, IB läses bara in i företagets första räkenskapsår. Bokföring: filväljaren förväntar sig `.si` | Visma-Compact, Visma-SIE4 |

## 6. Öppna frågor för Maya

1. Ska Maya stödja import av en hel historik (flera `.se`-filer, ett räkenskapsår åt gången) vid flytt från ett annat system, eller bara IB plus innevarande år?
2. Hur ska Maya hantera importerade verifikationers serie och nummer? Ska de behålla originalnumren i en egen serie per källa, eller numreras om? Detta påverkar kravet på obruten nummerföljd i BFL.
3. Behöver Maya skriva `#RTRANS`/`#BTRANS`, eller räcker det att rättelser alltid görs med nya verifikationer?
4. Teckenkodningsheuristik vid import: hur ska CP437, Windows-1252 och UTF-8 upptäckas, och ska användaren bekräfta?
5. Ska Maya exportera `#SRU`/`#TAXAR`? Det är frivilligt i typ 4, men nyttigt för bokslutsprogram och deklaration (INK2).
6. Ska Maya generera `#KSUMMA` vid export och verifiera den vid import?
7. Hur ska dimensioner (kostnadsställe, projekt) modelleras i Maya, om alls? Det avgör hur `{objektlista}` hanteras vid import.

## Källor

- **[S4C]** SIE-Gruppen, *SIE filformat, utgåva 4C*, 2025-08-06. https://sie.se/wp-content/uploads/2026/02/SIE_filformat_ver_4C_2025-08-06.pdf. Hänvisningar som "§5.7" avser punktnumreringen i dokumentet, och "#ETIKETT" avser beskrivningen av den posttypen i kap. 11.
- **[S4B]** SIE-Gruppen, *SIE filformat, utgåva 4B*, 2008-09-30 (föregångare, formatet oförändrat). https://sie.se/wp-content/uploads/2020/05/SIE_filformat_ver_4B_080930.pdf
- **[SIE-format]** sie.se, formatsida. https://sie.se/format/
- **[SIE-exempel]** sie.se, officiell SIE4-exempelfil (`SIE4 Exempelfil.SE`, exporterad med Visma Administration 2022.2). https://sie.se/wp-content/uploads/2024/01/SIE4-Exempelfil-Sample-file-1.zip
- **[Fortnox-imp]** Fortnox, *Importera SIE-fil*. https://support.fortnox.se/produkthjalp/bokforing/importera-sie-fil
- **[Fortnox-exp]** Fortnox, *Exportera SIE-fil*. https://support.fortnox.se/produkthjalp/bokforing/exportera-sie-fil
- **[Fortnox-fel]** Fortnox, *Felsökningsguide – SIE-fil*. https://support.fortnox.se/produkthjalp/bokforing/felsokningsguide-sie-fil
- **[Bokio-vad]** Bokio, *Hur fungerar en SIE-fil?* https://www.bokio.se/hjalp/komma-igang/importera-bokforing/vad-ar-en-sie-fil/
- **[Bokio-SI]** Bokio, *Så här importerar du en SI-fil*. https://www.bokio.se/hjalp/komma-igang/importera-bokforing/hur-importerar-jag-si-fil/
- **[Bokio-fel]** Bokio, *Min SIE-fil går inte att importera?* https://www.bokio.se/hjalp/komma-igang/importera-bokforing/min-sie-fil-gar-inte-att-importera/
- **[Bokio-exp]** Bokio, *Hur exporterar jag bokföring från Bokio?* https://www.bokio.se/hjalp/bokforing/exportera-bokforing/hur-exporterar-jag-bokforing-fran-bokio/
- **[Visma-Compact]** Spiris/Visma, *Import och export av SIE – Visma Compact 500*. https://support.spiris.se/visma-compact-500/content/visma-compact/bokforing/sie-export-sie-import.htm
- **[Visma-SIE4]** Spiris/Visma, *Import av verifikationer SIE4 – Visma Bokföring*. https://support.spiris.se/visma-bokforing/content/visma-bokforing/bokforing/import-av-verifikationer-sie4.htm
