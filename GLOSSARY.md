# Maya

Maya är ett self-hostat bokföringssystem för svenska aktiebolag som följer Bokföringslagen (BFL) och är byggt för att låta AI automatisera bokföringen.

## Language

**Kontoplan**:
Ett företags egen uppsättning konton. Den kopieras från BAS-kontoplanen som Maya har inbyggd när företaget läggs upp, och varje konto är aktivt eller inaktivt; ett konto raderas aldrig.
_Avoid_: Kontolista, chart of accounts

**Eget konto**:
Ett konto som företaget har skapat på ett nummer som inte finns i BAS-kontoplanen; kontoklassen som numret hör till avgör vad det är för slags konto.
_Avoid_: Användarkonto, anpassat konto, underkonto (som samlingsnamn)

**Grundkonto**:
Ett konto som BAS markerar som en del av basuppsättningen (■). Grundkontona är aktiva när ett nytt företag läggs upp.
_Avoid_: Baskonto, standardkonto

**Reserverat konto**:
Ett konto som Maya lägger in i varje företags kontoplan för sina egna flöden; det kan inte skapas som eget konto, och det kan inte byta namn, inaktiveras eller få en annan koppling.
_Avoid_: Systemkonto, Maya-konto, spärrat konto

**Inaktivt konto**:
Ett konto som finns i företagets kontoplan men inte kan bokföras på förrän någon aktiverar det; ett utkast får använda det.
_Avoid_: Dolt konto, spärrat konto

**Installation**:
En Maya-server med sina användare och företag; backup och återställning gäller alltid hela installationen, aldrig ett enskilt företag.
_Avoid_: Instans, server (som samlingsnamn), tenant

**Användare**:
En person som loggar in i Maya och har åtkomst till ett eller flera företag; en användare raderas aldrig, bara avaktiveras.
_Avoid_: Konto, kund, medlem

**Åtkomst**:
En användares rätt att läsa och föra ett visst företags bokföring; den som har åtkomst till ett företag kan ge och ta åtkomst till det för andra.
_Avoid_: Behörighet, roll, medlemskap

**Inbjudan**:
En engångslänk som låter en ny användare registrera sin första passkey och därmed få den åtkomst som inbjudaren gav.
_Avoid_: Registrering, invite

**Färsk passkey**:
En passkey som användaren har använt i den aktuella sessionen de senaste 5 minuterna; krävs för att hantera passkeys, bjuda in och ändra åtkomst.
_Avoid_: Ny passkey, nyligen inloggad

**Avaktivera**:
Att ta bort all åtkomst, alla sessioner och alla token för en användare utan att radera hen, så att behandlingshistoriken fortfarande pekar på hen.
_Avoid_: Radera, ta bort, spärra

**Aktör**:
Den som utför en handling i Maya: en användare genom en viss kanal (webb, AI via MCP, CLI) eller Maya självt; varje post i behandlingshistoriken anger sin aktör.
_Avoid_: Principal, actor, utförare

**Ansluten klient**:
Ett AI-program som en användare har godkänt att nå utvalda företag via MCP; användaren kan ändra urvalet och återkalla klienten.
_Avoid_: Integration, app, token

**Företag**:
Ett aktiebolag vars bokföring förs i Maya; en användare kan ha åtkomst till flera, och flera användare kan ha åtkomst till samma.
_Avoid_: Bolag, organisation, tenant, klient

**Räkenskapsår**:
Den period ett företags bokföring avslutas för, med ingående och utgående balanser.
_Avoid_: Bokföringsår, verksamhetsår

**Stänga**:
Att avsluta ett räkenskapsår så att årets resultat bokförs, alla dess perioder låses, nästa års ingående balans skrivs och årsarkivet skapas; ett stängt räkenskapsår öppnas aldrig igen.
_Avoid_: Låsa året, bokslut (som handling), årsavslut

**Ingående balans**:
Saldot på varje balanskonto när ett räkenskapsår börjar; den är inget verifikat. För företagets första räkenskapsår i Maya skrivs den in eller importeras, och för senare år skrivs den när föregående år stängs och är preliminär fram till dess.
_Avoid_: IB-verifikat, öppningsverifikat, startsaldo

