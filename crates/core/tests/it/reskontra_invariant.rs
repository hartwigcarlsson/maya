//! Invariant-testet för reskontrorna (#151, R4): slumpade följder av fakturahändelser genom
//! `core` mot en tillfällig databas, under båda bokföringsmetoderna och med metodbyte. Efter
//! varje steg ska avstämningen mot 1510 och 2440 ge noll, varje verifikat balansera, och det som
//! återstår av varje faktura vara detsamma i reskontran per datum, i listorna och i fakturans
//! reskontrapost.
//!
//! Händelserna är de som fakturorna har: utfärda och registrera (kundfaktura, extern
//! kundfaktura, leverantörsfaktura med och utan omvänd skattskyldighet och originalvaluta),
//! betala helt och delvis, öresutjämning, kursdifferens, återbetalning, kreditera (hel och
//! delvis, med och utan hänvisning), kundförlust och återkallelse, Rätta kontering, Rätta
//! registrering, Rätta avskrift och Rätta betalning med omregistrering, metodbyte, T2-utkastet och
//! dess bokföring, och betalningar efter årets slut. En händelse som en spärr stoppar hoppas
//! över; ett databasfel är ett fel i testet.
//!
//! Ett misslyckat fall skriver ut fröet och stegen, så att det går att köra om.

use std::fmt::Debug;

use jiff::ToSpan;
use jiff::civil::Date;
use maya_core::aktor::Aktör;
use maya_core::betalning::{self, BetalningsId, Faktura};
use maya_core::foretag::{self, FöretagsId};
use maya_core::kund::KundId;
use maya_core::kundfaktura::{self, KundfakturaId};
use maya_core::kundforlust::{self, KundförlustId};
use maya_core::leverantor::LeverantörsId;
use maya_core::leverantorsfaktura::{self, LeverantörsfakturaId};
use maya_core::reskontra;
use maya_core::verifikat::{self, Urval};
use maya_core::{
    Databas, Klocka, TillfälligDatabas, extern_kundfaktura, kontering, t2, testhjalp
};
use maya_domain::betalning::NyBetalning;
use maya_domain::extern_kundfaktura::{NyExternFaktura, NyExternRad};
use maya_domain::kontering::NyKontering;
use maya_domain::kreditfaktura::NyKreditering;
use maya_domain::kundfaktura::{NyFakturarad, NyKundfaktura};
use maya_domain::kundforlust::NyKundförlust;
use maya_domain::leverantorsfaktura::{NyLeverantörsfaktura, NyRad};

/// Dagen då allt händer: efter räkenskapsåret 2026, så att T2 kan skapas och betalningar kan
/// komma efter årets slut.
const NU: &str = "2027-03-01T10:00:00Z";

fn idag() -> Date {
    "2027-03-01".parse().unwrap()
}

/// Antalet följder per bokföringsmetod och antalet steg i varje. `MAYA_INVARIANT_FOLJDER`
/// kör fler följder, t.ex. för att leta efter fel en längre stund.
const FÖLJDER: u64 = 12;
const STEG: usize = 45;

fn följder() -> u64 {
    std::env::var("MAYA_INVARIANT_FOLJDER")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(FÖLJDER)
}

/// En enkel xorshift-generator, så att följderna går att upprepa utan ett beroende.
struct Slump(u64);

impl Slump {
    fn nästa(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    /// Ett tal i `0..n`.
    fn under(&mut self, n: u64) -> u64 {
        if n == 0 { 0 } else { self.nästa() % n }
    }

    fn mellan(&mut self, a: i64, b: i64) -> i64 {
        a + i64::try_from(self.under(u64::try_from(b - a + 1).unwrap())).unwrap()
    }

    fn singla(&mut self) -> bool {
        self.under(2) == 0
    }

    fn välj<T: Copy>(&mut self, v: &[T]) -> Option<T> {
        if v.is_empty() {
            None
        } else {
            Some(v[usize::try_from(self.under(v.len() as u64)).unwrap()])
        }
    }

    /// En dag från `från` till och med i dag.
    fn dag_från(&mut self, från: Date) -> Date {
        let dagar = (idag() - från).get_days().max(0);
        från + self.mellan(0, i64::from(dagar)).days()
    }
}

/// Öre som kronor i ett formulär: `-1234,56`.
fn kr(öre: i64) -> String {
    let tecken = if öre < 0 { "-" } else { "" };
    let öre = öre.abs();
    format!("{tecken}{},{:02}", öre / 100, öre % 100)
}

/// Fakturans egen sort, för valen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sort {
    Egen(KundfakturaId),
    Extern(KundfakturaId),
    Leverantör(LeverantörsfakturaId),
}

