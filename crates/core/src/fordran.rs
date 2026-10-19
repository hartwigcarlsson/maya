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
use maya_domain::kontantmetoden::{Kreditläge, Obokförd};
use maya_domain::leverantor::OmvändSkattskyldighet;
use maya_domain::leverantorsfaktura::{Rad, registreringens_kontering};
use sqlx::SqliteConnection;

use crate::DatabasFel;
use crate::betalning::Faktura;
use crate::foretag::{self, FöretagsId, datum};
use crate::kundfaktura::kontonummer;
use crate::verifikat::VerifikatId;

/// Om fakturans fordran eller skuld är bokförd nu: ett verifikat har bokfört den och är inte
/// vänt. För en händelse med ett datum (betalning, kredit, kundförlust) gäller också
/// [`bokförd_på_i`]: efter årets slut är en faktura som var obetald vid årets slut bokförd också
/// innan T2-verifikatet är bokfört (#150).
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

/// Om ett bokfört T2-verifikat har bokfört fakturans fordran eller skuld (#150). T2 vänds
/// aldrig, så en sådan faktura kan inte låsas upp med Rätta registrering eller Rätta avskrift,
/// och en betalning som bokfördes mot intäkt eller kostnad före T2 kan inte rättas: 1510 eller
/// 2440 skulle då avvika från reskontran (#151).
pub(crate) async fn i_bokfört_t2_i(
    conn: &mut SqliteConnection,
    faktura: Faktura,
) -> Result<bool, sqlx::Error> {
    let (kund, leverantör) = faktura.kolumner();
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM bokford_reskontrapost b
                        JOIN t2 ON t2.verifikat_id = b.verifikat_id
                        WHERE b.kundfaktura_id = ?1 OR b.leverantorsfaktura_id = ?2)",
    )
    .bind(kund)
    .bind(leverantör)
    .fetch_one(conn)
    .await
}

/// Om fakturans fordran eller skuld är bokförd för en händelse daterad `dag`: den är bokförd
/// ([`bokförd_i`]), eller `dag` ligger efter årets slut och fakturan var obetald då och väntar
/// på T2-verifikatet ([`bokförd_efter_i`]). Spec: en betalning daterad efter räkenskapsårets
/// slut av en faktura som var obetald vid årets slut bokförs mot 1510 eller 2440, också innan
/// T2-verifikatet är bokfört. Krediter och kundförluster följer samma regel, eftersom
/// T2-verifikatet tar med det som var obetalt vid årets slut och bokför det på 1510 eller 2440.
pub(crate) async fn bokförd_på_i(
    conn: &mut SqliteConnection,
    företag: FöretagsId,
    faktura: Faktura,
    dag: Date,
) -> Result<bool, DatabasFel> {
    Ok(bokförd_i(&mut *conn, faktura).await?
        || bokförd_efter_i(conn, företag, faktura)
            .await?
            .is_some_and(|slut| dag > slut))
}

/// Slutet på räkenskapsåret med fakturans fakturadatum, när fakturans fordran eller skuld inte
/// är bokförd, något var obetalt av den vid årets slut och årets T2-verifikat inte är bokfört
/// (#150): händelser daterade efter det bokförs mot 1510 eller 2440, eftersom T2-verifikatet
/// tar med fakturan. `None` annars, också för en kreditfaktura, som räknas av mot sitt original.
pub(crate) async fn bokförd_efter_i(
    conn: &mut SqliteConnection,
    företag: FöretagsId,
    faktura: Faktura,
) -> Result<Option<Date>, DatabasFel> {
    if bokförd_i(&mut *conn, faktura).await? {
        return Ok(None);
    }
    let fakturadatum: Option<Option<String>> = match faktura {
        Faktura::Kund(id) => {
            sqlx::query_scalar(
                "SELECT fakturadatum FROM kundfaktura
                 WHERE id = ? AND foretag_id = ? AND status IN ('utfardad', 'registrerad')
                   AND krediterar IS NULL",
            )
            .bind(id.0)
            .bind(företag.0)
            .fetch_optional(&mut *conn)
            .await?
        }
        Faktura::Leverantör(id) => {
            sqlx::query_scalar(
                "SELECT fakturadatum FROM leverantorsfaktura
                 WHERE id = ? AND foretag_id = ? AND status = 'registrerad' AND krediterar IS NULL",
            )
            .bind(id.0)
            .bind(företag.0)
            .fetch_optional(&mut *conn)
            .await?
        }
    };
    let Some(Some(fakturadatum)) = fakturadatum else {
        return Ok(None);
    };
    let Some(slut) = t2_inte_bokfört_i(&mut *conn, företag, datum(&fakturadatum)?).await? else {
        return Ok(None);
    };
    Ok((obetalt_vid_årets_slut_i(conn, faktura, slut).await? > 0).then_some(slut))
}

