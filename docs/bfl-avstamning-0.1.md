# BFL-avstämning för 0.1

Avstämningen av krav-id:na som [etapp 1](https://github.com/hartwigcarlsson/maya/issues/92) ska uppfylla, mot koden och testerna i release `0.1` ([#127](https://github.com/hartwigcarlsson/maya/issues/127)). Kraven står i [bfl-kontroll.md](bfl-kontroll.md). Sökvägarna är relativa till `crates/`, och `M` är `core/migrations/`.

Status: **ja** = uppfyllt i etappens omfattning, **delvis** = se avvikelserna nedan.

| Krav | Status | Var det uppfylls | Test |
|---|---|---|---|
| L1 | ja | Triggers i M `…140000_verifikat.sql`, återskapade i `…145000_verifikatutkast.sql` och `…181100_rattelse.sql` | `core/tests/verifikat_las.rs::ett_bokfört_verifikat_och_dess_rader_går_inte_att_ändra_eller_radera` |
| L2 | delvis | Utkast = `bokford IS NULL`; `core::verifikat::{spara_utkast, bokför_utkast}` | `core/tests/verifikatutkast.rs::ett_obalanserat_och_ofullständigt_utkast_sparas_och_öppnas_igen` |
| L3 | ja | `core::bokforing::grundbok`, `core::utskrift::verifikat` | `core/tests/grundbok_huvudbok.rs::grundboken_listar_bokförda_verifikat_i_registreringsordning_per_period` |
| L4 | ja | `core::bokforing::{grundbok, huvudbok}` med PDF och CSV | `core/tests/grundbok_huvudbok.rs::huvudboken_ger_ingående_balans_löpande_saldo_och_utgående_balans_per_konto` |
| L6 | ja, bara SEK | `domain::belopp` (öre, ingen valuta) | `domain/tests/belopp.rs::ett_belopp_tolkas_i_kronor_och_lagras_i_öre` |
| L7 | ja | Trigger `verifikat_bokfors`; spärren `differens` i `domain::verifikat` | `core/tests/verifikat_las.rs::schemat_avvisar_att_bokföra_obalanserat_eller_med_en_lucka_i_serien` |
| VF1 | ja | `core::verifikat::{bokför, rätta}`; `kalla IN ('manuell','rattelse')` | `core/tests/rattelse.rs::rättelsen_vänder_varje_rad_och_bokförs_med_nästa_nummer` |
| VF2 | delvis | Datum, bokföringstid, beskrivning, nummer, rader; underlag med filnamn och sha256 | `core/tests/utskrift.rs::verifikatets_underlag_och_kompletteringar_står_i_förteckningen` |
| VF3 | ja | Trigger `verifikat_bokfors` (max+1) och unikt index; samlingsplanen | `core/tests/verifikat.rs::samtidiga_bokföringar_ger_varken_dubbletter_eller_luckor` |
| VF4 | ja | En serie V per räkenskapsår, dokumenterad i `core::systemdokumentation` (samlingsplanen) | `core/tests/systemdokumentation.rs::samlingsplanen_visar_serien_v_per_räkenskapsår_med_perioder` |
| VF5 | ja | M `…171000_underlag.sql` (BLOB, sha256, trigger `underlag_andras_inte`); `core::underlag::ladda_upp` | `core/tests/underlag.rs::ett_uppladdat_underlag_arkiveras_med_sha256_från_mottagandet` |
| VF6 | ja | `underlagskoppling.komplettering`; `core::underlag::komplettera` | `core/tests/underlag.rs::ett_bokfört_verifikat_kompletteras_och_kompletteringen_loggas` |
| VF7 | ja | M `…181100_rattelse.sql` (`rattar`, `kopia_av`) | `core/tests/rattelse.rs::original_och_rättelse_länkar_till_varandra_med_vem_och_när_och_rättelsen_loggas` |
| R1 | ja | `core::verifikat::rätta`, `domain::rattelse` | `core/tests/rattelse.rs::rättelsen_vänder_varje_rad_och_bokförs_med_nästa_nummer` |
| R2 | ja | Som VF7, och händelsen `verifikat rättat` | som VF7; röktestet `maya/tests/rok.rs` (V1 visar "Rättad av V2") |
| T1 | delvis | `domain::att_gora::{frist, kontantfrist, femtiodagarsvarning}`, `domain::arbetsdagar`; Att göra | `domain/tests/att_gora.rs::ett_utkast_på_kassan_har_bara_kontantfristen` |
| H1 | ja | `bokford`, `bokford_av`, `bokford_kanal`; händelsen `verifikat bokfört` | `core/tests/verifikat.rs::bokföringen_loggas_med_registreringstidpunkt_användare_och_kanal` |
| H2 | ja | `core::behandlingshistorik::Händelse` (kontoplan, bokföringsmetod, momsperiod, BAS-tillägg, migrering, programversion) | `core/tests/migrering.rs::programversion_bytt_loggas_när_en_annan_version_startar`; `core/tests/behandlingshistorik.rs::varje_typ_i_etapp_1_som_har_en_kodväg_ger_en_post` |
| H3 | ja | Triggers i M `…130000_foretag.sql` och `…161300_foretagsinstallningar.sql` | `core/tests/behandlingshistorik.rs::behandlingshistoriken_går_inte_att_ändra_eller_radera` |
| S1 | ja | Systemdokumentationens kontoplan, sammanställning, samlingsplan och arkivplan | `core/tests/systemdokumentation.rs::avsnitten_finns_i_ordning_med_krav_som_finns_i_bfl_kontrollen` |
| S2 | ja | Behandlingsreglerna; `domain/data/regler/automatkonteringar.md` | `domain/tests/systemdokumentation.rs::automatkonteringarna_återges_ordagrant_som_i_specen` |
| S3 | ja, etappens flöden | Verifieringskedjan (e-faktura och bank finns inte än) | `core/tests/systemdokumentation.rs::verifieringskedjan_beskriver_flödet_med_hashar_och_kompletteringar` |
| S4 | ja | Avsnittet om utskrift; PDF/A-2b för alla dokument | `core/tests/utskrift.rs` |
| S5 | ja | Avsnittet om överföringar | `core/tests/systemdokumentation.rs::avsnitten_finns_i_ordning_med_krav_som_finns_i_bfl_kontrollen` (bara att avsnittet finns) |
| S6 | delvis | Versionen och ändringsloggen (`domain::systemdokumentation::ändringslogg`), PDF | `domain/tests/systemdokumentation.rs::ändringsloggen_har_den_här_versionen` |
| A1 | delvis | Bokfört och underlag raderas aldrig; `core::foretag::radera` bara utan räkenskapsinformation | `core/tests/foretagsinstallningar.rs::radering_nekas_när_företaget_har_bokförda_verifikat` |
| A2 | ja | PDF och CSV för verifikat, grundbok, huvudbok, rapporter, kontoplan och behandlingshistorik; underlagen i originalformat | `core/tests/grundbok_huvudbok.rs::grundboken_som_pdf_är_pdfa_2b_med_verifikaten_i_registreringsordning`; `web/tests/underlag.rs::pdf_och_bilder_visas_och_andra_format_laddas_ned` |
| A3 | delvis, som specen säger | `core::databas` migrerar med kontrollsummor och backup före | `core/tests/migrering.rs::migreringarna_körs_mot_en_databas_med_data_och_kontrollsummorna_är_desamma_efter` |
| A4 | ja | Migrering, SIE4 (läses tillbaka), backup och restore stäms av med kontrollsummor | `core/tests/migrering.rs::en_migrering_som_ändrar_bokförda_data_rullas_tillbaka_och_maya_startar_inte`; `core/tests/sie4.rs::en_avvikelse_vid_återläsningen_ger_ett_fel_och_ingen_fil` |
| A5 | ja | Nattlig backup med verifiering (`maya/src/backup.rs`, `core::backup`); varning om samma disk | `core/tests/backup.rs::backupen_skriver_databasen_och_ett_manifest_med_kontrollsummorna`; `core/tests/drift.rs::ett_backupmål_på_samma_enhet_som_databasen_ger_en_varning` |
| A6 | ja, förutsatt lagring i Sverige | Systemdokumentationens arkivplan, avsnittet Förvaring i Sverige: Maya förutsätter att databasen och alla backuper förvaras i Sverige och att den som driftar servern ansvarar för det; annan förvaring stöds inte i 0.1. Samma sak i `docs/drift.md` | `core/tests/systemdokumentation.rs::arkivplanen_förutsätter_att_databasen_och_backuperna_förvaras_i_sverige` |
| A7 | ja, genom att Maya inte krypterar | Arkivplanen; `MAYA_ARKIVPLAN_NYCKEL` för ett krypterat backupmål | `core/tests/systemdokumentation.rs::arkivplanen_tar_med_driftens_fritext_ordagrant` |
| Å1 | ja | `domain::rakenskapsar::Räkenskapsår::{första, nästa}` | `domain/tests/rakenskapsar.rs::{det_första_året_är_högst_18_månader, det_första_året_är_minst_en_månad}` |
| Å2 | ja | M `…151400_ingaende_balans.sql`; `core::ingaende_balans` (första året) | `core/tests/ingaende_balans.rs::första_årets_ingående_balans_skrivs_in_och_läses_tillbaka` |
| Å5 | ja | Allt Maya genererar är på svenska (`lang="sv"`, Typst-mallarna, CLI) | `web/tests/router.rs::startsidan_ar_en_svensk_leptos_sida_med_hydrering` |

Röktestet `maya/tests/rok.rs` kör hela flödet i en webbläsare: första användaren, inloggning, företag, underlag, bokföring från Att göra, rättelse och export av balansräkningen och SIE4.

## Avvikelser

Uppskjutna till en senare etapp, med ett öppet issue:

- **VF2**: verifikatet har inget fält för motpart. Motparten finns bara i underlaget. Motpart ur kund- och leverantörsregistret kommer i etapp 2 ([#93](https://github.com/hartwigcarlsson/maya/issues/93)).
- **S6**: systemdokumentationen är versionerad med en ändringslogg per release, men arkiveras inte. Det frysta exemplaret i årsarkivet kommer i etapp 5 ([#96](https://github.com/hartwigcarlsson/maya/issues/96)).
- **A1**: inget bokfört raderas, men det finns ingen sjuårsregel. Ett företag utan bokförda verifikat, underlag och ingående balans kan raderas, med sin kontoplan och behandlingshistorik; schemat nekar radering av ett företag med räkenskapsinformation. Årsarkiven som aldrig rensas kommer i etapp 5 ([#96](https://github.com/hartwigcarlsson/maya/issues/96)).

Inte uppfyllda, med ett öppet issue som avgör hur:

- **L2 och T1**: Att göra varnar för kontantfristen och 50 dagar, men inget låses eller bokförs när fristen har gått ut. Att låsningen sker i tid hänger på användaren. L2 och T1 i [#129](https://github.com/hartwigcarlsson/maya/issues/129).
- **A7**: uppfyllt bara för att Maya inte krypterar något. Nyckeln till ett krypterat backupmål är fritext i arkivplanen, och ingen kontroll görs av att den finns. A7 i [#131](https://github.com/hartwigcarlsson/maya/issues/131).

Testluckor: överföringsavsnittet (S5) testas bara för att det finns, och den nattliga schemaläggningen (A5) testas bara i beräkningen av nästa körning.