impl Sort {
    fn faktura(self) -> Faktura {
        match self {
            Sort::Egen(id) | Sort::Extern(id) => Faktura::Kund(id),
            Sort::Leverantör(id) => Faktura::Leverantör(id),
        }
    }
}

struct Värld {
    db: TillfälligDatabas,
    klocka: Klocka,
    anna: Aktör,
    id: FöretagsId,
    kund: KundId,
    leverantör: LeverantörsId,
    slump: Slump,
    fakturor: Vec<Sort>,
    /// Med originalvaluta, så att en kursdifferens kan stängas.
    valuta: Vec<LeverantörsfakturaId>,
    betalningar: Vec<BetalningsId>,
    förluster: Vec<KundförlustId>,
    upplåsta: Vec<Sort>,
    löpnummer: u32,
    steg: Vec<String>,
}

/// Ett fel som en spärr eller ett läge ger hoppar över steget; ett databasfel är ett fel.
fn utfall<T, E: Debug>(steg: &mut Vec<String>, vad: String, r: Result<T, E>) -> Option<T> {
    match r {
        Ok(t) => {
            steg.push(vad);
            Some(t)
        }
        Err(e) => {
            let text = format!("{e:?}");
            assert!(
                !text.contains("Databas(") && !text.contains("Korrupt"),
                "{vad} gav ett databasfel: {text}\nsteg:\n{}",
                steg.join("\n")
            );
            steg.push(format!("({vad}: {text})"));
            None
        }
    }
}

impl Värld {
    async fn ny(metod: &str, frö: u64) -> Värld {
        let db = Databas::tillfällig().await;
        let (anna, id) = testhjalp::fakturaföretag_med_metod(&db, metod).await;
        let kund = testhjalp::kund(&db, &anna, id, "Kund AB").await;
        let leverantör = testhjalp::leverantör(&db, &anna, id, "Kontorsvaror AB").await;
        Värld {
            db,
            klocka: Klocka::fast(NU.parse().unwrap()),
            anna,
            id,
            kund,
            leverantör,
            slump: Slump(0x9E37_79B9_7F4A_7C15 ^ frö.wrapping_mul(0x2545_F491_4F6C_DD1D)),
            fakturor: Vec::new(),
            valuta: Vec::new(),
            betalningar: Vec::new(),
            förluster: Vec::new(),
            upplåsta: Vec::new(),
            löpnummer: 0,
            steg: vec![format!("metod {metod}, frö {frö}")],
        }
    }

    fn nytt_nummer(&mut self, prefix: &str) -> String {
        self.löpnummer += 1;
        format!("{prefix}-{}", self.löpnummer)
    }

    fn fakturadatum(&mut self) -> Date {
        let från: Date = "2026-01-05".parse().unwrap();
        från + self.slump.mellan(0, 410).days()
    }

    /// Det som återstår av fakturan i dag.
    async fn återstår(&self, s: Sort) -> Option<(i64, Date)> {
        let post = betalning::reskontrapost(&self.db, &self.anna, self.id, s.faktura())
            .await
            .ok()?;
        let datum = self.datum(s).await?;
        Some((post.återstår, datum))
    }

    async fn datum(&self, s: Sort) -> Option<Date> {
        match s {
            Sort::Egen(id) | Sort::Extern(id) => {
                kundfaktura::lista(&self.db, &self.anna, self.id)
                    .await
                    .unwrap()
                    .into_iter()
                    .find(|r| r.id == id)?
                    .fakturadatum
            }
            Sort::Leverantör(id) => {
                leverantorsfaktura::lista(&self.db, &self.anna, self.id)
                    .await
                    .unwrap()
                    .into_iter()
                    .find(|r| r.id == id)?
                    .fakturadatum
            }
        }
    }

