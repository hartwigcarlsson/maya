# Arbetsgivardeklarationen lämnas som XML-fil, inte via API

Maya skapar arbetsgivardeklarationen som en XML-fil enligt Skatteverkets schema 1.1 och validerar den mot XSD:n. Användaren laddar upp filen och signerar den i Skatteverkets e-tjänst och markerar sedan perioden som lämnad i Maya. API:t *Arbetsgivardeklaration inlämning* kräver avtal som programvaruföretag, organisationscertifikat och en OAuth-klient. Det passar inte en self-hostad installation med ett fåtal företag, och signeringen sker ändå hos Skatteverket. Maya fyller själv i summafälten FK487 och FK497 och räknar arbetsgivaravgifterna exakt som Skatteverket: per ålderskategori, med öresavrundning per procentsats och med ungdomsnedsättningen. Samma belopp bokförs i lönekörningens verifikat, så en avvikelse mot Skatteverkets kontroller B_006 och B_007 avslöjar ett fel i Mayas beräkning i stället för att döljas av e-tjänsten.

## Considered Options

- **API-integration**: inlämning direkt från Maya. Valdes bort på grund av avtal och certifikat per installation.
- **Lämna FK487 och FK497 tomma** så att e-tjänsten räknar fram dem: enklare, men då kan det bokförda beloppet på 2731 skilja sig från det deklarerade utan att någon märker det.

## Consequences

- Maya sparar specifikationsnumret (FK570) per anställd och period, så att en ny fil för en lämnad period ersätter de tidigare individuppgifterna i stället för att dubblera dem.
- Avgiftssatser och ålderskategorier ändras årligen och måste uppdateras med Maya.
