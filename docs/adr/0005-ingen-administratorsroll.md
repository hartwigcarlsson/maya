# Ingen administratörsroll; återställning och avaktivering bara via server-CLI

Maya har inga roller. Den som har åtkomst till ett företag kan bjuda in användare till det, ge åtkomst till det och ta bort åtkomst till det, men inte företagets sista åtkomst. Allt som rör en användare som helhet görs bara med CLI:t på servern: att skapa den första användaren, att återställa en användare som har tappat alla sina passkeys, att avaktivera, att återaktivera och att byta domän (`rp_id`), vilket tömmer alla passkeys. Den som kan ge en annan användare en återställningslänk kan ta över hens konto, och då kan man inte lita på aktören i behandlingshistoriken (H1). Därför ligger den möjligheten bara hos den som driver servern. Det räcker för en self-hostad installation med få betrodda användare.

## Considered Options

- **En instansflagga *administratör*** som bjuder in och utfärdar återställningslänkar i webben: bekvämare, men det blir en roll att förvalta, och en kapad administratör kan ta över alla konton.
- **Alla användare kan återställa varandra**: valdes bort, eftersom vem som helst då kan ta över ett konto.

## Consequences

- En användare som tappar alla sina passkeys är utelåst tills den som driver servern kör CLI:t. Maya uppmanar därför varje användare att registrera mer än en passkey.
- Ingen kan i webben avaktivera en användare, eftersom det skulle ta bort åtkomst till företag som den som avaktiverar inte har åtkomst till.