    async fn steg(&mut self) {
        match self.slump.under(22) {
            0..=2 => self.utfärda().await,
            3 => self.registrera_extern().await,
            4..=5 => self.registrera_leverantörsfaktura().await,
            6..=8 => self.betala(false).await,
            9 => self.betala(true).await,
            10 => self.återbetala().await,
            11 => self.kreditera().await,
            12 => self.kundförlust().await,
            13 => self.återkalla().await,
            14 => self.rätta_kontering().await,
            15 => self.lås_upp().await,
            16 => self.registrera_om().await,
            17 => self.rätta_betalning().await,
            18 => self.byt_metod().await,
            19 => self.t2().await,
            20 => self.kursdifferens().await,
            _ => self.leverantörskredit().await,
        }
    }

    async fn utfärda(&mut self) {
        let d = self.fakturadatum();
        let mut rader = vec![NyFakturarad {
            beskrivning: "Konsulttimmar".into(),
            antal: self.slump.mellan(1, 5).to_string(),
            enhet: "tim".into(),
            à_pris: self.slump.mellan(100, 2000).to_string(),
            ..Default::default()
        }];
        if self.slump.singla() {
            rader.push(NyFakturarad {
                beskrivning: "Böcker".into(),
                antal: "1".into(),
                enhet: "st".into(),
                à_pris: format!(
                    "{},{:02}",
                    self.slump.mellan(10, 900),
                    self.slump.mellan(0, 99)
                ),
                momssats: "6".into(),
                konto: "3003".into(),
                ..Default::default()
            });
        }
        let formulär = NyKundfaktura {
            kund: self.kund.to_string(),
            fakturadatum: d.to_string(),
            rader,
            ..Default::default()
        };
        let Värld {
            db,
            klocka,
            anna,
            id,
            ..
        } = self;
        let utkast = kundfaktura::spara_utkast(db, klocka, anna, *id, None, &formulär)
            .await
            .unwrap();
        let r = kundfaktura::utfärda(db, klocka, anna, *id, utkast).await;
        if let Some(u) = utfall(&mut self.steg, format!("utfärda {d}"), r) {
            self.fakturor.push(Sort::Egen(u.id));
        }
    }

    fn externt_formulär(&mut self, nummer: &str, d: Date, netto: i64) -> NyExternFaktura {
        NyExternFaktura {
            kund: self.kund.to_string(),
            fakturanummer: nummer.into(),
            fakturadatum: d.to_string(),
            förfallodag: String::new(),
            rader: vec![NyExternRad {
                momssats: "25".into(),
                konto: "3001".into(),
                belopp: kr(netto),
            }],
            ..Default::default()
        }
    }

    async fn registrera_extern(&mut self) {
        let d = self.fakturadatum();
        let nummer = self.nytt_nummer("E");
        let netto = self.slump.mellan(10_000, 500_000);
        let formulär = self.externt_formulär(&nummer, d, netto);
        let Värld {
            db,
            klocka,
            anna,
            id,
            ..
        } = self;
        let fil = format!("%PDF-1.4 {nummer}");
        let (f, _) = extern_kundfaktura::ladda_upp(
            db,
            klocka,
            anna,
            *id,
            (&format!("{nummer}.pdf"), "application/pdf", fil.as_bytes()),
        )
        .await
        .unwrap();
        let r = extern_kundfaktura::registrera(db, klocka, anna, *id, f, &formulär).await;
        if utfall(&mut self.steg, format!("registrera extern {nummer} {d}"), r).is_some() {
            self.fakturor.push(Sort::Extern(f));
        }
    }

    async fn ladda_upp_leverantörsfaktura(&mut self, nummer: &str) -> LeverantörsfakturaId {
        let Värld {
            db,
            klocka,
            anna,
            id,
            ..
        } = self;
        let fil = format!("%PDF-1.4 leverantörsfaktura {nummer}");
        leverantorsfaktura::ladda_upp(
            db,
            klocka,
            anna,
            *id,
            (&format!("{nummer}.pdf"), "application/pdf", fil.as_bytes()),
        )
        .await
        .unwrap()
        .0
    }

