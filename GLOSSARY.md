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

**Inaktivt konto**:
Ett konto som finns i företagets kontoplan men inte kan bokföras på förrän någon aktiverar det; ett utkast får använda det.
_Avoid_: Dolt konto, spärrat konto

**Användare**:
En person som loggar in i Maya och har åtkomst till ett eller flera företag; en användare raderas aldrig, bara avaktiveras.
_Avoid_: Konto, kund, medlem

**Åtkomst**:
En användares rätt att läsa och föra ett visst företags bokföring; den som har åtkomst till ett företag kan ge och ta åtkomst till det för andra.
_Avoid_: Behörighet, roll, medlemskap

**Inbjudan**:
En engångslänk som låter en ny användare registrera sin första passkey och därmed få den åtkomst som inbjudaren gav.
_Avoid_: Registrering, invite

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

**Rättelse**:
Ett verifikat som vänder samtliga rader i ett tidigare bokfört verifikat och är länkat till det; det rättade verifikatet visar att det är rättat.
_Avoid_: Makulering, korrigering, ändring

**Komplettering**:
Ett underlag som kopplas till ett verifikat efter att det har bokförts, utan att verifikatet i övrigt förändras.
_Avoid_: Tillägg, bilaga

**Period**:
En kalendermånad inom ett räkenskapsår; en låst period kan inte få fler verifikat.
_Avoid_: Redovisningsperiod, månad (som term)

**Underlag**:
Den handling som styrker en affärshändelse (kvitto, faktura, avtal) och som arkiveras kopplad till ett eller flera verifikat.
_Avoid_: Bilaga, kvitto (som samlingsnamn), dokument

**Årsarkiv**:
Ett oföränderligt, fristående paket med ett räkenskapsårs bokföring, underlag och behandlingshistorik som skapas när året stängs och går att läsa och skriva ut utan Maya.
_Avoid_: Bokslutsexport, årsbackup, säkerhetskopia

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