/// Slutet på räkenskapsåret med `fakturadatum`, när årets T2-verifikat inte är bokfört.
async fn t2_inte_bokfört_i(
    conn: &mut SqliteConnection,
    företag: FöretagsId,
    fakturadatum: Date,
) -> Result<Option<Date>, DatabasFel> {
    let år: Option<(String, bool)> = sqlx::query_as(
        "SELECT r.slut, EXISTS (SELECT 1 FROM t2 JOIN verifikat v ON v.id = t2.verifikat_id
                                WHERE t2.rakenskapsar_id = r.id AND v.bokford IS NOT NULL)
         FROM rakenskapsar r WHERE r.foretag_id = ?1 AND r.start <= ?2 AND r.slut >= ?2",
    )
    .bind(företag.0)
    .bind(fakturadatum.to_string())
    .fetch_optional(&mut *conn)
    .await?;
    match år {
        Some((slut, false)) => Ok(Some(datum(&slut)?)),
        _ => Ok(None),
    }
}

/// Om fakturans fordran eller skuld var bokförd vid utgången av `dag`, som bokföringen visar
/// det: ett verifikat daterat senast `dag` bokförde den och var inte vänt med en rättelse
/// daterad senast `dag` (reskontran per datum, #151).
pub(crate) async fn bokförd_vid_i(
    conn: &mut SqliteConnection,
    faktura: Faktura,
    dag: Date,
) -> Result<bool, sqlx::Error> {
    let (kund, leverantör) = faktura.kolumner();
    sqlx::query_scalar(
        "SELECT EXISTS (
             SELECT 1 FROM bokford_reskontrapost b JOIN verifikat v ON v.id = b.verifikat_id
             WHERE (b.kundfaktura_id = ?1 OR b.leverantorsfaktura_id = ?2) AND v.datum <= ?3
               AND NOT EXISTS (SELECT 1 FROM verifikat r
                               WHERE r.rattar = b.verifikat_id AND r.bokford IS NOT NULL
                                 AND r.datum <= ?3))",
    )
    .bind(kund)
    .bind(leverantör)
    .bind(dag.to_string())
    .fetch_one(conn)
    .await
}

/// Om ett verifikat daterat senast `dag` har bokfört fakturans fordran eller skuld, vänt eller
/// inte: en upplåst faktura som var bokförd (#151).
pub(crate) async fn var_bokförd_vid_i(
    conn: &mut SqliteConnection,
    faktura: Faktura,
    dag: Date,
) -> Result<bool, sqlx::Error> {
    let (kund, leverantör) = faktura.kolumner();
    sqlx::query_scalar(
        "SELECT EXISTS (
             SELECT 1 FROM bokford_reskontrapost b JOIN verifikat v ON v.id = b.verifikat_id
             WHERE (b.kundfaktura_id = ?1 OR b.leverantorsfaktura_id = ?2) AND v.datum <= ?3)",
    )
    .bind(kund)
    .bind(leverantör)
    .bind(dag.to_string())
    .fetch_one(conn)
    .await
}

/// Det obetalda vid årets slut som väntar på årets T2-verifikat vid utgången av `dag`, för en
/// faktura vars fordran eller skuld inte är bokförd då: `dag` ligger efter slutet på
/// räkenskapsåret med `fakturadatum`, årets T2 är inte bokfört och något var obetalt vid årets
/// slut. Det är regeln i [`bokförd_efter_i`] per datum (#151).
pub(crate) async fn väntar_på_t2_vid_i(
    conn: &mut SqliteConnection,
    företag: FöretagsId,
    faktura: Faktura,
    fakturadatum: Date,
    dag: Date,
) -> Result<Option<i64>, DatabasFel> {
    let Some(slut) = t2_inte_bokfört_i(&mut *conn, företag, fakturadatum).await? else {
        return Ok(None);
    };
    if dag <= slut {
        return Ok(None);
    }
    let obetalt = obetalt_vid_årets_slut_i(conn, faktura, slut).await?;
    Ok((obetalt > 0).then_some(obetalt))
}

/// Det som var obetalt av fakturan vid utgången av `dag`, i öre, som bokföringen visade det
/// då: totalen minus krediter med fakturadatum senast `dag`, betalningar med betaldag senast
/// `dag` som inte var vända med en rättelse daterad senast `dag`, och (kundfaktura) en
/// kundförlust daterad senast `dag` som inte var återkallad då. Reskontran per datum (#151)
/// visar det.
pub(crate) async fn obetalt_vid_i(
    conn: &mut SqliteConnection,
    faktura: Faktura,
    dag: Date,
) -> Result<i64, sqlx::Error> {
    obetalt_i(conn, faktura, dag, true).await
}

