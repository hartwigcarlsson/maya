//! Rätta kontering på en utfärdad kundfaktura (#142).
//!
//! Användaren väljer ett nytt intäktskonto per rad med en valfri orsak. I en transaktion vänds
//! fakturans gällande utfärdandeverifikat med en rättelse enligt etapp 1 (R1, R2, VF7), ett nytt
//! utfärdandeverifikat bokförs med dagens datum med samma fordran och moms, intäkterna på de
//! nya kontona och PDF:en som underlag (VF5), och rättningen loggas (H1). Båda verifikaten
//! länkar till fakturan. Fakturan, numret och PDF:en ändras inte (L1).

use maya_domain::kontering::{Intäktsrad, NyKontering, ny_kontering};
use maya_domain::kontoplan::Kontonummer;
use serde_json::json;
use sqlx::SqliteConnection;

use crate::aktor::{Aktör, Handling};
use crate::atkomst::{self, Ärende};
use crate::foretag::FöretagsId;
use crate::kundfaktura::{KundfakturaFel, KundfakturaId};
use crate::underlag::UnderlagId;
use crate::verifikat::{self, Bokfört, Länk, VerifikatFel, VerifikatId};
use crate::{Databas, DatabasFel, Klocka};

/// Resultatet av en rättad kontering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KonteringRättad {
    /// Rättelsen som vände det föregående utfärdandeverifikatet.
    pub rättelse: Bokfört,
    /// Det nya utfärdandeverifikatet.
    pub verifikat: Bokfört,
}

/// En gjord rättning av konteringen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rättning {
    pub rättelse: Länk,
    pub verifikat: Länk,
    pub orsak: String,
}

/// Fakturans kontering som den är bokförd nu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Kontering {
    /// Det gällande utfärdandeverifikatet: det senaste från en rättning, annars utfärdandets.
    pub verifikat: Länk,
    /// Fakturans rader med intäktskontot som de är bokförda på nu.
    pub rader: Vec<Intäktsrad>,
    /// Rättningarna, äldst först.
    pub rättningar: Vec<Rättning>,
}

/// Fakturans gällande kontering. En faktura utan utfärdandeverifikat, t.ex. ett utkast, ger
/// [`VerifikatFel::IngetVerifikat`].
pub async fn kontering(
    db: &Databas,
    aktör: &Aktör,
    företag: FöretagsId,
    faktura: KundfakturaId,
) -> Result<Kontering, KundfakturaFel> {
    let mut tx = db.pool().begin().await?;
    atkomst::kräv(&mut tx, aktör, företag, Handling::LäsaFöretag).await?;
    let gällande = gällande_i(&mut tx, företag, faktura).await?;
    let utfärdandet = gällande.verifikat.ok_or(VerifikatFel::IngetVerifikat)?;
    let verifikat = verifikat::länk(&mut tx, Some(utfärdandet.0))
        .await?
        .ok_or_else(|| DatabasFel::Korrupt("kundfakturans verifikat".into()))?;
    let rader: Vec<(i64, i64, String)> = sqlx::query_as(
        "SELECT rattelse_id, verifikat_id, orsak FROM kundfaktura_kontering
         WHERE faktura_id = ? ORDER BY id",
    )
    .bind(faktura_id(faktura))
    .fetch_all(&mut *tx)
    .await?;
    let länk = |l: Option<Länk>| l.ok_or_else(|| DatabasFel::Korrupt("kontering".into()));
    let mut rättningar = Vec::new();
    for (rättelse, ny, orsak) in rader {
        rättningar.push(Rättning {
            rättelse: länk(verifikat::länk(&mut tx, Some(rättelse)).await?)?,
            verifikat: länk(verifikat::länk(&mut tx, Some(ny)).await?)?,
            orsak,
        });
    }
    Ok(Kontering {
        verifikat,
        rader: gällande.rader,
        rättningar,
    })
}