    async fn registrera_leverantörsfaktura(&mut self) {
        let d = self.fakturadatum();
        let nummer = self.nytt_nummer("L");
        // Netto i hela kronor delbart med fyra, så att 25 % moms blir hela ören.
        let netto = self.slump.mellan(25, 5_000) * 400;
        let omvänd = self.slump.under(4) == 0;
        let valuta = self.slump.under(3) == 0;
        let formulär = NyLeverantörsfaktura {
            leverantör: self.leverantör.to_string(),
            fakturanummer: nummer.clone(),
            fakturadatum: d.to_string(),
            förfallodag: (d + 30.days()).to_string(),
            referens: "4455667788".into(),
            totalbelopp: kr(if omvänd { netto } else { netto / 4 * 5 }),
            omvänd_skattskyldighet: if omvänd {
                "eu_tjanst".into()
            } else {
                String::new()
            },
            originalvaluta: if valuta {
                "EUR 100,00".into()
            } else {
                String::new()
            },
            rader: vec![NyRad {
                konto: "5410".into(),
                netto: kr(netto),
                momssats: "25".into(),
                moms: String::new(),
            }],
            ..Default::default()
        };
        let f = self.ladda_upp_leverantörsfaktura(&nummer).await;
        let Värld {
            db,
            klocka,
            anna,
            id,
            ..
        } = self;
        let r = leverantorsfaktura::registrera(db, klocka, anna, *id, f, &formulär).await;
        if utfall(&mut self.steg, format!("registrera {nummer} {d}"), r).is_some() {
            self.fakturor.push(Sort::Leverantör(f));
            if valuta {
                self.valuta.push(f);
            }
        }
    }

    /// En faktura i reskontran med något kvar att betala.
    async fn med_kvar(&mut self, tillgodo: bool) -> Option<(Sort, i64, Date)> {
        let mut kandidater = Vec::new();
        for &s in &self.fakturor {
            if self.upplåsta.contains(&s) {
                continue;
            }
            if let Some((å, d)) = self.återstår(s).await
                && (if tillgodo { å < 0 } else { å > 0 })
            {
                kandidater.push((s, å, d));
            }
        }
        self.slump.välj(&kandidater)
    }

    async fn registrera_betalning(&mut self, s: Sort, b: NyBetalning, vad: String) {
        let Värld {
            db,
            klocka,
            anna,
            id,
            ..
        } = self;
        let r = betalning::registrera(db, klocka, anna, *id, s.faktura(), &b).await;
        if let Some(r) = utfall(&mut self.steg, vad, r) {
            self.betalningar.push(r.id);
        }
    }

    async fn betala(&mut self, öresutjämning: bool) {
        let Some((s, å, d)) = self.med_kvar(false).await else {
            return;
        };
        let betaldag = self.slump.dag_från(d);
        let (belopp, rest) = if öresutjämning {
            let rest = self.slump.mellan(1, 1000).min(å - 1);
            (å - rest, rest)
        } else if self.slump.singla() {
            (å, 0)
        } else {
            (self.slump.mellan(1, å), 0)
        };
        if belopp <= 0 {
            return;
        }
        let b = NyBetalning {
            betaldag: betaldag.to_string(),
            belopp: kr(belopp),
            konto: "1930".into(),
            öresutjämning: öresutjämning && rest > 0,
            ..Default::default()
        };
        self.registrera_betalning(s, b, {
            let årsslut: Date = "2026-12-31".parse().unwrap();
            let vad = if rest > 0 {
                "öresutjämning"
            } else if d <= årsslut && betaldag > årsslut {
                "efter årets slut"
            } else {
                "betala"
            };
            format!("{vad} {s:?} {} {betaldag} rest {rest}", kr(belopp))
        })
        .await;
    }

    async fn kursdifferens(&mut self) {
        let mut kandidater = Vec::new();
        for &f in &self.valuta {
            let s = Sort::Leverantör(f);
            if !self.upplåsta.contains(&s)
                && let Some((å, d)) = self.återstår(s).await
                && å > 10_000
            {
                kandidater.push((s, å, d));
            }
        }
        let Some((s, å, d)) = self.slump.välj(&kandidater) else {
            return;
        };
        let avvikelse = self.slump.mellan(-5_000, 5_000);
        let belopp = å + avvikelse;
        let betaldag = self.slump.dag_från(d);
        let b = NyBetalning {
            betaldag: betaldag.to_string(),
            belopp: kr(belopp),
            konto: "1930".into(),
            kursdifferens: true,
            ..Default::default()
        };
        self.registrera_betalning(
            s,
            b,
            format!("kursdifferens {s:?} {} {betaldag}", kr(belopp)),
        )
        .await;
    }

