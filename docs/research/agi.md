# Arbetsgivardeklaration på individnivå (AGI): digital inlämning

Research för [#5](https://github.com/hartwigcarlsson/maya/issues/5). Hämtat 2026-10-12. Gäller teknisk beskrivning **1.1.18.2**, XML-schema **1.1** och API-tjänsten **Arbetsgivardeklaration inlämning v1.7** (tjänstebeskrivning dokumentversion 1.23, 2025-10-17).

## Kort svar

- Det finns **tre digitala vägar in**, och alla går till samma "eget utrymme" hos Skatteverket: (1) ladda upp en XML-fil i e-tjänsten Arbetsgivardeklaration på Mina sidor, (2) registrera uppgifterna för hand i e-tjänsten, (3) läsa in samma XML via Skatteverkets REST-API. [TB §4.2]
- **Själva inlämningen är alltid en signering** som en behörig företrädare gör med e-legitimation i Skatteverkets e-tjänst. API:et kan inte signera. Det returnerar en djuplänk till granskningssidan. [TB §4.2; API §2.5]
- **Formatet är XML (UTF-8)** som följer schemat `arbetsgivardeklaration_1.1.xsd` och `arbetsgivardeklaration_component_1.1.xsd`. Varje dataelement har ett obligatoriskt attribut `faltkod` med deklarationsrutans nummer (FK). [TB §10, §11.2]
- **API:et kräver ett avtal** mellan Skatteverket och ett *programvaruföretag* (F-skatt, momsregistrering, inga skulder hos Kronofogden), OAuth 2.0 och organisationscertifikat. Filuppladdning kräver bara e-legitimation och inget avtal. [Villkor §3–4; API §5.1.4; TB §6.2]
- **Frist**: för ett AB med momsunderlag ≤ 40 mnkr ska deklarationen ha kommit in senast den **12:e** i månaden efter redovisningsperioden, utom för perioderna juli och december, som har frist den **17:e** (augusti respektive januari). Över 40 mnkr gäller den 26:e (27 december för november). Infaller dagen på en helgdag flyttas fristen till nästa vardag. [SFL 26:26, 26:30, 26:32; lag 1930:173 2 §]

## 1. Rättslig grund

| Vad | Källa |
|---|---|
| Den som gör skatteavdrag eller betalar arbetsgivaravgifter ska lämna skattedeklaration, i form av en arbetsgivardeklaration per redovisningsperiod. | SFL 26 kap. 2 § 1 och 3 § |
| Redovisningsperioden är en kalendermånad. | SFL 26 kap. 10 § |
| Den som är registrerad ska deklarera **varje** period. Om det inte finns något att redovisa räcker en skriftlig upplysning till Skatteverket. | SFL 26 kap. 3 § andra st. |
| Uppgifter på arbetsgivarnivå (summa skatteavdrag och avgifter m.m.). | SFL 26 kap. 19 § |
| Uppgifter per betalningsmottagare: ersättning, skatteavdrag, avgiftsunderlag, arbetsplatsens adress m.m. | SFL 26 kap. 19 a § |
| Övriga uppgifter per mottagare: andra ersättningar och förmåner, arbetsställenummer, rut/rot-underlag, personaloption, bostadsersättning, styrelsearvode m.m. | SFL 26 kap. 19 b § |
| Traktamente och bilersättning under schablon: det räcker att ange *att* ersättningen getts ut (kryss). | SFL 26 kap. 19 c § |
| Uppgift om annat driftstöd vid regionalt avdrag (ny 2026). | SFL 26 kap. 19 f § |
| Frister. | SFL 26 kap. 26, 30–32 §§ |
| Rättelse ska göras "snarast", men inte efter utgången av sjätte året efter det kalenderår uppgiften avser. | SFL 26 kap. 40 § |
| Deklarationer ska lämnas på fastställt formulär och undertecknas. Ett deklarationsombud får bara lämna deklarationer elektroniskt. | SFL 38 kap. 1–2 §§; 6 kap. 5 § |
| Förseningsavgift för skattedeklaration är 625 kr, eller 1 250 kr efter föreläggande. | SFL 48 kap. 1 och 6 §§ |
| Frånvarouppgifter (föräldrapenning/tillfällig föräldrapenning) lämnas i samband med AGI från och med period januari 2025. Den anställda ska informeras senast samma dag. | Lag (2024:1300) 2–4 §§ och ikraftträdande |

## 2. Inlämningsvägar

### 2.1 E-tjänsten (filuppladdning eller registrering)

- Den som laddar upp en fil behöver bara e-legitimation, ingen registrerad behörighet. Bara en *behörig företrädare* kan signera och skicka in: personen själv, en firmatecknare eller ett deklarationsombud. Med rollen *registreringsombud* kan man registrera och ändra uppgifter men inte signera. [TB §6.2–6.6]
- Man kan ladda upp flera filer för samma period, till exempel en med huvuduppgift och flera med individuppgifter. Innehåller filen bara individuppgifter kan e-tjänsten räkna fram arbetsgivaravgifter och summera skatteavdraget. [TB §4.2.1]
- Utkast sparas i "eget utrymme". Skatteverket kan inte se uppgifterna förrän de signerats och skickats in. När det är gjort lämnas en elektronisk kvittens. [TB §4.2, §7.2]
- Det finns en **testtjänst** som inte kräver inloggning: <https://sso.test.skatteverket.se/agd_tt/da_testtjanst_web/login.do?method=test>. Den har en gräns på 100 MB. [TB-sidan "Teknisk beskrivning och testtjänst"; TB §11.5]
- Gränser: 300 MB per fil för filöverföring och API, och högst **1 000 unika deklarationer** (arbetsgivare × period) per fil. [TB §10.2]

### 2.2 API: "Arbetsgivardeklaration inlämning v1"

Bas-URI i produktion är `https://api.skatteverket.se/arbetsgivardeklaration/inlamning/v1`. Komplett testtjänst finns på `api.test.skatteverket.se`. [API §3, §5.1]

| Operation | Metod + sökväg (relativt `/arbetsgivardeklaration/inlamning/v1`) |
|---|---|
| Läs in underlag (XML enligt schemat) → `inlamningId` | `POST /underlag` |
| Hämta kontrollresultat (asynkront: `PROCESSING` → `DONE_SUCCESS` / `DONE_FAILED` / `DONE_REJECTED`) | `GET /underlag/{inlamningId}/kontrollresultat` |
| Avbryt inlämning med fel | `DELETE /underlag/{inlamningId}` |
| Spara inlämning trots fel | `POST /underlag/{inlamningId}/spara` |
| Skapa granskningsunderlag → djuplänk till Mina sidor, kan också låsa perioden (`lasPeriod`) | `POST /arbetsgivare/{agId}/redovisningsperioder/{YYYYMM}/skapaGranskningsunderlag` |
| Ta bort sparad inlämning | `DELETE /arbetsgivare/{agId}/redovisningsperioder/{YYYYMM}/inlamningar/{inlamningId}` |
| Kontrollera en enskild HU/IU (JSON, sparas inte) | `POST /underlag/huvuduppgift/kontrollera`, `POST /underlag/individuppgift/kontrollera` |

[API §4.1.1, §5.1.1]

- **Säkerhet**: TLS 1.2 och OAuth 2.0 Authorization Code Grant, där organisationen autentiseras med organisationscertifikat. Headers: `client_id`, `client_secret` och `SKV-client_correlationid`. Scope: `agd`. [API §5.1.3–5.1.4, §4.1.2.2]
- Inlämningar som innehåller fel och som varken sparas eller avbryts rensas efter 24 timmar. [API §4.1.1]
- **Anslutning**: man begär tillgång till sandbox och produktion via formulär och tilldelas API-nycklar. Produktion kräver avtal med programvaruföretaget. [API §3.2] De allmänna villkoren kräver att programvaruföretaget är registrerat i aktiebolags-, handels- eller liknande register, momsregistrerat, godkänt för F-skatt och fritt från skulder hos Kronofogden. Personuppgifter ska tas emot i en miljö inom EU/EES. [Villkor §4, §5.2]
- Ett separat API, **"Arbetsgivardeklaration hantera redovisningsperiod v1"** (`/arbetsgivardeklaration/hanteraredovisningsperiod/v1`), har operationerna *hämta grunddata* (deklarations- och anståndsdatum, periodens tillstånd), *händelser*, *summeringsrapport*, *lås/lås upp*, *kvittenser* och *skapa granskningsunderlag*. En låst period kan också signeras via tjänsten "Skriv under digitalt". Tjänstebeskrivningen varnar själv för att den innehåller kända fel om vilka fält som är obligatoriska. [HRP §1, §3, §4.1.1]

## 3. Filens struktur

```
<Skatteverket omrade="Arbetsgivardeklaration">        (instans-ns …/da/instans/schema/1.1)
  <agd:Avsandare>          programnamn, orgnr, teknisk kontakt, Skapad (tidpunkt)
  <agd:Blankettgemensamt>  per arbetsgivare: AgRegistreradId + 0..n Kontaktperson
  <agd:Blankett>*          Arendeinformation (Arendeagare, Period) + Blankettinnehall: <agd:HU> | <agd:IU>
  <agd:Franvarouppgift>*
</Skatteverket>
```

[instans-XSD; TB §10.5; Exempelfil 01]

- Komponent-namnrymden är `http://xmls.skatteverket.se/se/skatteverket/da/komponent/schema/1.1`. Minst en av de två namnrymderna måste ha ett prefix. [TB §10.4; instans-XSD]
- Alla belopp anges som **heltal i kronor**. [TB §8]
- Kryssfält anges som `1`/`true` eller utelämnas. [Fältlista, fliken Fälttyp]
- En fil får innehålla flera arbetsgivare och flera perioder. [TB §10.5]
- Schemaversionen ändras bara vid förändringar som bryter bakåtkompatibiliteten. Nya fält läggs till utan versionsbyte. [TB §10.6]
- Kontroller i fyra nivåer: `ba_nnn` (avvisande vid uppladdning), `S_nnn` och `B_nnn` (stoppande samband inom eller mellan formulär) och `R_nnn` (rimlighet, inte stoppande). [TB §7, §12.5]

### 3.1 Identitet och ersättningslogik

- Obligatoriskt på HU och IU: `RedovisningsPeriod` (FK006, `ÅÅÅÅMM`) och `AgRegistreradId` (FK201, 12 siffror inklusive sekel, för AB `16NNNNNNNNNN`). [TB §8.1; Fältlista]
- IU kräver dessutom `Specifikationsnummer` (FK570, 1–10 siffror). Kombinationen arbetsgivare + period + mottagare + specifikationsnummer är nyckeln: en ny IU med samma nyckel **ersätter** den gamla. Ta bort görs med `Borttag` (FK205) plus nyckeln. [TB §8.1.3, §9.1–9.2]
- Mottagarens identitet anges i FK215 (person- eller samordningsnummer), FK222 (födelsetid `ÅÅÅÅMMDDnnn`) eller FK224 (annat id). Saknas personnummer krävs namn och adress. [TB §8.3.1]
- **Exakt ett** av sju skattefält ska vara ifyllt per IU: FK001 (avdragen preliminärskatt, 0 om inget avdrag gjorts), FK274 SINK, FK275 A-SINK, FK114, FK276, FK253 eller FK094. Olika skatteregimer för samma mottagare och period kräver separata IU med olika specifikationsnummer. [TB §8.3.2]

### 3.2 Centrala fält för ett litet AB

| FK | Element | Innebörd | Formulär |
|---|---|---|---|
| 001 | `AvdrPrelSkatt` | Avdragen preliminärskatt | IU |
| 011 | `KontantErsattningUlagAG` | Kontant bruttolön m.m. (avgiftsunderlag) | IU |
| 012 | `SkatteplOvrigaFormanerUlagAG` | Övriga skattepliktiga förmåner | IU |
| 013 / 018 / 098 | `SkatteplBilformanUlagAG` / `DrivmVidBilformanUlagAG` / `BetForDrivmVidBilformanUlagAG` | Bilförmån, drivmedel (marknadsvärde × 1,2), nettolöneavdrag för drivmedel | IU |
| 019 / 020 | `AvdragUtgiftArbetet` / `AndraKostnadsers` | Avdrag för utgifter i arbetet / övriga kostnadsersättningar | IU |
| 023 | `AndelStyrelsearvode` | Varav styrelsearvode (från 202401) | IU |
| 030 | `Tjanstepension` | Tjänstepension | IU |
| 041 / 043 | Bostadsförmån småhus / ej småhus (kryss) | | IU |
| 048 | `FormanHarJusterats` | Förmånsvärdet justerat av Skatteverket (kryss) | IU |
| 050 / 051 | `Bilersattning` / `Traktamente` | Skattefri ersättning har getts ut (kryss) | IU |
| 060 | `Arbetsstallenummer` | Arbetsställenummer om det finns fler än ett arbetsställe | IU |
| 131–134 | `…EjUlagSA` | Ersättning eller förmån som inte är underlag för socialavgifter, t.ex. under 1 000 kr/år | IU |
| 245 / 246 | `ArbetsplatsensGatuadress` / `ArbetsplatsensOrt` | Arbetsplatsens adress om det bara finns ett tjänsteställe (från 202301) | IU |
| 487 | `SummaArbAvgSlf` | Summa arbetsgivaravgifter och SLF | HU |
| 497 | `SummaSkatteavdr` | Summa skatteavdrag | HU |
| 470–477, 463 | FoU, regionalt stöd, rederi, annat driftstöd (463 från 202601) | | HU |
| 481 / 486 | `UlagSlfVinstandel` / `SlfVinstandelFK486` | SLF på vinstandelsstiftelse/sjukpension | HU |
| 491–496 | Underlag/skatteavdrag för pensionsförsäkring respektive ränta och utdelning | | HU |
| 821–827 | `FranvaroDatum`, `FranvaroTyp` (`FORALDRAPENNING` / `TILLFALLIG_FORALDRAPENNING`), omfattning i % eller timmar | | Frånvarouppgift |

[Fältlista 1.1.18.1; TB §5.3, §8.2–8.4]

- Fält med giltighetsperiod: FK062 och FK063 (växa-stöd) **upphör efter 202512**, FK499 (sjuklönekostnad) upphörde efter 202406, och FK463 tillkommer från 202601. [Fältlista]
- En förmån som den anställda betalat fullt ut genom nettolöneavdrag redovisas ändå, med värdet 0. [TB §5.3]

### 3.3 Summering och avgiftsberäkning

- Skatteverket räknar fram egna jämförelsebelopp: IK587 för avgifter och IK597 för skatteavdrag. Har filen FK487 och FK497 måste de stämma mot jämförelsebeloppen (kontroll B_006 och B_007). Utelämnas de räknar e-tjänsten fram dem. [TB §12.6]
- Avgiftsunderlaget summeras per IU och fördelas på ålderskategori. Avgiften beräknas per procentsats, och ören tas bort per procentsats. Därefter läggs SLF till och avdragen för FoU, regionalt stöd och rederi dras av. [TB §12.6.1]
- Procentsatser enligt bilagan Summering och beräkningsregler:

  | Kategori | Sats |
  |---|---|
  | Full avgift | 31,42 % |
  | Ålderspensionsavgift endast (född 1938 eller senare, har fyllt 67 vid årets ingång år 2026) | 10,21 % |
  | Född 1937 eller tidigare | 0 % |
  | Ungdom 19–23 år från 202604, upp till 25 000 kr/mån | 20,81 % |
  | Utan fast driftställe från 202601 | 18,80 % |
  | SLF | 24,26 % |

  [Summering, flikarna Procentsatser och Ålderskategorier; TB "Ändringar i 1.1.18.2"]
- Skatteavdrag = summan av FK001, FK274 och FK275 i alla IU, plus FK495 och FK496 från HU. [TB §12.6.2]

## 4. Frister och rättelser

- **≤ 40 mnkr momsunderlag** (eller ingen momsdeklaration): jan→12 feb, feb→12 mar, mar→12 apr, apr→12 maj, maj→12 jun, jun→12 jul, **jul→17 aug**, aug→12 sep, sep→12 okt, okt→12 nov, nov→12 dec, **dec→17 jan**. [SFL 26:26, 26:32]
- **> 40 mnkr**: den 26:e i månaden efter (nov→27 dec). Skatteverket kan besluta om den 12:e om betalningarna missköts. [SFL 26:30–31]
- Frist som infaller på lördag, söndag eller annan helgdag flyttas till nästa vardag. [Lag (1930:173) 2 §]
- Rättelse och omprövning görs på samma sätt som första inlämningen, högst sex år bakåt. E-tjänsten tar inte emot deklarationer äldre än sex år. [TB §9, "Ändringar i 1.1.18.2"; SFL 26:40]
- **Frånvarouppgifter** lämnas bara tillsammans med periodens *första* deklaration och kan inte rättas eller kompletteras i efterhand. De förs vidare till Försäkringskassan. [TB §3, §9.3; lag (2024:1300) 5 §]

## 5. Konsekvenser för Maya (tolkning, inte källa)

- Den minsta vägen som håller är att **generera en XML-fil per period** enligt schema 1.1, validera den mot XSD, och låta användaren testa den i testtjänsten eller ladda upp den i e-tjänsten och signera där. Det kräver inget avtal med Skatteverket.
- API-vägen förutsätter att Maya, eller den som driver en instans, ingår avtal som *programvaruföretag* och skaffar organisationscertifikat och OAuth-klient. Det passar dåligt för en självhostad enbolagsinstallation. Signeringen sker ändå i Skatteverkets gränssnitt.
- Om Maya sätter FK487 och FK497 själv måste Mayas avgiftsberäkning (ålderskategorier, öresavrundning per sats) matcha Skatteverkets exakt. Annars stoppar kontrollerna B_006 och B_007 deklarationen. Ett alternativ är att utelämna dem och låta e-tjänsten räkna.
- Specifikationsnumret (FK570) måste vara stabilt per anställd och period för att rättelser ska ersätta i stället för att dubblera. Det behöver alltså lagras.

## Källor

- [TB] Skatteverket, *Teknisk beskrivning (1.1.18.2) för arbetsgivardeklaration*: <https://www.skatteverket.se/foretag/arbetsgivare/lamnaarbetsgivardeklaration/tekniskbeskrivningochtesttjanst/tekniskbeskrivning11182.4.7eada0316ed67d7282a791.html>. Översiktssida med testtjänst: <https://www.skatteverket.se/foretag/arbetsgivare/lamnaarbetsgivardeklaration/tekniskbeskrivningochtesttjanst.4.309a41aa1672ad0c8377c8b.html>
- [Fältlista] *Bilaga Fältlista 1.1.18.1* (xlsx): <https://www.skatteverket.se/download/18.1522bf3f19aea8075ba80b/1765448363471/Bilaga%20F%C3%A4ltlista_1.1.18.1.xlsx>
- [Summering] *Bilaga Summering och beräkningsregler* (xlsx): <https://www.skatteverket.se/download/18.1522bf3f19aea8075ba503c/1768990791934/Bilaga%20Summering%20och%20Ber%C3%A4kningsregler..xlsx>
- Övriga bilagor: Kontroller <https://www.skatteverket.se/download/18.1522bf3f19aea8075ba80c/1765448380675/Bilaga%20Kontroller%201.1.18.1.xlsx>, Struktur XML-dokument <https://www.skatteverket.se/download/18.1522bf3f19aea8075ba80e/1765448416038/Bilaga%20Struktur,%20XML-dokument_1.1.18.1.xlsx>, Land- och ersättningskoder <https://www.skatteverket.se/download/18.1522bf3f19aea8075baa84/1765545277710/Bilaga%20Land-%20och%20ers%C3%A4ttningskoder.xlsx>
- [XSD] Instansdel <https://www.skatteverket.se/download/18.6491ed82199c4722dae6cc/1760546683779/arbetsgivardeklaration_1.1.xsd>, komponentdel <https://www.skatteverket.se/download/18.6491ed82199c4722dae6cd/1760546712759/arbetsgivardeklaration_component_1.1.xsd>
- [Exempelfiler] <https://www.skatteverket.se/download/18.1522bf3f19aea8075ba810/1765448451193/Exempelfiler_XML.zip> (19 filer, bl.a. 01 vanliga löntagare, 10 borttag IU, 18 HU+IU+frånvaro)
- [API] Skatteverket, *Tjänstebeskrivning Arbetsgivardeklaration inlämning v1.7*, dokumentversion 1.23, 2025-10-17: <https://www7.skatteverket.se/portal-wapi/open/apier-och-oppna-data/utvecklarportalen/v1/getFile/tjanstebeskrivning-agd-inlamning>
- [HRP] Skatteverket, *Beskrivning av tjänsten Arbetsgivardeklaration hantera redovisningsperiod*, dokumentversion 1.11, 2025-10-14: <https://www7.skatteverket.se/portal-wapi/open/apier-och-oppna-data/utvecklarportalen/v1/getFile/tjanstebeskrivning-agd-hantera-redovisningsperiod-v1/pdf/1.9.1/tjanstebeskrivning-agd-hantera-redovisningsperiod-v1.pdf>
- [Villkor] *Allmänna villkor för Skatteverkets API Arbetsgivardeklaration inlämning*, version 1.0 (fr.o.m. 2018-10-25): <https://www7.skatteverket.se/portal-wapi/open/apier-och-oppna-data/utvecklarportalen/v1/getFile/allmanna-villkor-arbetsgivardeklaration-inlamning>
- [SFL] Skatteförfarandelag (2011:1244), ändrad t.o.m. SFS 2026:1305: <https://data.riksdagen.se/dokument/sfs-2011-1244.text>
- Lag (2024:1300) om uppgiftsskyldighet i fråga om frånvaro på grund av vård av barn: <https://data.riksdagen.se/dokument/sfs-2024-1300.text>
- Lag (1930:173) om beräkning av lagstadgad tid: <https://data.riksdagen.se/dokument/sfs-1930-173.text>