/// Rättar konteringen av den utfärdade fakturan `faktura` med ett nytt intäktskonto per rad,
/// i en transaktion: rättelsen av det gällande utfärdandeverifikatet och det nya
/// utfärdandeverifikatet, båda daterade i dag, och posten `kontering rättad`. Spärrarna i
/// [`NyKontering::kontrollera`] prövas mot det som är bokfört nu, och rättelsen och det nya
/// verifikatet med samma spärrar som andra verifikat; spärras något bokförs ingenting.
pub async fn rätta(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    företag: FöretagsId,
    faktura: KundfakturaId,
    formulär: &NyKontering,
) -> Result<KonteringRättad, KundfakturaFel> {
    let mut ärende = Ärende::börja(db, klocka, aktör, företag, Handling::RättaFlöde).await?;
    let gällande = gällande_i(ärende.conn(), företag, faktura).await?;
    // Rätta kontering finns bara när utfärdandet har ett verifikat (#149).
    let utfärdandet = gällande.verifikat.ok_or(VerifikatFel::IngetVerifikat)?;
    let konton = verifikat::läs_förutsättningar(ärende.conn(), företag)
        .await?
        .konton;
    let nya = formulär
        .kontrollera(&gällande.rader, &konton)
        .map_err(KundfakturaFel::Spärrat)?;

    let idag = maya_domain::tid::datum(ärende.nu);
    let original = verifikat::läs_verifikat(ärende.conn(), företag, utfärdandet)
        .await?
        .ok_or(VerifikatFel::IngetVerifikat)?;
    let (Some(original_nummer), Some(nuvarande)) = (original.nummer, original.som_original())
    else {
        return Err(VerifikatFel::IngetVerifikat.into());
    };
    let rättelse = verifikat::rätta_i(&mut ärende, utfärdandet, idag, &formulär.orsak).await?;
    let bokfört = verifikat::bokför_i_ärende(
        &mut ärende,
        &verifikat::formulär_av(
            idag,
            &original.beskrivning,
            &ny_kontering(&nuvarande.rader, &gällande.rader, &nya),
        ),
        &[gällande.pdf],
    )
    .await?;

    let (nu, kanal, av) = (
        ärende.nu.to_string(),
        ärende.aktör.kanal.namn(),
        ärende.aktör.användare.map(|a| a.0),
    );
    let kontering: i64 = sqlx::query_scalar(
        "INSERT INTO kundfaktura_kontering
             (faktura_id, rattelse_id, verifikat_id, orsak, rattad, rattad_kanal, rattad_av)
         VALUES (?, ?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(faktura_id(faktura))
    .bind(rättelse.id.0)
    .bind(bokfört.id.0)
    .bind(formulär.orsak.trim())
    .bind(nu)
    .bind(kanal)
    .bind(av)
    .fetch_one(ärende.conn())
    .await?;
    // Det nya verifikatet bokför fakturans fordran (#149).
    crate::fordran::bokför_i(
        ärende.conn(),
        företag,
        crate::betalning::Faktura::Kund(faktura),
        bokfört.id,
    )
    .await?;
    for (i, konto) in nya.iter().enumerate() {
        sqlx::query(
            "INSERT INTO kundfaktura_konteringsrad (kontering_id, radnr, konto) VALUES (?, ?, ?)",
        )
        .bind(kontering)
        .bind(i as i64 + 1)
        .bind(i64::from(konto.get()))
        .execute(ärende.conn())
        .await?;
    }
    let ändrade: Vec<_> = gällande
        .rader
        .iter()
        .zip(&nya)
        .enumerate()
        .filter(|(_, (r, k))| r.konto != **k)
        .map(|(i, (r, k))| {
            json!({ "rad": i + 1, "beskrivning": r.beskrivning, "från": r.konto.get(), "till": k.get() })
        })
        .collect();
    ärende
        .logga(
            crate::behandlingshistorik::Händelse::KonteringRättad,
            &gällande.nummer.to_string(),
            json!({
                "kundfaktura": faktura_id(faktura),
                "nummer": gällande.nummer,
                "kontering": kontering,
                "vänt": { "id": original.id.0, "nummer": original_nummer.to_string() },
                "rättelse": { "id": rättelse.id.0, "nummer": rättelse.nummer.to_string() },
                "verifikat": { "id": bokfört.id.0, "nummer": bokfört.nummer.to_string() },
                "datum": idag.to_string(),
                "orsak": formulär.orsak.trim(),
                "rader": ändrade,
            }),
        )
        .await?;
    ärende.slutför().await?;
    Ok(KonteringRättad {
        rättelse,
        verifikat: bokfört,
    })
}

/// Det som är bokfört för en utfärdad kundfaktura nu.
pub(crate) struct Gällande {
    pub nummer: u64,
    /// Det gällande utfärdandeverifikatet; `None` när utfärdandet inte bokfördes
    /// (kontantmetoden, #149).
    pub verifikat: Option<VerifikatId>,
    pub rader: Vec<Intäktsrad>,
    /// Den arkiverade PDF:en.
    pub pdf: UnderlagId,
}

/// Fakturans gällande utfärdandeverifikat och intäktskonto per rad: från den senaste
/// rättningen av konteringen, annars från utfärdandet. Senare flöden som speglar utfärdandet
/// (kreditfaktura, kundförlust) läser konteringen härifrån.
pub(crate) async fn gällande_i(
    conn: &mut SqliteConnection,
    företag: FöretagsId,
    faktura: KundfakturaId,
) -> Result<Gällande, KundfakturaFel> {
    type Huvud = (String, Option<i64>, Option<i64>, Option<i64>);
    let id = faktura_id(faktura);
    let huvud: Option<Huvud> = sqlx::query_as(
        "SELECT status, nummer, verifikat_id, underlag_id FROM kundfaktura
         WHERE id = ? AND foretag_id = ?",
    )
    .bind(id)
    .bind(företag.0)
    .fetch_optional(&mut *conn)
    .await?;
    let (status, nummer, utfärdandet, pdf) = huvud.ok_or(KundfakturaFel::FinnsInte)?;
    let (Some(nummer), Some(pdf), true) = (nummer, pdf, status != "utkast") else {
        return Err(VerifikatFel::IngetVerifikat.into());
    };
    let senaste: Option<(i64, i64)> = sqlx::query_as(
        "SELECT id, verifikat_id FROM kundfaktura_kontering WHERE faktura_id = ?
         ORDER BY id DESC LIMIT 1",
    )
    .bind(id)
    .fetch_optional(&mut *conn)
    .await?;
    let rader: Vec<(String, i64, i64, Option<i64>)> = sqlx::query_as(
        "SELECT r.beskrivning, r.netto, r.konto,
                (SELECT k.konto FROM kundfaktura_konteringsrad k
                 WHERE k.kontering_id = ?2 AND k.radnr = r.radnr)
         FROM kundfakturarad r WHERE r.faktura_id = ?1 ORDER BY r.radnr",
    )
    .bind(id)
    .bind(senaste.map(|(k, _)| k))
    .fetch_all(&mut *conn)
    .await?;
    let rader = rader
        .into_iter()
        .map(|(beskrivning, netto, konto, rättat)| {
            Ok(Intäktsrad {
                beskrivning,
                netto,
                konto: kontonummer(rättat.unwrap_or(konto))?,
            })
        })
        .collect::<Result<_, DatabasFel>>()?;
    Ok(Gällande {
        nummer: nummer.unsigned_abs(),
        verifikat: senaste.map(|(_, v)| v).or(utfärdandet).map(VerifikatId),
        rader,
        pdf: UnderlagId(pdf),
    })
}

fn faktura_id(faktura: KundfakturaId) -> i64 {
    faktura.0
}

fn kontonummer(nummer: i64) -> Result<Kontonummer, DatabasFel> {
    u16::try_from(nummer)
        .ok()
        .and_then(Kontonummer::new)
        .ok_or_else(|| DatabasFel::Korrupt(format!("ett kontonummer: {nummer}")))
}