**Import**:
När ett företags bokföring för ett räkenskapsår förs över från ett annat system med en SIE4-fil; den görs en gång i ett tomt företag, bokför de överförda verifikaten och arkiverar filen tillsammans med en avstämning av alla summor.
_Avoid_: Migrering (som term), inläsning, SIE-import (i UI)

**Övertagande**:
När Maya tar över ett befintligt bolags löpande bokföring från ett annat system, från och med en övertagandeperiod: vid byte vid årsskiftet räkenskapsårets första period, vid byte mitt i året månaden efter det senaste importerade verifikatet. Det gamla systemet har hanterat momsdeklarationer, arbetsgivardeklarationer och avskrivningar före den.
_Avoid_: Migrering, systembyte (som term), startdatum

**Övertagen faktura**:
En kund- eller leverantörsfaktura som var obetald i det tidigare systemet vid övertagandet och som läggs in i reskontran med sitt återstående belopp; är den redan bokförd där bokförs den inte igen.
_Avoid_: Ingående faktura, öppen post, historisk faktura

**Verifikat**:
En bokföringspost som dokumenterar en affärshändelse med debet- och kreditrader; ett bokfört verifikat ändras aldrig, bara rättas genom ett nytt verifikat.
_Avoid_: Verifikation, voucher, transaktion, journal entry

**Utkast**:
Ett förslag till verifikat som ännu inte är bokfört och som en användare måste godkänna, t.ex. ett som AI har skapat.
_Avoid_: Förslag, draft, preliminärt verifikat

**Bokföra**:
Att godkänna ett utkast eller skapa ett verifikat direkt så att det låses och får sitt verifikationsnummer; först då räknas det som bokfört.
_Avoid_: Godkänna (som samlingsnamn), låsa, posta, attestera

**Verifikationsnummer**:
Ett verifikats löpnummer inom räkenskapsåret, tilldelat utan luckor i den ordning verifikaten bokförs och skrivet med prefixet V (V1, V2 …).
_Avoid_: Verifikatnummer, löpnummer, id

**Verifikatdatum**:
Dagen då affärshändelsen inträffade; avgör vilken period och vilket räkenskapsår verifikatet hör till.
_Avoid_: Bokföringsdatum, transaktionsdatum, affärshändelsedatum (i UI)

**Kontantfrist**:
Sista dag att bokföra ett utkast som för pengar in i eller ut ur kassan: första arbetsdagen efter verifikatdatumet.
_Avoid_: Kassafrist, nästa-dag-regeln

**Rättelse**:
Ett verifikat som vänder samtliga rader i ett tidigare bokfört verifikat och är länkat till det; det rättade verifikatet visar att det är rättat.
_Avoid_: Makulering, korrigering, ändring

**Komplettering**:
Ett underlag som kopplas till ett verifikat efter att det har bokförts, utan att verifikatet i övrigt förändras.
_Avoid_: Tillägg, bilaga

**Källreferens**:
Ett importerat verifikats serie, nummer, registreringsdatum och signatur i det system det kom från; den visas på verifikatet men ersätter aldrig verifikationsnumret.
_Avoid_: Originalnummer, externt id, gammalt verifikationsnummer

**Dimension**:
En uppdelning av bokföringen utöver kontot, som kostnadsställe eller projekt, där en verifikatrad märks med ett objekt; Maya för inga dimensioner, och en import släpper dem.
_Avoid_: Kostnadsbärare, objekt (som samlingsnamn), tagg

**Period**:
En kalendermånad inom ett räkenskapsår; en låst period kan inte få fler verifikat.
_Avoid_: Redovisningsperiod, månad (som term)

**Låsa**:
Att slutgiltigt spärra en period så att inga fler verifikat kan bokföras med ett datum i den; perioderna i ett räkenskapsår låses i ordning och låses aldrig upp.
_Avoid_: Stänga (om en period), periodstänga, frysa

