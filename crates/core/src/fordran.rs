//! Fakturans fordran eller skuld i bokföringen (#149): om den är bokförd på 1510 eller 2440.
//!
//! Varje faktura vet det. Fordran eller skulden är bokförd från utfärdandet eller registreringen
//! under fakturametoden och från T2-verifikatet under kontantmetoden (#150): ett sådant
//! verifikat står i tabellen `bokford_reskontrapost`, och fordran eller skulden är bokförd så
//! länge verifikatet inte är vänt (Rätta registrering och Rätta avskrift vänder det; Rätta
//! kontering vänder det och bokför ett nytt, som också står där). Bokföringsmetoden på
//! fakturadatumet avgör om utfärdandet eller registreringen bokförs ([`fakturametoden_på`]),
//! så ett metodbyte kräver ingen övergångsbokning.
//!
//! När fordran eller skulden inte är bokförd bokförs betalningar och krediter mot fakturans
//! intäkt eller kostnad och moms ([`maya_domain::kontantmetoden::Obokförd`], [`obokförd_i`]).

use jiff::civil::Date;
use maya_domain::faktura::{Momssats, momsrader};
use maya_domain::foretag::{Bokföringsmetod, Momsperiod, gäller};
use maya_domain::kontantmetoden::Obokförd;
use maya_domain::leverantor::OmvändSkattskyldighet;
use maya_domain::leverantorsfaktura::{Rad, registreringens_kontering};
use sqlx::SqliteConnection;

use crate::DatabasFel;
use crate::betalning::Faktura;
use crate::foretag::{self, FöretagsId, datum};
use crate::kundfaktura::kontonummer;
use crate::verifikat::VerifikatId;

/// Om fakturans fordran eller skuld är bokförd nu: ett verifikat har bokfört den och är inte
/// vänt. #150: en betalning efter årets slut av en faktura som var obetald vid årets slut under
/// kontantmetoden räknas som bokförd också innan T2-verifikatet är bokfört; den regeln hör
/// hemma här.
pub(crate) async fn bokförd_i(
    conn: &mut SqliteConnection,
    faktura: Faktura,
) -> Result<bool, sqlx::Error> {
    let (kund, leverantör) = faktura.kolumner();
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM reskontrapost_bokford
                        WHERE kundfaktura_id = ?1 OR leverantorsfaktura_id = ?2)",
    )
    .bind(kund)
    .bind(leverantör)
    .fetch_one(conn)
    .await
}

/// Att verifikatet `verifikat` bokförde fakturans fordran eller skuld: utfärdandet,
/// registreringen, en Rätta kontering eller T2 (#150). Fakturan ska vara utfärdad eller
/// registrerad och verifikatet bokfört.
pub(crate) async fn bokför_i(
    conn: &mut SqliteConnection,
    företag: FöretagsId,
    faktura: Faktura,
    verifikat: VerifikatId,
) -> Result<(), sqlx::Error> {
    let (kund, leverantör) = faktura.kolumner();
    sqlx::query(
        "INSERT INTO bokford_reskontrapost
             (foretag_id, kundfaktura_id, leverantorsfaktura_id, verifikat_id)
         VALUES (?, ?, ?, ?)",
    )
    .bind(företag.0)
    .bind(kund)
    .bind(leverantör)
    .bind(verifikat.0)
    .execute(conn)
    .await?;
    Ok(())
}

/// Om bokföringsmetoden på `dag` är fakturametoden, så att en faktura daterad den dagen bokförs
/// när den utfärdas eller registreras.
pub(crate) async fn fakturametoden_på(
    conn: &mut SqliteConnection,
    företag: FöretagsId,
    dag: Date,
) -> Result<bool, DatabasFel> {
    let metoder = foretag::läs_företag(conn, företag).await?.bokföringsmetoder;
    let metod = gäller(&metoder, dag)
        .or(metoder.first().map(|(_, m)| *m))
        .ok_or_else(|| DatabasFel::Korrupt("bokforingsmetod".into()))?;
    Ok(metod == Bokföringsmetod::Faktura)
}

/// Fakturans kontering när fordran eller skulden inte är bokförd, eller `None` när den är det.
pub(crate) async fn obokförd_i(
    conn: &mut SqliteConnection,
    företag: FöretagsId,
    faktura: Faktura,
) -> Result<Option<Obokförd>, DatabasFel> {
    if bokförd_i(&mut *conn, faktura).await? {
        return Ok(None);
    }
    Ok(Some(underlaget_i(conn, företag, faktura).await?))
}

