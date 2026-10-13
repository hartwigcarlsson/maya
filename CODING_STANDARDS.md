# Coding standards

## Bokföringslagen

Check every change against [`docs/bfl-kontroll.md`](docs/bfl-kontroll.md):

- Each requirement the diff touches is met by the diff, or an open issue cites its id.
- The commit's `BFL:` line names exactly the requirements the diff touches. `BFL: ej berörd` on a diff that touches bokförda poster, verifikationsnummer, underlag, behandlingshistorik, schema or migrations, SIE import/export, backups, or any delete path is a finding.

## Glossary

- A term added to `GLOSSARY.md` whose word already appears with another meaning, in another definition or in a decision on the wayfinder map, is a finding: rename the new term or reword the old use.
