# Maya

Maya är ett self-hostat bokföringssystem för svenska aktiebolag som följer Bokföringslagen (BFL) och är byggt för att låta AI automatisera bokföringen.

## Language

**Användare**:
En person som loggar in i Maya och har åtkomst till ett eller flera företag.
_Avoid_: Konto, kund, medlem

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
Att godkänna ett utkast eller registrera ett verifikat så att det låses och får sitt verifikationsnummer; först då räknas det som bokfört.
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