/// Fakturans kontering under fakturametoden ur de utfärdade eller registrerade raderna, som
/// andelarna räknas ur: 1510 mot intäkt och utgående moms för en kundfaktura, kostnad, ingående
/// moms (och fiktiv moms) mot 2440 för en leverantörsfaktura.
pub(crate) async fn underlaget_i(
    conn: &mut SqliteConnection,
    företag: FöretagsId,
    faktura: Faktura,
) -> Result<Obokförd, DatabasFel> {
    let korrupt = |vad: &str| DatabasFel::Korrupt(format!("{faktura:?}: {vad}"));
    match faktura {
        Faktura::Kund(id) => {
            let total: Option<i64> =
                sqlx::query_scalar("SELECT total FROM kundfaktura WHERE id = ? AND foretag_id = ?")
                    .bind(id.0)
                    .bind(företag.0)
                    .fetch_optional(&mut *conn)
                    .await?
                    .flatten();
            let total = total.ok_or_else(|| korrupt("total"))?;
            let rader: Vec<(i64, i64, i64)> = sqlx::query_as(
                "SELECT konto, netto, momssats FROM kundfakturarad
                 WHERE faktura_id = ? ORDER BY radnr",
            )
            .bind(id.0)
            .fetch_all(&mut *conn)
            .await?;
            let mut konton = Vec::new();
            let mut satser = Vec::new();
            for (konto, netto, sats) in rader {
                let sats = u8::try_from(sats)
                    .ok()
                    .and_then(Momssats::från_procent)
                    .ok_or_else(|| korrupt("momssats"))?;
                konton.push((kontonummer(konto)?, netto));
                satser.push((sats, netto));
            }
            let moms = momsrader(satser);
            let summa: i64 = konton.iter().map(|(_, n)| n).sum::<i64>()
                + moms.iter().map(|m| m.moms).sum::<i64>();
            if summa != total {
                return Err(korrupt("raderna summerar inte till totalen"));
            }
            let kontering = maya_domain::kundfaktura::fordringens_kontering(total, konton, &moms);
            Ok(Obokförd::av(
                &kontering,
                maya_domain::betalning::Reskontra::Kund.konto(),
            ))
        }
        Faktura::Leverantör(id) => {
            let huvud: Option<(Option<String>, Option<i64>, Option<String>)> = sqlx::query_as(
                "SELECT fakturadatum, total, omvand_skattskyldighet FROM leverantorsfaktura
                 WHERE id = ? AND foretag_id = ?",
            )
            .bind(id.0)
            .bind(företag.0)
            .fetch_optional(&mut *conn)
            .await?;
            let (Some(fakturadatum), Some(total), omvänd) =
                huvud.ok_or_else(|| korrupt("fakturan"))?
            else {
                return Err(korrupt("fakturadatum eller total"));
            };
            let omvänd = match omvänd {
                None => OmvändSkattskyldighet::Ingen,
                Some(kod) => {
                    OmvändSkattskyldighet::från_kod(&kod).ok_or_else(|| korrupt("omvänd"))?
                }
            };
            let momsperioder = foretag::läs_företag(&mut *conn, företag)
                .await?
                .momsperioder;
            let dag = datum(&fakturadatum)?;
            let momsregistrerat = gäller(&momsperioder, dag)
                .or_else(|| momsperioder.first().map(|(_, m)| *m))
                .is_none_or(|m| m != Momsperiod::EjMomsregistrerad);
            let rader: Vec<(Option<i64>, i64, i64, i64)> = sqlx::query_as(
                "SELECT konto, netto, momssats, moms FROM leverantorsfakturarad
                 WHERE faktura_id = ? ORDER BY radnr",
            )
            .bind(id.0)
            .fetch_all(&mut *conn)
            .await?;
            let rader = rader
                .into_iter()
                .map(|(konto, netto, sats, moms)| {
                    Ok(Rad {
                        konto: kontonummer(konto.ok_or_else(|| korrupt("konto"))?)?,
                        netto,
                        momssats: u8::try_from(sats).map_err(|_| korrupt("momssats"))?,
                        moms,
                    })
                })
                .collect::<Result<Vec<_>, DatabasFel>>()?;
            let kontering = registreringens_kontering(&rader, momsregistrerat, omvänd);
            let o = Obokförd::av(
                &kontering,
                maya_domain::betalning::Reskontra::Leverantör.konto(),
            );
            if o.total() != -total {
                return Err(korrupt("raderna summerar inte till totalen"));
            }
            Ok(o)
        }
    }
}

/// Konteringen för en kreditfaktura med totalen `total` (negativ) som krediterar `original`.
/// `kontering` är kreditfakturans kontering under fakturametoden (utfärdandets eller
/// registreringens, med debet och kredit bytta mot originalets).
///
/// Är originalets fordran eller skuld bokförd speglar krediten originalet. Annars bokförs bara
/// den del av krediten som överstiger det som var obetalt av originalet, mot 1510 eller 2440
/// (kontantmetoden: kredit av en obetald del bokförs inte), och `None` när krediten ryms i det
/// obetalda. Kreditfakturan bokför i inget av fallen en egen fordran eller skuld: den räknas av
/// mot originalet.
pub(crate) async fn kreditens_kontering(
    conn: &mut SqliteConnection,
    original: Faktura,
    total: i64,
    kontering: Vec<maya_domain::verifikat::Rad>,
) -> Result<Option<Vec<maya_domain::verifikat::Rad>>, DatabasFel> {
    if bokförd_i(&mut *conn, original).await? {
        return Ok(Some(kontering));
    }
    let (reskontrakonto, originalets_total): (_, Option<i64>) = match original {
        Faktura::Kund(id) => (
            maya_domain::betalning::Reskontra::Kund.konto(),
            sqlx::query_scalar("SELECT total FROM kundfaktura WHERE id = ?")
                .bind(id.0)
                .fetch_one(&mut *conn)
                .await?,
        ),
        Faktura::Leverantör(id) => (
            maya_domain::betalning::Reskontra::Leverantör.konto(),
            sqlx::query_scalar("SELECT total FROM leverantorsfaktura WHERE id = ?")
                .bind(id.0)
                .fetch_one(&mut *conn)
                .await?,
        ),
    };
    let obetalt =
        crate::betalning::återstår_i(&mut *conn, original, originalets_total.unwrap_or(0)).await?;
    let del = maya_domain::kontantmetoden::kreditens_bokförda_del(-total, obetalt);
    if del == 0 {
        return Ok(None);
    }
    let o = Obokförd::av(&kontering, reskontrakonto);
    // Delen har kreditfakturans tecken på reskontrakontot.
    Ok(Some(o.kontering(del * o.total().signum())))
}
