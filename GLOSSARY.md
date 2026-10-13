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

**Underlag**:
Den handling som styrker en affärshändelse (kvitto, faktura, avtal) och som arkiveras kopplad till sitt verifikat.
_Avoid_: Bilaga, kvitto (som samlingsnamn), dokument

**Bokföringsmetod**:
Företagets val mellan fakturametoden och kontantmetoden, som styr när fakturor bokförs.

**Kundfaktura**:
En utgående faktura till en kund; den kan skapas i Maya eller i ett annat system, men bokförs alltid i Maya.
_Avoid_: Utgående faktura, försäljningsfaktura

**Leverantörsfaktura**:
En inkommande faktura från en leverantör som registreras och bokförs i Maya.
_Avoid_: Inkommande faktura, inköpsfaktura
