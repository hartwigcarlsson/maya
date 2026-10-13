# Periodlåsning är slutgiltig

En period (kalendermånad) låses i ordning och kan aldrig låsas upp. Bokförda verifikat är redan oföränderliga, så en upplåsning skulle bara öppna för att i efterhand lägga in verifikat i en stängd månad, till exempel en som redan är momsdeklarerad. BFL och BFNAR säger att rättelser efter stängning bokförs i den öppna period där felet upptäcks (krav R3 i `docs/bfl-kontroll.md`), så Maya behöver ingen upplåsning. Priset är att en för tidig låsning bara kan rättas framåt. Därför bekräftar Maya med en tydlig dialog innan en period låses.

## Considered Options

- **Upplåsning med logg**: den senast låsta perioden kan låsas upp och händelsen loggas i behandlingshistoriken. Valdes bort eftersom en momsdeklarerad eller rapporterad månad då kan ändras i tysthet, och varje rapport som bygger på låsta perioder måste klara att perioden öppnas igen.