    async fn återbetala(&mut self) {
        let Some((s, å, d)) = self.med_kvar(true).await else {
            return;
        };
        let betaldag = self.slump.dag_från(d);
        let b = NyBetalning {
            betaldag: betaldag.to_string(),
            belopp: kr(-å),
            konto: "1930".into(),
            ..Default::default()
        };
        self.registrera_betalning(s, b, format!("återbetala {s:?} {} {betaldag}", kr(-å)))
            .await;
    }

    async fn kreditera(&mut self) {
        let egna: Vec<_> = self
            .fakturor
            .iter()
            .copied()
            .filter(|s| matches!(s, Sort::Egen(_) | Sort::Extern(_)) && !self.upplåsta.contains(s))
            .collect();
        let Some(s) = self.slump.välj(&egna) else {
            return;
        };
        let Some((_, d)) = self.återstår(s).await else {
            return;
        };
        let kreditdag = self.slump.dag_från(d);
        match s {
            Sort::Egen(original) => {
                let hela = self.slump.singla();
                let kreditering = NyKreditering {
                    hela,
                    antal: if hela { Vec::new() } else { vec!["1".into()] },
                };
                let Värld {
                    db,
                    klocka,
                    anna,
                    id,
                    ..
                } = self;
                let r = kundfaktura::kreditera(db, klocka, anna, *id, original, &kreditering).await;
                let Some(utkast) = utfall(&mut self.steg, format!("kreditutkast {s:?}"), r) else {
                    return;
                };
                let Värld {
                    db,
                    klocka,
                    anna,
                    id,
                    ..
                } = self;
                let mut formulär = kundfaktura::kundfakturan(db, anna, *id, utkast)
                    .await
                    .unwrap()
                    .formulär;
                formulär.fakturadatum = kreditdag.to_string();
                kundfaktura::spara_utkast(db, klocka, anna, *id, Some(utkast), &formulär)
                    .await
                    .unwrap();
                let r = kundfaktura::utfärda(db, klocka, anna, *id, utkast).await;
                utfall(
                    &mut self.steg,
                    format!("kreditera {s:?} hela {hela} {kreditdag}"),
                    r,
                );
            }
            Sort::Extern(original) => {
                let nummer = self.nytt_nummer("EK");
                let netto = -self.slump.mellan(1_000, 200_000);
                let mut formulär = self.externt_formulär(&nummer, kreditdag, netto);
                formulär.kund = String::new();
                let Värld {
                    db,
                    klocka,
                    anna,
                    id,
                    ..
                } = self;
                let fil = format!("%PDF-1.4 {nummer}");
                let r = extern_kundfaktura::kreditera(
                    db,
                    klocka,
                    anna,
                    *id,
                    original,
                    (&format!("{nummer}.pdf"), "application/pdf", fil.as_bytes()),
                )
                .await;
                let Some(k) = utfall(&mut self.steg, format!("externt kreditutkast {s:?}"), r)
                else {
                    return;
                };
                let Värld {
                    db,
                    klocka,
                    anna,
                    id,
                    ..
                } = self;
                let r = extern_kundfaktura::registrera(db, klocka, anna, *id, k, &formulär).await;
                utfall(
                    &mut self.steg,
                    format!("kreditera {s:?} {} {kreditdag}", kr(netto)),
                    r,
                );
            }
            Sort::Leverantör(_) => {}
        }
    }

