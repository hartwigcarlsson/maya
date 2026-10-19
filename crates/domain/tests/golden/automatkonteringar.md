Tabellen är spec:en för bygget och återges ordagrant i Systemdokumentationen. Belopp är i SEK. Saldo räknas med debet som positivt.

Tabellen läggs till i tabellen från etapp 1 och återges ordagrant i Systemdokumentationen. Belopp är i SEK. *Netto* är radernas belopp exklusive moms, *moms* är momsbeloppet per sats. Utgående moms är 2611 (25 %), 2621 (12 %) och 2631 (6 %). Fiktiv utgående moms vid omvänd skattskyldighet är 2614, 2624 och 2634 efter momssats.

| Flöde | Debet | Kredit | Villkor |
|---|---|---|---|
| **Rättelse** av ett bokfört verifikat | för varje rad i originalet: samma konto med originalets kreditbelopp | för varje rad i originalet: samma konto med originalets debetbelopp | originalet är bokfört, inte rättat, inte en rättelse och inte från ett av Mayas flöden; radtexten följer med; rättelsen länkas till originalet; datum ≥ originalets datum och i en period som inte är låst |
| **Registrera om** (inget bokförs) | utkast med originalets rader och underlag, märkt *kopia av* | | bara tillsammans med en rättelse; utkastet följer de vanliga spärrarna |
| **Preliminär ingående balans** för år *n* (inget verifikat) | IB(*k*, *n*) = UB(*k*, *n*−1) för varje konto *k* i klass 1–2 | | år *n*−1 finns i Maya och är inte stängt; därtill IB(2099, *n*) = UB(2099, *n*−1) + Σ UB(*k*, *n*−1) över klass 3–8, alltså det resultat som inte är ombokat; räknas vid läsning och visas som preliminär |
| **Utfärda** eller **registrera** kundfaktura | 1510 totalen | intäktskontot per rad netto; 2611/2621/2631 moms | fakturametoden; datum = fakturadatum; omvänd skattskyldighet och 0 % ger ingen momsrad |
| **Betalning** av kundfaktura, fordran bokförd | betalkontot | 1510 | |
| **Betalning** av kundfaktura, fordran inte bokförd | betalkontot | intäkt och moms proportionellt | kontantmetoden |
| **Kreditfaktura** till kund, fordran bokförd | intäktskontot per rad netto; utgående moms per sats | 1510 totalen | speglar originalet, proportionellt vid delkredit |
| **Kreditfaktura** till kund, fordran inte bokförd | intäkt och moms för den del som överstiger det obetalda | 1510 samma belopp | kontantmetoden; kredit av en obetald del bokförs inte |
| **Återbetalning** av kundens tillgodohavande | 1510 | betalkontot | |
| **Kundförlust**, fordran bokförd | 6351 netto; utgående moms per sats proportionellt | 1510 återstående belopp | konstaterad förlust med skäl; fordran inte bokförd ger bara status |
| **Återkalla kundförlust** | förlustverifikatets kreditrader | förlustverifikatets debetrader | länkat till förlusten |
| **Registrera** leverantörsfaktura | kostnadskontot per rad netto; 2641 moms | 2440 totalen | fakturametoden; datum = fakturadatum; företag utan momsregistrering bokför momsen på kostnadskontot |
| **Registrera** leverantörsfaktura med omvänd skattskyldighet | kostnadskontot per rad netto; 2645 (EU-tjänst, EU-vara, utanför EU) eller 2647 (bygg) fiktiv moms | 2440 totalen; 2614/2624/2634 fiktiv moms | fakturametoden; justeringsparet mot 4599 läggs till i etapp 3 |
| **Betalning** av leverantörsfaktura, skuld bokförd | 2440 | betalkontot | |
| **Betalning** av leverantörsfaktura, skuld inte bokförd | kostnad och ingående moms (och fiktiv moms vid omvänd skattskyldighet) proportionellt | betalkontot (och fiktiv utgående moms) | kontantmetoden |
| **Kreditfaktura** från leverantör, skuld bokförd | 2440 totalen | registreringens debetrader, proportionellt vid delkredit | speglar registreringsverifikatet, också fiktiv moms vid omvänd skattskyldighet |
| **Kreditfaktura** från leverantör, skuld inte bokförd | 2440 för den del som överstiger det obetalda | kostnad och moms samma belopp | kontantmetoden; kredit av en obetald del bokförs inte |
| **Återbetalning** av tillgodohavande hos leverantör | betalkontot | 2440 | |
| **Öresutjämning** vid betalning av kundfaktura, rest ≤ 10 kr | 3740 resten | 1510 resten | fordran bokförd; i samma verifikat som betalningen; när fordran inte är bokförd stängs resten utan verifikat |
| **Öresutjämning** vid betalning av leverantörsfaktura, rest ≤ 10 kr | 2440 resten | 3740 resten | skuld bokförd; i samma verifikat som betalningen; när skulden inte är bokförd stängs resten utan verifikat |
| **Kursdifferens** vid betalning av leverantörsfaktura | 7960 vid förlust | 3960 vid vinst | fakturan har originalvaluta; i samma verifikat som betalningen |
| **Rätta kontering**, **Rätta avskrift**, **Rätta registrering**, **Rätta betalning** | rättelse enligt etapp 1 av flödets verifikat | | Rätta kontering bokför därtill ett nytt utfärdandeverifikat med dagens datum; de andra låser upp fakturan eller tar bort betalningen |
| **T2-samlingsverifikat** | 1510 för varje obetald kundfaktura; kostnad och ingående moms (och fiktiv moms) proportionellt för varje obetald leverantörsfaktura | intäkt och utgående moms proportionellt; 2440 för varje obetald leverantörsfaktura | kontantmetoden; datum = årets sista dag; utkast som Maya skapar; vänds inte |

Senare etapper lägger till sina rader (moms, lön, avskrivningar, stängning) i samma tabell.