**Underlag**:
Den handling som styrker en affärshändelse (kvitto, faktura, avtal) och som arkiveras kopplad till ett eller flera verifikat.
_Avoid_: Bilaga, kvitto (som samlingsnamn), dokument

**Årsarkiv**:
Ett oföränderligt, fristående paket med ett räkenskapsårs bokföring, underlag och behandlingshistorik som skapas när året stängs och går att läsa och skriva ut utan Maya; en senare komplettering läggs i ett tillägg till arkivet i stället för att ändra det.
_Avoid_: Bokslutsexport, årsbackup, säkerhetskopia

**Arkivplan**:
Beskrivningen av var ett företags räkenskapsinformation, säkerhetskopior och årsarkiv finns, hur de nås och vem som ansvarar för dem; den ingår i systemdokumentationen och i varje årsarkiv.
_Avoid_: Lagringsplan, backupplan

**Systemdokumentation**:
Beskrivningen av ett företags kontoplan, behandlingsregler och arkivplan, som Maya tar fram ur de regler som gäller; den visar vilken version av Maya den gäller för och ingår i varje årsarkiv.
_Avoid_: Användarmanual, systembeskrivning

**Bokföringsmetod**:
Företagets val mellan fakturametoden och kontantmetoden, som styr när fakturor bokförs.

**Kundfaktura**:
En utgående faktura till en kund; den kan skapas i Maya eller i ett annat system, men bokförs alltid i Maya.
_Avoid_: Utgående faktura, försäljningsfaktura

**Leverantörsfaktura**:
En inkommande faktura från en leverantör som registreras och bokförs i Maya.
_Avoid_: Inkommande faktura, inköpsfaktura

**Kund**:
Den som ett företag säljer till och skickar kundfakturor till; registreras per företag.
_Avoid_: Köpare, beställare, klient

**Kundreskontra**:
Företagets register över kundfakturor och vad som återstår att betala på dem; allt i den bokförs in i huvudboken.
_Avoid_: Kundregister, fordringslista

**Utfärda**:
Att färdigställa en kundfaktura som skapats i Maya så att den låses, får sitt fakturanummer och sin PDF; därefter ändras den bara genom en kreditfaktura.
_Avoid_: Skicka (som samlingsnamn), godkänna, fakturera

**Kreditfaktura**:
En kund- eller leverantörsfaktura som helt eller delvis upphäver en tidigare faktura och hänvisar till den; den är en ny affärshändelse och inte en rättelse.
_Avoid_: Kreditnota, negativ faktura, makulering

**Leverantör**:
Den som ett företag köper från och tar emot leverantörsfakturor från; registreras per företag.
_Avoid_: Säljare, leverantörskund, motpart (som term)

**Leverantörsreskontra**:
Företagets register över leverantörsfakturor och vad som återstår att betala på dem; allt i den bokförs in i huvudboken.
_Avoid_: Leverantörsregister, skuldlista

**Registrera**:
Att färdigställa en leverantörsfaktura så att den låses i leverantörsreskontran; under fakturametoden bokförs den i samma steg.
_Avoid_: Attestera, godkänna, ta emot

**Okopplat underlag**:
Ett uppladdat och arkiverat underlag som ännu inte hör till något verifikat eller någon faktura.
_Avoid_: Inkorgsdokument, obehandlat kvitto, lös bilaga

**Fakturauppgifter**:
Det ett företag anger under Inställningar → Företag för sina kundfakturor utöver adress, säte, momsreg-nr och F-skatt: betalsätt, OCR-referens på eller av, logotyp, svarsadress och standardmeddelande för fakturamejl; en utfärdad kundfaktura kopierar in det den använder.
_Avoid_: Fakturaprofil, fakturahuvud

**Omvänd skattskyldighet**:
Att köparen och inte säljaren redovisar momsen på en faktura: för en kundfaktura bygg inom Sverige eller EU-tjänst (0 % moms, intäkten på 3231 eller 3308, köparens momsreg-nr och texten "Omvänd betalningsskyldighet" på fakturan), för en leverantörsfaktura dessutom EU-vara och tjänst från land utanför EU; kunden och leverantören har ett förval.
_Avoid_: Reverse charge, omvänd moms