    async fn leverantörskredit(&mut self) {
        let originalen: Vec<_> = self
            .fakturor
            .iter()
            .filter_map(|s| match s {
                Sort::Leverantör(f) if !self.upplåsta.contains(s) => Some(*f),
                _ => None,
            })
            .collect();
        let original = if self.slump.under(4) == 0 {
            None
        } else {
            match self.slump.välj(&originalen) {
                Some(f) => Some(f),
                None => return,
            }
        };
        let från = match original {
            Some(f) => match self.datum(Sort::Leverantör(f)).await {
                Some(d) => d,
                None => return,
            },
            None => "2026-01-05".parse().unwrap(),
        };
        let d = self.slump.dag_från(från);
        let nummer = self.nytt_nummer("LK");
        let netto = self.slump.mellan(1, 500) * 400;
        let formulär = NyLeverantörsfaktura {
            leverantör: self.leverantör.to_string(),
            fakturanummer: nummer.clone(),
            fakturadatum: d.to_string(),
            förfallodag: (d + 30.days()).to_string(),
            totalbelopp: kr(-netto / 4 * 5),
            rader: vec![NyRad {
                konto: "5410".into(),
                netto: kr(-netto),
                momssats: "25".into(),
                moms: String::new(),
            }],
            krediterar: original.map(|f| f.to_string()).unwrap_or_default(),
            ..Default::default()
        };
        let f = self.ladda_upp_leverantörsfaktura(&nummer).await;
        let Värld {
            db,
            klocka,
            anna,
            id,
            ..
        } = self;
        let r = leverantorsfaktura::registrera(db, klocka, anna, *id, f, &formulär).await;
        if utfall(
            &mut self.steg,
            format!(
                "leverantörskredit {} {nummer} {original:?} {d}",
                if original.is_some() {
                    "med hänvisning"
                } else {
                    "utan hänvisning"
                }
            ),
            r,
        )
        .is_some()
            && original.is_none()
        {
            // En kreditfaktura utan hänvisning är en egen post med ett tillgodohavande.
            self.fakturor.push(Sort::Leverantör(f));
        }
    }

    async fn kundförlust(&mut self) {
        let Some((s, _, d)) = self.med_kvar(false).await else {
            return;
        };
        let Faktura::Kund(f) = s.faktura() else {
            return;
        };
        let datum = self.slump.dag_från(d);
        let formulär = NyKundförlust {
            skäl: "konkurs".into(),
            annat: String::new(),
            datum: datum.to_string(),
        };
        let Värld {
            db,
            klocka,
            anna,
            id,
            ..
        } = self;
        let r = kundforlust::bokför(db, klocka, anna, *id, f, &formulär).await;
        if let Some(b) = utfall(&mut self.steg, format!("kundförlust {s:?} {datum}"), r) {
            self.förluster.push(b.id);
        }
    }

    async fn återkalla(&mut self) {
        let Some(förlust) = self.slump.välj(&self.förluster.clone()) else {
            return;
        };
        let Värld {
            db,
            klocka,
            anna,
            id,
            ..
        } = self;
        let r = kundforlust::återkalla(db, klocka, anna, *id, förlust).await;
        if utfall(&mut self.steg, format!("återkalla {förlust:?}"), r).is_some() {
            self.förluster.retain(|f| *f != förlust);
        }
    }

    async fn rätta_kontering(&mut self) {
        // Bara en faktura med ett utfärdandeverifikat har en kontering att rätta.
        let mut kandidater = Vec::new();
        for s in &self.fakturor {
            if let Sort::Egen(f) = *s
                && let Ok(k) = kontering::kontering(&self.db, &self.anna, self.id, f).await
            {
                kandidater.push((f, k));
            }
        }
        let i = usize::try_from(self.slump.under(kandidater.len() as u64)).unwrap();
        if kandidater.is_empty() {
            return;
        }
        let (f, nu) = kandidater.swap_remove(i);
        let Värld {
            db,
            klocka,
            anna,
            id,
            ..
        } = self;
        let formulär = NyKontering {
            konton: nu
                .rader
                .iter()
                .map(|r| {
                    if r.konto.get() == 3001 {
                        "3002"
                    } else {
                        "3001"
                    }
                    .into()
                })
                .collect(),
            orsak: "fel konto".into(),
        };
        let r = kontering::rätta(db, klocka, anna, *id, f, &formulär).await;
        utfall(&mut self.steg, format!("rätta kontering {f:?}"), r);
    }

    async fn lås_upp(&mut self) {
        let kandidater: Vec<_> = self
            .fakturor
            .iter()
            .copied()
            .filter(|s| {
                matches!(s, Sort::Extern(_) | Sort::Leverantör(_)) && !self.upplåsta.contains(s)
            })
            .collect();
        let Some(s) = self.slump.välj(&kandidater) else {
            return;
        };
        let Värld {
            db,
            klocka,
            anna,
            id,
            ..
        } = self;
        let låst = match s {
            Sort::Extern(f) => {
                let r = extern_kundfaktura::rätta_registrering(db, klocka, anna, *id, f, "").await;
                utfall(&mut self.steg, format!("rätta registrering {s:?}"), r).is_some()
            }
            Sort::Leverantör(f) => {
                let r = leverantorsfaktura::rätta_avskrift(db, klocka, anna, *id, f, "").await;
                utfall(&mut self.steg, format!("rätta avskrift {s:?}"), r).is_some()
            }
            Sort::Egen(_) => false,
        };
        if låst {
            self.upplåsta.push(s);
        }
    }

