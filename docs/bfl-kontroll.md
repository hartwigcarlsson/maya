# BFL-kontroll

Krav som Bokföringslagen (SFS 1999:1078) och BFNAR 2013:2 ställer på Maya. Varje krav har ett id, och beslut, ADR:er, issues och commits hänvisar till kraven med det id:t. "Måste" är krav enligt BFL eller BFNAR. "Bör" är BFN:s rekommendation eller en följd för Maya.

Källhänvisningar och fullständig genomgång finns i forskningsrapporten [Vilka krav ställer BFL och BFNAR på ett bokföringssystem?](https://github.com/hartwigcarlsson/maya/blob/research/bfl-krav/docs/research/bfl-krav.md) ([#2](https://github.com/hartwigcarlsson/maya/issues/2)). Lagtexten gäller t.o.m. SFS 2024:342 och BFNAR 2024:1. Uppdatera listan när lagen eller de allmänna råden ändras.

## Så görs kontrollen

1. Gå igenom varje avsnitt nedan och avgör vilka krav förändringen **berör**: kod, schema, migrering, export/import, deployment eller ett beslut som formar något av det.
2. Varje berört krav måste vara **uppfyllt** av förändringen, eller ha ett öppet issue som hänvisar till id:t och visar hur det uppfylls senare.
3. Skriv resultatet i commit-meddelandets brödtext eller i ticketets lösningskommentar: `BFL: L1, V3 uppfyllda; A4 i #<issue>`. Om inget krav berörs: `BFL: ej berörd`.

## Bokföringsposter och låsning

- **L1** En bokförd post (verifikation och dess rader) är oföränderlig. Ingen kodväg får uppdatera eller radera låsta poster (BFL 5:1, 7:2; p. 2.1).
- **L2** Utkast får finnas, men räknas inte som bokfört. Låsning sker senast inom tillämplig tidsfrist (komm. s. 15; BFL 5:2; BFNAR kap. 3).
- **L3** Varje post visar registreringsordning, redovisningsperiod, verifikationsnummer, kontering och belopp (p. 2.3).
- **L4** Systemet kan ta fram hela bokföringen i registreringsordning (grundbok) och per konto i registreringsordning (huvudbok) (BFL 5:1; p. 2.2).
- **L5** Det finns en enda samlad huvudbok. Delsystem som reskontra och kassa bokförs in i den (komm. s. 18; p. 2.11).
- **L6** Redovisningsvalutan är SEK, eller EUR. Utländska belopp kan alltid presenteras omräknade till avistakurs eller tillåten kurs (BFL 4:6; p. 2.4–2.5).
- **L7** Varje verifikation är balanserad, det vill säga debet är lika med kredit. Detta är en följd av dubbel bokföring och huvudboken, inte en uttrycklig regel.

## Verifikationer

- **V1** Varje bokföringspost har en verifikation, även bokslutstransaktioner och rättelser (BFL 5:6, 5:10; p. 2.18).
- **V2** Verifikationen har fälten sammanställningsdatum, affärshändelsedatum, beskrivning, belopp, motpart, verifikationsnummer och referenser till underlag och var de finns. Motpart får utelämnas enligt p. 5.7 (BFL 5:7–8; p. 5.5–5.7).
- **V3** Verifikationsnummer tilldelas obrutet inom varje serie, utan luckor. Systemet kan visa att serien är obruten (p. 5.9).
- **V4** Serierna och deras tidsperioder är konfigurerade och dokumenterade (p. 5.10, 9.6).
- **V5** Mottagna underlag, alltså originalfiler, kopplas till verifikationen och sparas oförändrade i originalformat (BFL 5:6, 7:1).
- **V6** En komplettering av en verifikation loggar när den gjordes, av vem och vad som lades till. Det mottagna innehållet förändras aldrig (p. 5.8).
- **V7** En rättad eller ersatt verifikation sparas. Ursprunget framgår, liksom vem som rättade och när (BFL 5:9; p. 5.15–5.16).

## Rättelser och perioder

- **R1** Rättelser görs alltid med en ny rättelsepost som har en egen verifikation. Det gäller även makulering (p. 2.17–2.18).
- **R2** Rättelsen anger vem som rättade och när. Den rättade posten länkar till rättelsen och visar att den har rättats (BFL 5:5).
- **R3** Redovisningsperioder, som månader, kan stängas. Efter stängning bokförs rättelser i den öppna period där felet upptäcktes (komm. s. 19–20, 27).
- **R4** Systemet stöder och dokumenterar avstämningar, och avvikelser kan noteras (p. 2.15–2.16).

## Tidsfrister

- **T1** Systemet hjälper till att hålla fristerna. Kontanta betalningar bokförs senast påföljande arbetsdag, övrigt så snart det kan ske eller enligt vald senareläggning i p. 3.6–3.8. Låsning sker senast vid fristen (BFL 5:2).
- **T2** Om kontantmetoden stöds ska obetalda fordringar och skulder bokföras vid räkenskapsårets slut (BFL 5:2 tredje st.).

## Behandlingshistorik

- **H1** Registreringstidpunkt och användare sparas för varje bokföringspost (p. 9.16; BFL 5:5).
- **H2** Ändringar som påverkar postbehandlingen loggas med tidpunkt. Det gäller kontoplan, momssatser, automatkonteringsregler, serieinställningar, nya delsystem och programversion (p. 9.16; komm. s. 96).
- **H3** Behandlingshistoriken går inte att ändra och sparas lika länge som övrig räkenskapsinformation (BFL 1:2 p. 9, 7:2).

## Systemdokumentation

- **S1** Det finns kontoplan, beskrivning av hur konton sammanställs till årsredovisningen, samlingsplan och arkivplan (p. 9.2–9.4, 8.3).
- **S2** Automatkonteringar och andra behandlingsregler, som moms och periodiseringar, är dokumenterade (p. 9.9–9.10). Maya räknas inte som "enkelt system" (p. 9.5).
- **S3** Verifieringskedjan och informationsflödena (import, e-faktura, bank) är beskrivna (p. 9.7–9.8, 9.11).
- **S4** Det är beskrivet hur all elektronisk räkenskapsinformation tas fram på papper (p. 9.12).
- **S5** Det är beskrivet hur överföringar och konverteringar mellan format görs utan risk för förändring (komm. s. 70; BFL 7:6).
- **S6** Systemdokumentationen är versionerad och arkiveras som räkenskapsinformation (BFL 1:2 p. 9).

## Arkivering

- **A1** All räkenskapsinformation bevaras till och med 31 december det sjunde året efter det kalenderår då räkenskapsåret slutade. Systemet får inte erbjuda radering före det (BFL 7:2, 7:7).
- **A2** Allt kan presenteras i läsbar form och skrivas ut omedelbart, till exempel som PDF- eller utskriftsexport av grundbok, huvudbok, verifikationer, underlag och behandlingshistorik (BFL 7:1).
- **A3** Systemet och utrustningen som behövs för presentationen hålls tillgängliga hela bevarandetiden. Arkiverade år ska därför vara läsbara även efter uppgraderingar och schemamigreringar (BFL 7:2).
- **A4** Format- och systembyten, inklusive schemamigrering som ändrar format och export eller import till ett annat system, behandlas som överföringar. De ska verifieras, till exempel med checksummor och avstämning av summor, innan original kasseras (BFL 7:6; komm. s. 68–70).
- **A5** Det finns automatisk säkerhetskopiering, och kopian förvaras åtskild från originalet (p. 8.2).
- **A6** Data och kopior förvaras i Sverige. Förvaring i ett annat EU-land kräver anmälan till Skatteverket, förvaring utanför EU kräver tillstånd. Skatteverket ska kunna få omedelbar elektronisk åtkomst (BFL 7:2, 7:3 a, 7:4).
- **A7** Kryptering av vilande data är tillåten bara om klartext kan tas fram omedelbart. Nycklarna bevaras som räkenskapsinformation och dekryptering är möjlig under hela bevarandetiden (p. 7.8; komm. s. 72).

## Räkenskapsår och bokslut

- **Å1** Räkenskapsåret är 12 månader. Kalenderår eller brutet år kan väljas, och det första året får vara mellan 1 och 18 månader (BFL 3:1, 3:3).
- **Å2** Öppningsbalans registreras när bokföringen startar (BFL 4:3).
- **Å3** Bokslutstransaktioner bokförs med verifikation. Årsredovisningen enligt ÅRL och specifikationer av sammandragna balansposter kan tas fram och arkiveras (BFL 5:3, 6:1–2, 6:8).
- **Å4** Anläggningsregister med fälten i p. 4.5–4.7, uppdaterat senast vid bokslutet. Det kan utelämnas bara om bolaget har få tillgångar och bokföringen ändå ger överblick (p. 4.2–4.7).
- **Å5** Räkenskapsinformation som Maya genererar är på svenska eller engelska (BFL 1:4).