**Momsläge**:
Hur en kundfaktura har moms: med momssatserna, med omvänd skattskyldighet, eller utan moms för ett företag som inte är momsregistrerat på fakturadatumet; den utfärdade fakturan bär sitt momsläge.
_Avoid_: Momstyp, momskod

**Betalsätt**:
Ett sätt som en faktura kan betalas på: bankgiro, plusgiro eller IBAN med BIC.
_Avoid_: Betalningsmetod, betalkanal

**Utskick**:
Ett mejl med en utfärdad kundfaktura som Maya skickade eller försökte skicka, eller en markering att fakturan lämnades på annat sätt (post, personligen, kundens portal); det loggas med tid, mottagare, vem och resultat och är inget tillstånd på fakturan.
_Avoid_: Leverans, distribution, skickad (som status)

**Kursdifferens**:
Skillnaden mellan det som återstår av en leverantörsfaktura i originalvaluta, registrerad i SEK, och det som betalades i SEK; den stängs i betalningens verifikat mot 3960 vid vinst och 7960 vid förlust, så att inget återstår.
_Avoid_: Valutadifferens, kursavvikelse, valutajustering

**Kundförlust**:
En konstaterad förlust på det som återstår av en utfärdad kundfaktura, med skäl (konkurs, ackord, utmätning utan resultat eller annat) och datum; när fordran är bokförd bokförs den på 6351 med utgående moms minskad proportionellt, och den kan återkallas med ett nytt verifikat så att fakturan kan betalas igen. En befarad kundförlust är ett manuellt verifikat och ingen kundförlust i Maya.
_Avoid_: Avskrivning (det är anläggningstillgångar), osäker fordran, nedskrivning av kundfordran

**Tillgodohavande**:
Det som motparten har att få tillbaka när det som återstår av en faktura är negativt, till exempel när en betald faktura krediteras; det återbetalas som en betalning åt andra hållet och visas på fakturan och kunden.
_Avoid_: Kreditsaldo, överskott, negativ fordran

**Återbetalning**:
En betalning åt andra hållet av ett tillgodohavande: för en kund 1510 mot betalkontot, för en leverantör betalkontot mot 2440.
_Avoid_: Utbetalning (som term), retur, kreditering

**Anställd**:
En person som får lön eller styrelsearvode från ett företag, även en ägare; registreras per företag.
_Avoid_: Personal, medarbetare, löntagare

**Lönekörning**:
En utbetalning av lön till en eller flera anställda med en gemensam utbetalningsdag, som blir ett verifikat när den bokförs.
_Avoid_: Lönebatch, löneperiod, lön (som term)

**Lönespecifikation**:
En anställds del av en lönekörning, med lönerader, skatteavdrag och nettolön; den lämnas till den anställda och arkiveras som underlag.
_Avoid_: Lönebesked, lönespec

**Lönerad**:
En rad på en lönespecifikation av en viss löneart, t.ex. månadslön, sjukavdrag, förmån, utlägg eller retroaktivt tillägg.
_Avoid_: Lönepost, lönetransaktion

**Fastställa**:
Att låsa en lönekörning så att dess lönespecifikationer skapas; den bokförs i ett senare steg, tidigast på utbetalningsdagen.
_Avoid_: Attestera, godkänna, låsa

**Arbetsgivardeklaration**:
Företagets deklaration till Skatteverket av löner, förmåner, skatteavdrag och arbetsgivaravgifter för en utbetalningsmånad, med en uppgift per anställd; kallas AGI.
_Avoid_: Arbetsgivaruppgift, månadsuppgift, skattedeklaration

**Momsperiod**:
Den period (månad, kvartal eller beskattningsår) som ett företag redovisar moms för; den består av en eller flera hela perioder.
_Avoid_: Redovisningsperiod, deklarationsperiod, period (utan förled)

**Momsdeklaration**:
Företagets redovisning av moms till Skatteverket för en momsperiod, med beloppen per ruta räknade ur saldona på de konton som kopplingstabellen för moms pekar ut; den är ej deklarerad, deklarerad eller inlämnad.
_Avoid_: Momsrapport (om själva deklarationen), skattedeklaration, momsredovisning