/// Det som återstod av fakturan vid utgången av varje dag med en händelse på den, i
/// datumordning, som reskontran visar det nu: en rättad betalning räknas aldrig, eftersom den
/// var fel, som för T2 ([`obetalt_vid_årets_slut_i`]). Dagarna är fakturadatumet och dagarna
/// för krediter, betalningar, rättelser av betalningar, kundförluster och återkallelser (#151).
pub(crate) async fn återstod_i(
    conn: &mut SqliteConnection,
    faktura: Faktura,
) -> Result<Vec<(Date, i64)>, DatabasFel> {
    let sql = match faktura {
        Faktura::Kund(_) => {
            "SELECT fakturadatum FROM kundfaktura WHERE id = ?1 AND fakturadatum IS NOT NULL
             UNION SELECT fakturadatum FROM kundfaktura
                   WHERE krediterar = ?1 AND status IN ('utfardad', 'registrerad')
             UNION SELECT betaldag FROM betalning WHERE kundfaktura_id = ?1
             UNION SELECT v.datum FROM betalningsrattelse r JOIN betalning b ON b.id = r.betalning_id
                   JOIN verifikat v ON v.id = r.verifikat_id WHERE b.kundfaktura_id = ?1
             UNION SELECT datum FROM kundforlust WHERE kundfaktura_id = ?1
             UNION SELECT v.datum FROM kundforlust_aterkallelse a
                   JOIN kundforlust l ON l.id = a.kundforlust_id
                   JOIN verifikat v ON v.id = a.verifikat_id WHERE l.kundfaktura_id = ?1
             ORDER BY 1"
        }
        Faktura::Leverantör(_) => {
            "SELECT fakturadatum FROM leverantorsfaktura WHERE id = ?1 AND fakturadatum IS NOT NULL
             UNION SELECT fakturadatum FROM leverantorsfaktura
                   WHERE krediterar = ?1 AND status = 'registrerad'
             UNION SELECT betaldag FROM betalning WHERE leverantorsfaktura_id = ?1
             UNION SELECT v.datum FROM betalningsrattelse r JOIN betalning b ON b.id = r.betalning_id
                   JOIN verifikat v ON v.id = r.verifikat_id WHERE b.leverantorsfaktura_id = ?1
             ORDER BY 1"
        }
    };
    let (kund, leverantör) = faktura.kolumner();
    let dagar: Vec<String> = sqlx::query_scalar(sql)
        .bind(kund.or(leverantör))
        .fetch_all(&mut *conn)
        .await?;
    let mut återstod = Vec::new();
    for dag in dagar {
        let dag = datum(&dag)?;
        återstod.push((dag, obetalt_i(&mut *conn, faktura, dag, false).await?));
    }
    Ok(återstod)
}

/// Det som T2-verifikatet tar med för årets slut `slut` (#150): det som var obetalt då, som
/// reskontran visar det nu. En betalning i året som har rättats efter årets slut var fel och
/// räknas inte som betald, så att fordran eller skulden kommer med i T2 och står på 1510 eller
/// 2440 när betalningen är borta ur reskontran.
pub(crate) async fn obetalt_vid_årets_slut_i(
    conn: &mut SqliteConnection,
    faktura: Faktura,
    slut: Date,
) -> Result<i64, sqlx::Error> {
    obetalt_i(conn, faktura, slut, false).await
}