    /// Registrerar om en upplåst faktura, med samma belopp eller ett större.
    async fn registrera_om(&mut self) {
        let Some(s) = self.slump.välj(&self.upplåsta.clone()) else {
            return;
        };
        let öka = self.slump.singla();
        let Värld {
            db,
            klocka,
            anna,
            id,
            ..
        } = self;
        let registrerad = match s {
            Sort::Extern(f) => {
                let mut formulär = extern_kundfaktura::fakturan(db, anna, *id, f)
                    .await
                    .unwrap()
                    .unwrap()
                    .formulär;
                if öka {
                    formulär.rader[0].belopp.push('0');
                }
                let r = extern_kundfaktura::registrera(db, klocka, anna, *id, f, &formulär).await;
                utfall(&mut self.steg, format!("registrera om {s:?} öka {öka}"), r).is_some()
            }
            Sort::Leverantör(f) => {
                let mut formulär = leverantorsfaktura::fakturan(db, anna, *id, f)
                    .await
                    .unwrap()
                    .unwrap()
                    .formulär;
                if öka {
                    formulär.totalbelopp = format!("{}0", formulär.totalbelopp.replace(' ', ""));
                    formulär.rader[0].netto =
                        format!("{}0", formulär.rader[0].netto.replace(' ', ""));
                    formulär.rader[0].moms = String::new();
                }
                let r = leverantorsfaktura::registrera(db, klocka, anna, *id, f, &formulär).await;
                utfall(&mut self.steg, format!("registrera om {s:?} öka {öka}"), r).is_some()
            }
            Sort::Egen(_) => false,
        };
        if registrerad {
            self.upplåsta.retain(|u| *u != s);
        }
    }

    async fn rätta_betalning(&mut self) {
        let Some(b) = self.slump.välj(&self.betalningar.clone()) else {
            return;
        };
        let Värld {
            db,
            klocka,
            anna,
            id,
            ..
        } = self;
        let r = betalning::rätta(db, klocka, anna, *id, b, &idag().to_string(), "fel belopp").await;
        if utfall(&mut self.steg, format!("rätta betalning {b:?}"), r).is_some() {
            self.betalningar.retain(|x| *x != b);
        }
    }

    async fn byt_metod(&mut self) {
        let Värld {
            db,
            klocka,
            anna,
            id,
            ..
        } = self;
        let metoder = foretag::företaget(db, anna, *id)
            .await
            .unwrap()
            .bokföringsmetoder;
        let nu = metoder.last().map(|(_, m)| m.kod()).unwrap_or("faktura");
        let till = if nu == "faktura" {
            "kontant"
        } else {
            "faktura"
        };
        let r = foretag::byt_bokföringsmetod(
            db,
            klocka,
            anna,
            *id,
            &maya_domain::foretag::Byte {
                gäller_från: "2027-01-01".into(),
                val: till.into(),
            },
        )
        .await;
        utfall(&mut self.steg, format!("byt metod till {till}"), r);
    }

    async fn t2(&mut self) {
        let r = t2::kör(&self.db, &self.klocka).await;
        utfall(&mut self.steg, "T2-jobbet".into(), r);
        if self.slump.singla() {
            return;
        }
        let Värld {
            db,
            klocka,
            anna,
            id,
            ..
        } = self;
        let utkast = t2::lista(db, anna, *id)
            .await
            .unwrap()
            .into_iter()
            .find(|t| !t.bokfört);
        if let Some(t) = utkast {
            let r = t2::bokför(db, klocka, anna, *id, t.verifikat).await;
            utfall(&mut self.steg, format!("bokför T2 {:?}", t.verifikat), r);
        }
    }