**Deklarera**:
Att färdigställa en momsdeklaration så att momsavräkningen bokförs och filen till Skatteverket skapas; därefter kan inget som påverkar dess rutor bokföras i momsperioden förrän någon öppnar deklarationen igen.
_Avoid_: Lämna in (som synonym), skicka, stänga momsen

**Inlämnad**:
En momsdeklaration eller periodisk sammanställning som användaren har markerat som lämnad och signerad hos Skatteverket; Maya kan inte själv se att den är lämnad.
_Avoid_: Skickad, signerad, rapporterad

**Momsavräkning**:
Verifikatet som en momsdeklaration bokför på momsperiodens sista dag och som för över periodens moms till redovisningskontot för moms; när deklarationen öppnas igen vänds det genom en rättelse.
_Avoid_: Momsomföring, momsbokning, nollställning

**Kopplingstabell för moms**:
Företagets tabell över vilken ruta i momsdeklarationen varje konto bidrar till, högst en per konto; den kopieras från Mayas standardtabell när företaget läggs upp.
_Avoid_: Momskod, momsmappning, momsattribut

**Förvald moms**:
Det momskonto, eller ingen moms, som Maya föreslår när ett visst konto konteras; den bestäms per företag och påverkar aldrig momsdeklarationens rutor.
_Avoid_: Momskod, momsmappning, standardmoms

**Justeringspar**:
Två motsatta rader med samma belopp, en på ett konto som är kopplat till en ruta och en på ett reserverat konto i samma kontogrupp, som lägger ett underlag i rutan utan att ändra resultatet.
_Avoid_: Spegelkonto, motkontopar, momsjustering

**Periodisk sammanställning**:
Företagets redovisning till Skatteverket per kalenderkvartal av tjänster sålda till köpare i andra EU-länder, per köpare och momsreg-nr; den räknas ur kundreskontran och har samma statusar som momsdeklarationen.
_Avoid_: PS, EU-rapport, EU-sammanställning

**Anläggningstillgång**:
En tillgång som är avsedd att användas stadigvarande i företaget och bokförs på ett konto i BAS-klasserna 10–12; den skrivs av över sin nyttjandeperiod om den har en.
_Avoid_: Inventarie (som samlingsnamn), tillgång (utan förled), asset

**Anläggningsregister**:
Företagets register över anläggningstillgångar med anskaffningsvärde, avskrivningsplan, av- och nedskrivningar och avyttringar; allt i det bokförs in i huvudboken.
_Avoid_: Inventarieförteckning, tillgångslista, anläggningsreskontra

**Okopplad anskaffning**:
En bokförd debetrad på ett anskaffningskonto för anläggningstillgångar som ännu inte hör till någon anläggningstillgång i anläggningsregistret.
_Avoid_: Ny tillgång, oregistrerad inventarie

**Nyttjandeperiod**:
Det antal månader som en anläggningstillgång väntas användas och skrivs av linjärt över, räknat från den månad den tas i bruk.
_Avoid_: Livslängd, avskrivningstid, ekonomisk livslängd

**Restvärde**:
Det belopp som en anläggningstillgång väntas vara värd när nyttjandeperioden är slut och som inte skrivs av.
_Avoid_: Slutvärde, skrotvärde, bokfört värde

**Avskrivning**:
Den del av en anläggningstillgångs anskaffningsvärde som förbrukas under en period och bokförs som kostnad enligt avskrivningsplanen; skattemässiga överavskrivningar ingår inte.
_Avoid_: Värdeminskning, nedskrivning, överavskrivning

**Nedskrivning**:
En engångsminskning av en anläggningstillgångs värde utöver avskrivningen, när värdet varaktigt har sjunkit under det bokförda värdet.
_Avoid_: Avskrivning, värdejustering

**Avyttring**:
När en anläggningstillgång säljs eller utrangeras och lämnar anläggningsregistret, med vinst eller förlust mot det bokförda värdet.
_Avoid_: Försäljning (som samlingsnamn), utrangering (som samlingsnamn), borttagning