/// Det som var obetalt vid utgången av `dag`. Med `per_datum` räknas en betalning som betald
/// tills dagen för dess rättelse, och en bokförd kundförlust som gällande tills dagen för
/// återkallelsens verifikat; annars räknas en rättad betalning och en återkallad kundförlust
/// aldrig. En kundförlust som bara är en status har inget datum för återkallelsen.
async fn obetalt_i(
    conn: &mut SqliteConnection,
    faktura: Faktura,
    dag: Date,
    per_datum: bool,
) -> Result<i64, sqlx::Error> {
    let sql = match faktura {
        Faktura::Kund(_) => {
            "SELECT f.total
                 - coalesce((SELECT -sum(k.total) FROM kundfaktura k
                             WHERE k.krediterar = f.id AND k.status IN ('utfardad', 'registrerad')
                               AND k.fakturadatum <= ?2), 0)
                 - coalesce((SELECT sum(b.belopp + b.oresutjamning) FROM betalning b
                             WHERE b.kundfaktura_id = f.id AND b.betaldag <= ?2
                               AND NOT EXISTS (
                                   SELECT 1 FROM betalningsrattelse r
                                   JOIN verifikat v ON v.id = r.verifikat_id
                                   WHERE r.betalning_id = b.id AND (?3 = 0 OR v.datum <= ?2))), 0)
                 - coalesce((SELECT sum(l.belopp) FROM kundforlust l
                             WHERE l.kundfaktura_id = f.id AND l.datum <= ?2
                               AND NOT EXISTS (
                                   SELECT 1 FROM kundforlust_aterkallelse a
                                   LEFT JOIN verifikat v ON v.id = a.verifikat_id
                                   WHERE a.kundforlust_id = l.id
                                     AND (?3 = 0 OR a.verifikat_id IS NULL OR v.datum <= ?2))), 0)
             FROM kundfaktura f WHERE f.id = ?1"
        }
        Faktura::Leverantör(_) => {
            "SELECT f.total
                 - coalesce((SELECT -sum(k.total) FROM leverantorsfaktura k
                             WHERE k.krediterar = f.id AND k.status = 'registrerad'
                               AND k.fakturadatum <= ?2), 0)
                 - coalesce((SELECT sum(b.belopp + b.oresutjamning + b.kursdifferens)
                             FROM betalning b
                             WHERE b.leverantorsfaktura_id = f.id AND b.betaldag <= ?2
                               AND NOT EXISTS (
                                   SELECT 1 FROM betalningsrattelse r
                                   JOIN verifikat v ON v.id = r.verifikat_id
                                   WHERE r.betalning_id = b.id AND (?3 = 0 OR v.datum <= ?2))), 0)
             FROM leverantorsfaktura f WHERE f.id = ?1"
        }
    };
    let id = match faktura {
        Faktura::Kund(id) => id.0,
        Faktura::Leverantör(id) => id.0,
    };
    let obetalt: Option<Option<i64>> = sqlx::query_scalar(sql)
        .bind(id)
        .bind(dag.to_string())
        .bind(per_datum)
        .fetch_optional(conn)
        .await?;
    Ok(obetalt.flatten().unwrap_or(0))
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

/// Originalets läge för en kreditfaktura som krediterar det: om fordran eller skulden är
/// bokförd, om den väntar på årets T2-verifikat, det som återstår och det som var obetalt vid
/// årets slut. En kredit i året av en faktura som väntar på T2 minskar också det som T2 tar med.
pub(crate) async fn kreditläge_i(
    conn: &mut SqliteConnection,
    företag: FöretagsId,
    original: Faktura,
) -> Result<Kreditläge, DatabasFel> {
    let bokförd_efter = bokförd_efter_i(&mut *conn, företag, original).await?;
    let vid_årets_slut = match bokförd_efter {
        Some(slut) => obetalt_vid_årets_slut_i(&mut *conn, original, slut).await?,
        None => 0,
    };
    Ok(Kreditläge {
        bokförd: bokförd_i(&mut *conn, original).await?,
        bokförd_efter,
        obetalt: crate::betalning::återstår_i(&mut *conn, original).await?,
        vid_årets_slut,
    })
}

/// Konteringen för en kreditfaktura med totalen `total` (negativ) som krediterar `original`.
/// `kontering` är kreditfakturans kontering under fakturametoden: utfärdandets, eller för en
/// leverantörs kreditfaktura originalets registrering speglad i proportion till krediten
/// ([`maya_domain::kontantmetoden::Obokförd::spegling`]).
///
/// Är originalets fordran eller skuld bokförd speglar krediten originalet. Annars bokförs bara
/// den del av krediten som överstiger det som var obetalt av originalet, mot 1510 eller 2440
/// (kontantmetoden: kredit av en obetald del bokförs inte), och `None` när krediten ryms i det
/// obetalda. Kreditfakturan bokför i inget av fallen en egen fordran eller skuld: den räknas av
/// mot originalet. `dag` är kreditfakturans fakturadatum ([`bokförd_på_i`], #150).
pub(crate) async fn kreditens_kontering(
    conn: &mut SqliteConnection,
    företag: FöretagsId,
    original: Faktura,
    dag: Date,
    total: i64,
    kontering: Vec<maya_domain::verifikat::Rad>,
) -> Result<Option<Vec<maya_domain::verifikat::Rad>>, DatabasFel> {
    if bokförd_på_i(&mut *conn, företag, original, dag).await? {
        return Ok(Some(kontering));
    }
    let del = kreditläge_i(&mut *conn, företag, original)
        .await?
        .bokförd_del(-total, dag);
    if del == 0 {
        return Ok(None);
    }
    let reskontrakonto = match original {
        Faktura::Kund(_) => maya_domain::betalning::Reskontra::Kund.konto(),
        Faktura::Leverantör(_) => maya_domain::betalning::Reskontra::Leverantör.konto(),
    };
    let o = Obokförd::av(&kontering, reskontrakonto);
    // Delen har kreditfakturans tecken på reskontrakontot.
    Ok(Some(o.kontering(del * o.total().signum())))
}
