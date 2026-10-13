# Underlag lagras i SQLite

Underlag lagras som BLOB i en egen tabell i samma SQLite-databas som bokföringen, tillsammans med sin SHA-256. Ett verifikat och dess underlag bokförs därmed i en och samma transaktion, så ett bokfört verifikat kan aldrig peka på ett underlag som saknas. En säkerhetskopia (`VACUUM INTO`) är en enda konsistent fil. Hashen visar att originalet är oförändrat (krav VF5) och är den checksumma som backupverifiering och migreringar kontrollerar (A4). Priset är att databasen och varje säkerhetskopia växer med underlagen. Därför är ett underlag högst 25 MB, och innehållet läses bara när det behövs.

## Considered Options

- **Filer på disk namngivna efter sin hash, metadata i SQLite**: databasen blir liten, och filerna skrivs en gång och kan säkerhetskopieras inkrementellt. Valdes bort eftersom konsistensen då hänger på att filer skrivs före rader och att databasen säkerhetskopieras före filerna. Backup och återställning blir då två delar som måste stämma överens.
- **Separat `underlag.db` via `ATTACH`**: valdes bort eftersom transaktioner över flera databasfiler inte är atomära i WAL-läge.