    /// Avstämningen, verifikaten och det som återstår, efter ett steg.
    async fn kontrollera(&self) {
        let steg = || self.steg.join("\n");
        let r = reskontra::reskontran(&self.db, &self.anna, self.id, idag())
            .await
            .unwrap();
        for del in [&r.kund, &r.leverantör] {
            assert_eq!(
                del.differens,
                0,
                "{:?}: reskontran {} mot saldot {}\nposter: {:#?}\nsteg:\n{}",
                del.reskontra,
                del.bokfört,
                del.saldo,
                del.poster,
                steg()
            );
        }
        for v in verifikat::lista(&self.db, &self.anna, self.id, Urval::Alla)
            .await
            .unwrap()
        {
            let v = verifikat::verifikatet(&self.db, &self.anna, self.id, v.id)
                .await
                .unwrap()
                .unwrap();
            let debet: i64 = v.rader.iter().map(|r| r.debet.öre()).sum();
            let kredit: i64 = v.rader.iter().map(|r| r.kredit.öre()).sum();
            assert_eq!(
                debet,
                kredit,
                "verifikat {:?} balanserar inte\nsteg:\n{}",
                v.id,
                steg()
            );
        }
        // Det som återstår är detsamma per datum, i listorna och i fakturans reskontrapost.
        let kund = kundfaktura::lista(&self.db, &self.anna, self.id)
            .await
            .unwrap();
        let lev = leverantorsfaktura::lista(&self.db, &self.anna, self.id)
            .await
            .unwrap();
        for &s in &self.fakturor {
            let listan = match s {
                Sort::Egen(id) | Sort::Extern(id) => {
                    kund.iter().find(|r| r.id == id).map(|r| r.återstår)
                }
                Sort::Leverantör(id) => lev.iter().find(|r| r.id == id).map(|r| r.återstår),
            };
            let post = betalning::reskontrapost(&self.db, &self.anna, self.id, s.faktura())
                .await
                .unwrap()
                .återstår;
            let per_datum = r
                .kund
                .poster
                .iter()
                .chain(&r.leverantör.poster)
                .find(|p| p.faktura == s.faktura())
                .map_or(0, |p| p.återstår);
            assert_eq!(
                listan,
                Some(post),
                "{s:?}: listan mot reskontraposten\nsteg:\n{}",
                steg()
            );
            assert_eq!(
                per_datum,
                post,
                "{s:?}: per datum mot reskontraposten\nsteg:\n{}",
                steg()
            );
        }
    }
}

/// Händelserna som varje bokföringsmetod ska ha gjort minst en gång, som början på stegets
/// text, så att testet inte tyst hoppar över en händelse som alltid spärras.
const HÄNDELSER: &[&str] = &[
    "utfärda ",
    "registrera extern ",
    "registrera L-",
    "betala ",
    "öresutjämning ",
    "efter årets slut ",
    "kursdifferens ",
    "återbetala ",
    "kreditera Egen",
    "kreditera Extern",
    "leverantörskredit med hänvisning",
    "leverantörskredit utan hänvisning",
    "kundförlust ",
    "återkalla ",
    "rätta kontering ",
    "rätta registrering ",
    "rätta avskrift ",
    "registrera om ",
    "rätta betalning ",
    "byt metod ",
    "T2-jobbet",
    "bokför T2 ",
];

/// Kör följderna under `metod`. Varannan följd byter metod från 2027 innan något har hänt,
/// så att båda metoderna och övergången mellan dem prövas. Under fakturametoden 2026 finns
/// inget T2 att bokföra.
async fn kör(metod: &str) {
    let mut gjorda: Vec<&str> = Vec::new();
    if metod == "faktura" {
        gjorda.push("bokför T2 ");
    }
    for frö in 1..=följder() {
        let mut värld = Värld::ny(metod, frö).await;
        if frö % 2 == 0 {
            värld.byt_metod().await;
        }
        for _ in 0..STEG {
            värld.steg().await;
            värld.kontrollera().await;
        }
        for h in HÄNDELSER {
            if värld.steg.iter().any(|s| s.starts_with(h)) && !gjorda.contains(h) {
                gjorda.push(h);
            }
        }
    }
    let saknas: Vec<_> = HÄNDELSER.iter().filter(|h| !gjorda.contains(h)).collect();
    assert!(
        saknas.is_empty(),
        "{metod}: händelserna {saknas:?} gjordes aldrig"
    );
}

#[tokio::test]
async fn avstämningen_ger_noll_efter_varje_fakturahändelse_under_fakturametoden() {
    kör("faktura").await;
}

#[tokio::test]
async fn avstämningen_ger_noll_efter_varje_fakturahändelse_under_kontantmetoden() {
    kör("kontant").await;
}
