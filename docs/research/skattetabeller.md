# Skattetabeller: maskinell hämtning och beräkning av skatteavdrag

Ärende: [#6](https://github.com/hartwigcarlsson/maya/issues/6). Undersökt 2026-10-12 mot primärkällor (skatteverket.se, Skatteverkets EntryScape-katalog för öppna data, Skatteverkets utvecklarportal, riksdagen.se). Alla API-anrop nedan är provkörda samma dag.

## Kort svar

- **Tabellerna finns som öppna data (CC0) med ett fritt, nyckellöst REST-API** i Skatteverkets EntryScape-instans, och som CSV-filer. Ingen registrering, inget avtal. [1][2]
- **Uppdatering: en gång per år**, nästa års tabeller läggs in i mitten av december (2026 års data ändrades 2025-12-10). [1][3]
- **Programmet slår upp ett belopp, det räknar inte fram skatten.** Upp till 80 000 kr/mån anger tabellen skatteavdraget i kronor. Över det anges en procentsats. Engångsbelopp har egna procenttabeller. [4][5]
- **Det programmet själv måste implementera är reglerna runt uppslaget**: vilken tabell och kolumn som gäller, huvudinkomst kontra 30 %, engångsbelopp, jämkning (procentbeslut), förhöjt avdrag och SINK. Det är lagregler i skatteförfarandelagen, inte data. [6]
- **Vilken tabell och vilket jämkningsbeslut en viss anställd har** kommer från den anställdes beslut om A-skatt, eller maskinellt via Skatteverkets API *Fråga om skatteavdrag*. API:et kräver avtal med Skatteverket och OAuth2-nycklar. [7][8]

## 1. Källor för tabelldata

### 1.1 Öppna data via EntryScape (rekommenderat)

Skatteverket publicerar sina öppna data i en EntryScape-katalog (DCAT-AP-SE). Datamängden *Skattetabeller för månadslön* har två distributioner: CSV-filer per år och ett API. Licensen är CC0 och periodiciteten `ANNUAL`. Beskrivningen lyder: "Vi lägger till nästkommande års tabeller i mitten av december varje år." [1]

| Datamängd | Metadata | API (RowStore) |
|---|---|---|
| Skattetabeller för månadslön | `https://skatteverket.entryscape.net/store/9/metadata/5` | `https://skatteverket.entryscape.net/rowstore/dataset/88320397-5c32-4c16-ae79-d36d95b17b95` |
| Skatteavdrag på engångsbelopp (kolumn 1–6) | `https://skatteverket.entryscape.net/store/9/metadata/1005` | `https://skatteverket.entryscape.net/rowstore/dataset/91940080-6456-44fe-a10d-abee79196c48` |
| Skattesatser per kommun (kommunal-, region-, begravnings-, kyrkoavgift) | `https://skatteverket.entryscape.net/store/9/metadata/10` | `https://skatteverket.entryscape.net/rowstore/dataset/c67b320b-ffee-4876-b073-dd9236cd2a99` |
| Skattetabeller för tvåveckorslön | `https://skatteverket.entryscape.net/store/9/metadata/77` | (se metadata) |

Det finns också datamängder för sjöinkomst (närfart/fjärrfart), dagpenning och engångsbelopp för sjömän. De är inte relevanta för ett vanligt AB. [2]

**Hur API:et fungerar** (RowStore, auto-genererat Swagger 2.0 på `<dataset>/swagger`) [9]:

- `GET <dataset>/info` ger kolumnnamn och radantal (månadslön: 76 034 rader, år 2016 och framåt).
- `GET <dataset>/json?<kolumn>=<värde>&_limit=&_offset=` filtrerar på exakt värde eller reguljärt uttryck per kolumn. Svaret är JSON med `results`, `resultCount` och en `next`-länk. Servern begränsar `_limit` till 100 rader per sida (`_limit=1000` gav `limit: 100` vid provkörning).
- Kolumnerna i månadstabellen är `år`, `antal dgr`, `tabellnr`, `inkomst fr.o.m.`, `inkomst t.o.m.`, `kolumn 1` … `kolumn 7`. Alla värden är strängar. Kolumn 7 användes bara 2023 och är tom för 2026.
- Exempel (provkört): `.../json?år=2026&tabellnr=33&antal dgr=30B&inkomst fr.o.m.=30001` ger en rad: kolumn 1 = 5928 kr.

**CSV-filerna** (en per år, t.ex. 2026: `https://skatteverket.entryscape.net/store/9/resource/2615`) [1]:

- Semikolonseparerad, rubrikrad `År;Antal dgr;Tabellnr;Inkomst fr.o.m.;Inkomst t.o.m.;Kolumn 1;…;Kolumn 7`.
- **Teckenkodning ISO-8859-1**, inte UTF-8 (rubriken börjar med byte `0xC5` = "Å"). Detta är verifierat genom provnedladdning.
- 2026 har 7 966 datarader, tabell 29–42. Den sista %-raden per tabell har tomt `Inkomst t.o.m.` (öppet intervall uppåt).
- Att ladda ned årets CSV en gång och lagra den lokalt är enklare än att paginera API:et (cirka 80 sidor).

### 1.2 Filer på skatteverket.se ("specialversioner för programföretag")

Skatteverket publicerar samma tabeller som PDF, TXT (fast postlängd) och XLSX per år, plus en postbeskrivning och den tekniska beskrivningen SKV 433. [3] Postbeskrivningen för TXT-filen [10]:

| Pos | Längd | Innehåll |
|---|---|---|
| 1 | 2 | `14` = 14-dagarstabell, `30` = 30-dagarstabell |
| 3 | 1 | `B` = skatt i belopp, `%` = skatt i procent |
| 4 | 2 | Tabellnummer |
| 6 | 7 | Lägsta inkomst i intervallet |
| 13 | 7 | Högsta inkomst i intervallet |
| 20, 25, 30, 35, 40, 45 | 5 vardera | Skatt kolumn 1–6 |

Fältet "Antal dgr" i CSV/API (`30B`, `30%`) är alltså positionerna 1–3 i TXT-formatet. Nedladdnings-URL:erna på skatteverket.se innehåller id-nummer som ändras varje år, så de går inte att förutsäga. EntryScape är stabilare att bygga mot.

## 2. Tabellernas uppbyggnad

**Tabellnummer** = summan av kommunalskatt, regionskatt, begravningsavgift och (för medlemmar i Svenska kyrkan) kyrkoavgift, avrundat till heltal. 32,50 ger tabell 32 och 32,51 ger tabell 33. Månadstabellerna har nummer 29–42 för 2026. [4] Skattesatserna per kommun och församling finns i datamängden ovan. I 2026 års filer finns inte längre församlingskod. [3]

**Kolumner (2026)** [5, avsnitt 3]:

| Kolumn | Avser |
|---|---|
| 1 | Lön m.m. till den som vid årets ingång inte fyllt 66 år (jobbskatteavdrag) |
| 2 | Pension m.m. till den som fyllt 66 år |
| 3 | Lön m.m. till den som fyllt 66 år (förhöjt jobbskatteavdrag). Även för personer födda 1937 eller tidigare med förvärvsinkomst |
| 4 | Sjuk- och aktivitetsersättning, under 66 år |
| 5 | Andra pensionsgrundande ersättningar (t.ex. a-kassa), född 1938 eller senare |
| 6 | Pension m.m. till den som inte fyllt 66 år |

För Mayas fall, lön till en ägare eller anställd i ett AB, gäller normalt kolumn 1 (under 66) eller kolumn 3 (66 och äldre). Kolumnen bestäms av arbetsgivaren utifrån ålder och typ av ersättning. Den returneras inte av *Fråga om skatteavdrag* (se 4.2). [5][8]

**Intervall och belopp/procent** [5, avsnitt 5 och 9]:

- Månadstabellen: skatteavdrag i **kronor** upp till 80 000 kr. Intervallen är 100 kr upp till 20 000 kr och 200 kr mellan 20 001 och 80 000 kr. Över 80 000 kr anges **procent**, och intervallen bryts där procentsatsen ändras.
- Tvåveckorstabellen: kronor upp till 40 000 kr (intervall 50 kr upp till 10 000 kr, därefter 100 kr), procent över 40 000 kr.
- Halvmånadslön: dubbla lönen, läs av i månadstabellen och halvera avdraget. Veckolön: dubbla lönen, läs av i tvåveckorstabellen och halvera avdraget.

Exempel från 2026 års CSV, tabell 33: `2026;30%;33;80001;81000;33;33;29;29;39;39;`. En månadslön mellan 80 001 och 81 000 kr ger alltså 33 % i kolumn 1.

## 3. Hur skatteavdraget räknas fram och vad programmet själv måste implementera

### 3.1 Själva tabellberäkningen behöver inte implementeras

Skatteverket fastställer tabellerna genom föreskrift (för 2026: SKVFS 2025:20). [5] Enligt lag ska de grunda sig på kommunal och statlig skatt, begravningsavgift, allmän pensionsavgift, trossamfundsavgift och public service-avgift, med grundavdrag och skattereduktioner (jobbskatteavdrag m.m.). [6, 12 kap. 3 §] SKV 433 beskriver hela den beräkningen steg för steg för programföretag som vill räkna själva: årsinkomst = intervallets övre gräns × 12, grundavdrag, statlig skatt 20 % över 643 000 kr, jobbskatteavdrag, public service-avgift (högst 1 184 kr 2026) och fördelning per löneperiod. [5, avsnitt 6–8] **Maya behöver inte göra detta.** Uppslag i den publicerade tabellen är den legala metoden. SKV 433 behövs bara om man vill räkna exakt ovanför 80 000 kr (se öppna frågor).

### 3.2 Regler som programmet måste implementera (skatteförfarandelagen 2011:1244)

| Situation | Regel | Källa |
|---|---|---|
| Huvudinkomst, regelbunden lön | Skatteavdrag enligt beslut om A-skatt, dvs. allmän skattetabell | SFL 11 kap. 16–17 §§ [6] |
| Januari | Får använda den tabell som senast tillämpades föregående år | SFL 11 kap. 16 § 2 st. [6] |
| Ej huvudinkomst (sidoinkomst), arbete kortare än en vecka m.m. | **30 %** av underlaget | SFL 11 kap. 20 § [6] |
| Ersättning som inte är regelbunden (engångsbelopp) | Särskild tabell för engångsbelopp, i procent | SFL 11 kap. 17 § och 12 kap. 6 § [6] |
| Skatteverket har beslutat en procentsats (jämkning) | Avdrag med beslutad procent av inkomsten. Gäller även engångsbelopp | [7][11] |
| Anställd har inte visat beslut trots begäran, och utbetalaren har inte hämtat uppgift från Skatteverket | **Förhöjt avdrag**: tabellen för orten där utbetalaren har sitt kontor + 10 % av tabellavdraget | SFL 11 kap. 21 § [6] |
| Anställd begär högre avdrag | Frivillig förhöjning, senast från utbetalning som infaller två veckor efter begäran | SFL 11 kap. 22 § [6] |
| Ersättning under 100 kr, eller under 1 000 kr totalt under året | Inget skatteavdrag | SFL 10 kap. 4 § [6] |
| SINK-beslut finns | 22,5 % (2026), 20 % från 2027-01-01. Utan beslut gäller vanlig preliminärskatt | Lag (1991:586) 7 §; SFL 13 kap. 1–3 §§ [12][13] |

Underlaget för skatteavdrag är kontant ersättning plus skattepliktiga förmåner, värderade enligt inkomstskattelagen. [6, 11 kap. 4 och 9–11 §§] Det innebär att Maya måste summera lön och förmånsvärden innan tabelluppslaget.

### 3.3 Engångsbelopp

Exempel på engångsbelopp är retroaktiv lön, semesterersättning, tantiem, provision och avgångsvederlag. Om mottagaren har tidsbestämd lön används engångstabellen. Om mottagaren saknar sådan lön dras 30 %. Procentsatsen väljs efter **årslönen**, dvs. förväntad total bruttoinkomst från huvudarbetsgivaren under hela året, inklusive förmåner, detta och tidigare engångsbelopp. Den räknas som om personen varit anställd hela året. [11] Engångstabellen för 2026, kolumn 1: 0 % upp till 25 041 kr, 10 %, 21 %, 26 %, 34 %, och 54 % över 660 000 kr. [5, avsnitt 10] Samma tabell finns i engångsbelopps-API:et (fält `kolumn`, `årslön i kr lägst/högst`, `preliminärt skatteavdrag i procent av engångsbeloppet`). [1]

## 4. API:er hos Skatteverket som kräver avtal

### 4.1 Fråga om skatteavdrag i kronor (FOSIK) 1.0

Tjänsten tar emot inkomstår, typ (2 = månadslön), tabell, kolumn (1–6) och bruttobelopp, och svarar med skatteavdraget i hela kronor (öre trunkeras). Den kan också gå baklänges från nettobelopp till bruttobelopp. Upp till 1 000 frågor kan skickas per POST. URL: `https://api.skatteverket.se/inkomstbeskattning/fraga-om-skatteavdrag-i-kronor/v1`. Sandlådan är öppen för alla. Produktion kräver avtal och client_id/client_secret med OAuth2. [14] Tjänsten är ett bekvämt sätt att få exakta kronbelopp även ovanför procentgränsen, men den kräver avtal och nätåtkomst vid varje lönekörning.

### 4.2 Fråga om skatteavdrag (FOS) Partner API 2.0

För upp till 1 000 personnummer per anrop returnerar tjänsten skatteform (A/F/FA), skattetabell och eventuellt procentbeslut för jämkning med giltighetstid. Den returnerar **inte kolumn**. Jämkningsbeslut uppdateras dagligen och tabeller årsvis. Autentiseringen är OAuth2 Client Credentials. Produktion kräver att man uppfyller API:ets allmänna villkor och har ingått avtal med Skatteverket. Felbeskrivningar är kodade i ISO-8859-1. [8] Detsamma finns som e-tjänst för arbetsgivare. [7]

## Öppna frågor (möjliga nya ärenden)

1. **Avrundning vid procentrader.** De källor jag hittade säger inte hur ett procentavdrag (lön över 80 000 kr/mån) ska avrundas till kronor. SKV 433 avsnitt 9 säger att avdraget även där "får beräknas" exakt enligt metoden med 200-kronorsintervall. FOSIK trunkerar öre. Beslutet kan vara att använda procent och avrunda nedåt, men det är inte belagt.
2. **Ska Maya använda FOS-API:et?** Det kräver avtal med Skatteverket. Det är troligen överdrivet för ett enmansbolag där ägaren själv vet sin tabell. Alternativet är att användaren matar in tabellnummer, kolumn och eventuell jämkningsprocent från sitt beslut om A-skatt.
3. **Hur ska årsskiftet hanteras?** Nya tabeller publiceras i mitten av december. Januarilön får dras enligt föregående års tabell (SFL 11 kap. 16 §). Maya behöver en årlig import och en regel för vilket års tabell som gäller vid utbetalning.
4. **Förmåner i underlaget.** Tabelluppslaget kräver underlag inklusive förmånsvärden (t.ex. bilförmån). Det hänger ihop med hur Maya modellerar lön och förmåner.
5. **Rätt tabell vid förhöjt avdrag.** Den tabell som "närmast motsvarar" skattesatsen på utbetalarens ort måste kunna härledas ur skattesatsdatamängden (kommun → summa → avrundning).

## Källor

1. Skatteverket, EntryScape-katalog: *Skattetabeller för månadslön* (dataset 5, distribution 95 CSV och 97 API, modifierad 2025-12-10). Även *Skatteavdrag på engångsbelopp* (1005/1007/1009) och *Skattesatser per kommun* (10). `https://skatteverket.entryscape.net/store/9/metadata/5?format=application/json`, `.../metadata/95`, `.../metadata/97`, `.../metadata/1005`, `.../metadata/1009`, `.../metadata/10` (hämtade 2026-10-12).
2. Skatteverket, EntryScape-sökning efter datamängder: `https://skatteverket.entryscape.net/store/search?type=solr&query=...` (hämtad 2026-10-12).
3. Skatteverket, *Teknisk beskrivning för skattetabeller* (specialversioner för programföretag): https://skatteverket.se/specialversionerforprogramforetagmfl.4.319dc1451507f2f99e86ee.html
4. Skatteverket, *Skattetabeller* (arbetsgivare): https://www.skatteverket.se/foretag/arbetsgivare/arbetsgivaravgifterochskatteavdrag/skattetabeller
5. Skatteverket, *Teknisk beskrivning SKV 433, 2026, utgåva 36* (daterad 2025-12-10): https://www.skatteverket.se/download/18.1522bf3f19aea8075ba55c/1766385913260/teknisk-beskrivning-skv-433-2026-utgava-36.pdf
6. Skatteförfarandelagen (2011:1244), 10 kap. 4 §, 11 kap., 12 kap., 13 kap.: https://data.riksdagen.se/dokument/sfs-2011-1244.text
7. Skatteverket, *Skatteavdrag* (arbetsgivare): https://www.skatteverket.se/foretag/arbetsgivare/arbetsgivaravgifterochskatteavdrag/skatteavdrag.4.233f91f71260075abe8800021121.html
8. Skatteverket, *Tjänstebeskrivning Fråga om skatteavdrag Partner API 2.0* (dokumentversion 1.9, 2025-10-16): https://www7.skatteverket.se/portal-wapi/open/apier-och-oppna-data/utvecklarportalen/v1/getFile/tjanstebeskrivning-fraga-om-skatteavdrag
9. RowStore Swagger för månadstabellen: https://skatteverket.entryscape.net/rowstore/dataset/88320397-5c32-4c16-ae79-d36d95b17b95/swagger (provkörd 2026-10-12).
10. Skatteverket, *Postbeskrivning allmänna tabeller 2026*: https://www.skatteverket.se/download/18.1522bf3f19aea8075ba5b8/1765287252247/postbeskrivning-allmanna-tabeller.doc
11. Skatteverket, *Engångsbelopp*: https://www.skatteverket.se/privat/skatter/arbeteochinkomst/askattsedelochskattetabeller/engangsbelopp.4.70ac421612e2a997f85800014872.html
12. Lag (1991:586) om särskild inkomstskatt för utomlands bosatta, 7 §: https://data.riksdagen.se/dokument/sfs-1991-586.text
13. Skatteverket, *SINK*: https://www.skatteverket.se/privat/internationellt/bosattutomlands/sinksarskildinkomstskattforutomlandsbosatta.4.6fdde64a12cc4eee23080002583.html
14. Skatteverket, *Tjänstebeskrivning Fråga om skatteavdrag i kronor 1.0* (dokumentversion 1.2, 2024-02-07): https://www7.skatteverket.se/portal-wapi/open/apier-och-oppna-data/utvecklarportalen/v1/getFile/tjanstebeskrivning-fraga-om-skatteavdrag-i-kronor-v1
