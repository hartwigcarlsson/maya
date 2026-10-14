# Vilket Rust-bibliotek skapar Mayas PDF:er?

Forskning för ticketet [Vilket Rust-bibliotek skapar Mayas PDF:er?](https://github.com/hartwigcarlsson/maya/issues/65). Fakta är hämtade 2026-10-14 från crates.io, bibliotekens egna README:er och dokumentation samt källkoden i `~/.cargo/registry`. Mätningarna kommer från två små prov som byggdes på en Mac (Apple Silicon, rustc 1.99.0) och som inte är incheckade.

## Svar

**Använd `typst` som bibliotek (`typst` + `typst-pdf` 0.15.1, Apache-2.0).** Mallarna för kundfaktura, lönespecifikation och balans- och resultatrapport skrivs i Typst-markup, bäddas in i binären med `include_str!` och fylls med data via `sys.inputs`. Typst sköter radbrytning, tabeller som fortsätter över sidor, sidhuvud och sidfot med "Sida X av Y", typsnittsinbäddning med subsetting och PDF/A-2b. Inget externt program behövs vid körning.

Priset är binärstorleken, 30–35 MB (se [Mätningar](#mätningar)), och att Typst fortfarande är 0.x med API-brott mellan minor-versioner. Alternativet med lägst kostnad är `krilla`, som Typst själv använder för PDF-skrivningen. Där måste Maya då bygga egen layout, tabeller och sidbrytning.

## Krav som jämförs

- Fast mall för kundfakturan, med svensk text (å/ä/ö), logotyp och OCR-nummer ([kundfakturor](https://github.com/hartwigcarlsson/maya/issues/19)).
- Lönespecifikationer ([lönekörning](https://github.com/hartwigcarlsson/maya/issues/21)).
- Balans- och resultatrapporten, som kan bli flera sidor med tabeller ([balans- och resultatrapporten](https://github.com/hartwigcarlsson/maya/issues/69)).
- En enda körbar binär i Docker, utan externa program vid körning.
- BFL-kontrollens **A2** (allt kan tas fram i läsbar form, t.ex. som PDF) och **A3** (läsbart under hela bevarandetiden) i [docs/bfl-kontroll.md](../bfl-kontroll.md). PDF/A är det etablerade arkivformatet för A3.

## Kandidater

| | `typst` + `typst-pdf` | `krilla` | `printpdf` | `genpdf` | HTML till PDF med webbläsare |
|---|---|---|---|---|---|
| Version, senast släppt | 0.15.1, 2026-07-17 | 0.8.2, 2026-06-04 | 0.12.8, 2026-09-05 | 0.2.0, 2021-06-17 | – |
| Licens | Apache-2.0 | MIT OR Apache-2.0 | MIT | Apache-2.0 OR MIT | – |
| Nivå | Typsättning: markup, layout, tabeller, sidbrytning | 2D-grafik och text på angivna koordinater | Låg nivå (operationer), plus experimentell HTML-layout | Enkel dokumentmodell ovanpå `printpdf` 0.3 | Full HTML/CSS |
| Tabeller och sidbrytning | Ja, inbyggt (`table` med upprepat `table.header`) | Nej, uttryckligen utanför scope | Bara via `html`-funktionen, som "is still evolving" | Grundläggande | Ja |
| Typsnitt | Inbäddade, subsettade, OpenType | Inbäddade, subsettade, OpenType inkl. färgtypsnitt | Inbäddade, auto-subsetting | Via `rusttype` 0.8 | Ja |
| PDF/A | Alla PDF/A-delar, validerat vid export | PDF/A-1 till A-4, validerat vid export | Ingen standardkontroll | Nej | Nej |
| Externt program vid körning | Nej | Nej | Nej | Nej | Ja, Chromium |

### `typst` som bibliotek

- Typst är Apache-2.0-licensierat ([typst/typst](https://github.com/typst/typst)) och släpps ungefär två gånger per år: 0.14.0 2025-10-24 och 0.15.0 2026-06-15 ([crates.io](https://crates.io/crates/typst/versions)). Det är 0.x, och API:t ändras mellan minor-versioner. I provet hade till exempel `PagedDocument` flyttat till cratet `typst-layout`, och fälten hade blivit metoder.
- PDF-exporten sker i `typst-pdf`, som bygger på `krilla` (beroendet `krilla ^0.8.2` i [typst-pdf 0.15.1](https://crates.io/crates/typst-pdf/0.15.1/dependencies)). Typst 0.14 lade till stöd för alla PDF/A-standarder: PDF/A-1b, -1a, -2b, -2u, -2a, -3b, -3u, -3a, -4, -4f och -4e ([changelog 0.14.0](https://typst.app/docs/changelog/0.14.0/)). Standarden väljs i `PdfOptions::standards` ([PDF-referensen](https://typst.app/docs/reference/pdf/)). Bilagor (`pdf.attach`) stöds inte i PDF/A-2, bara i PDF/A-3 och senare (samma källa).
- Bilder: PNG, JPG, GIF, SVG, PDF, WebP och råa pixeldata ([image](https://typst.app/docs/reference/visualize/image/)). En logotyp som användaren laddat upp kan skickas in som bytes och avkodas med `image(bytes)`.
- Data kommer in i mallen via `sys.inputs` ([sys](https://typst.app/docs/reference/foundations/sys/)). Mallen och datat hålls därmed isär. Kundnamn och fakturarader ska aldrig klistras in i Typst-källkoden som text, eftersom de då tolkas som markup.
- Typst kräver att anroparen implementerar traitet `World` (källfiler, typsnitt, datum). Wrappern [`typst-as-lib`](https://github.com/Relacibo/typst-as-lib) (MIT, 0.17.0) gör det åt en, men dess README säger att API:t "is currently not really stable". Maya kan antingen använda den eller skriva en egen `World` på ett hundratal rader. Wrappern drar inte in nätverk så länge funktionerna `packages`, `ureq` och `reqwest` är avslagna ([crates.io-funktioner](https://crates.io/crates/typst-as-lib/0.17.0)).
- Typsnitt: `typst-assets` med funktionen `fonts` bäddar in Typsts standardtypsnitt (Libertinus Serif, New Computer Modern, DejaVu Sans Mono m.fl., 9,6 MB på disk). Maya bör i stället bädda in de få typsnitt mallarna använder.

### `krilla`

- Högnivå-API för PDF-skapande, testat mot veraPDF och Arlington-modellen i CI och mot sex PDF-läsare ([README](https://github.com/LaurenzV/krilla)).
- Stöder PDF/A-1, -2, -3, -4 och PDF/UA-1 med validering vid export (samma källa). I provet stoppade validatorn ett dokument som saknade datum.
- README:n säger uttryckligen att textlayout, tabeller, sidbrytning och sidhuvud och sidfot ligger utanför bibliotekets scope, och hänvisar till Typst för det. Med `krilla` direkt får Maya mäta text, bryta rader och dela tabeller över sidor själv.

### `printpdf`

- MIT, aktivt underhållet ([fschutt/printpdf](https://github.com/fschutt/printpdf)). Bäddar in typsnitt med Unicode och auto-subsetting.
- README:n listar "Conformance / error checking for various PDF standards" under det som inte stöds. Det finns alltså ingen PDF/A-validering.
- HTML-layouten (funktionen `html`, som är på som standard) hanterar tabeller och sidbrytning, men README:n säger att den "is still evolving" och rekommenderar manuell positionering för exakt kontroll.

### `genpdf`

- Senaste version 0.2.0 kom 2021-06-17 och bygger på `printpdf ^0.3.4`, `lopdf ^0.26` och `rusttype ^0.8` ([beroenden](https://crates.io/crates/genpdf/0.2.0/dependencies)), alltså versioner som är flera år gamla. Det finns en gaffel, `genpdfi` 0.2.7 (2026-01-27), men den är liten (cirka 38 000 nedladdningar totalt, [crates.io](https://crates.io/crates/genpdfi)). Ingen av dem har PDF/A. Avfärdas.

### HTML till PDF

- Alla verktyg som återger HTML och CSS fullt ut kräver en webbläsarmotor vid körning, t.ex. Chromium via CDP. Det bryter kravet på en enda binär och gör Docker-avbildningen flera hundra MB större. `wkhtmltopdf` är arkiverat på GitHub (senaste push 2022-11-22, [wkhtmltopdf/wkhtmltopdf](https://github.com/wkhtmltopdf/wkhtmltopdf)). Avfärdas.
- Den rena Rust-varianten är `printpdf`s `html`-funktion, se ovan.

## Mätningar

Två små prov byggdes i release-läge med `strip = true`.

**Typst-provet:** `typst`, `typst-pdf`, `typst-layout` 0.15.1 och `typst-as-lib` 0.17.0. Mallen är en faktura med kundnamn och OCR-nummer från `sys.inputs`, en tabell på 120 rader med upprepat huvud och en sidfot med "Sida X av Y", exporterad som PDF/A-2b.

- Resultatet blev 3 sidor och 96 624 byte på cirka 20–30 ms, efter kompilering av mallen.
- `pdftotext` gav tillbaka "Åsa Öberg, Västerås" och "räksmörgås" korrekt, och sidfoten löd "Sida 1 av 3" till "Sida 3 av 3".
- `pdffonts` visade Libertinus Serif Regular och Bold som inbäddade, subsettade och med Unicode-tabell.
- XMP-metadatan angav `pdfaid:part 2` och `pdfaid:conformance B`, `dc:language sv`.
- Två körningar gav byte-identiska filer (samma MD5). Utdatat är deterministiskt när datumet sätts i mallen.
- PDF/A-exporten avbröts med felet "PDF/A-2b error: missing document date" tills mallen satte `document(date: ...)`.
- Binären blev 44,7 MB med alla `typst-assets`-typsnitt och 35,6 MB med bara två inbäddade typsnitt (cirka 0,8 MB). Med `lto = true` och `codegen-units = 1` sjönk den till 30,1 MB.
- Första release-bygget tog 1 min 39 s på 7 kärnor (657 s CPU-tid). Därefter tog inkrementella byggen av själva provet under 2 s, och LTO-bygget tog 3 min 58 s (564 s CPU-tid, främst länkning).

**Krilla-provet:** `krilla` 0.8.2 med en rad text och ett inbäddat typsnitt, exporterad som PDF/A-2b.

- Binären blev 3,7 MB.
- Första bygget tog 20 s (118 s CPU-tid).
- Texten med å/ä/ö kom tillbaka korrekt, och typsnittet var inbäddat och subsettat.

Ingen extern PDF/A-validator (veraPDF) kördes. Överensstämmelsen bygger på bibliotekens egna validatorer, som krillas CI i sin tur kontrollerar mot veraPDF.

## Rekommendation och villkor

1. **Typst som bibliotek**, med mallar i `.typ`-filer inbäddade i binären och data via `sys.inputs`.
2. **PDF/A-2b** som standard för fakturor, lönespecifikationer och rapporter (A3). Det är bara en exportinställning och kostar inget extra. Beslutet i [Hur lagras underlag och data, och hur säkerhetskopieras de?](https://github.com/hartwigcarlsson/maya/issues/13) om att årsarkivet inte innehåller PDF/A i v1 står kvar. PDF/A-2b gäller bara de enskilda dokument Maya skapar. PDF/A-3b behövs bara om Maya vill bifoga filer i PDF:en, t.ex. en maskinläsbar faktura, eftersom Typst inte stöder bilagor i PDF/A-2.
3. **Lås exakta versioner** (`typst = "=0.15.1"` osv.). Uppgradering av Typst är ett eget arbete med API-ändringar, och genererade PDF:er kan se annorlunda ut efter den. En PDF som redan är utfärdad och arkiverad ska sparas som fil och inte genereras om, så att en uppgradering aldrig ändrar ett arkiverat dokument (A3).
4. **Sätt datum och språk i mallen** (`document(date: ...)`, `text(lang: "sv")`). PDF/A kräver datum, och ett fast datum ger byte-identisk utdata.
5. **Bädda in egna typsnitt** med en licens som tillåter inbäddning (t.ex. SIL OFL). Krillas PDF/A-validator kräver att typsnitten får bäddas in. Använd inte `typst-assets/fonts`, som lägger till cirka 9 MB.
6. **Räkna med 30–35 MB** för PDF-delen i binären (30 MB med LTO) och ungefär 1,5 minuter extra för ett första release-bygge. Cacha beroendena i Docker-bygget (cargo-chef eller motsvarande).
7. Fallback: om binärstorleken eller Typsts versionstakt blir ett problem kan Maya byta till `krilla` direkt. Då krävs egen kod för layout, tabeller och sidbrytning.

## Öppna frågor till senare tickets

- Om ett inbetalningskort med OCR-B-typsnitt ska skrivas ut behövs ett OCR-B-typsnitt med licens som tillåter inbäddning. Det här ticketet behandlar OCR-numret bara som text.
- Ingen veraPDF-validering ingår här. Ett test som kör veraPDF i CI mot mallarna vore ett bra skydd, och det kräver inget program vid körning.
