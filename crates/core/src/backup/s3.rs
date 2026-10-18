//! En liten S3-klient för backupmålet: PUT, GET, DELETE och ListObjectsV2 med path-style-adresser
//! (`<endpoint>/<bucket>/<nyckel>`, som alla S3-kompatibla tjänster förstår) och AWS Signature
//! Version 4. Blockerande (ureq); anropas från `spawn_blocking`.

use std::fmt::Write as _;
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;
use std::time::Duration;

use sha2::{Digest, Sha256};
use url::Url;

use crate::Klocka;
use crate::config::S3Target;

/// SHA-256 av en tom kropp.
const TOM: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

#[derive(Debug, thiserror::Error)]
pub(crate) enum S3Fel {
    #[error("S3-målet svarade {status} på {metod} {nyckel}: {text}")]
    Svar {
        metod: &'static str,
        nyckel: String,
        status: u16,
        text: String,
    },
    #[error("{0} finns inte i S3-målet")]
    FinnsInte(String),
    #[error("kan inte nå S3-målet: {0}")]
    Anslutning(#[from] ureq::Error),
    #[error("{0}")]
    Io(#[from] io::Error),
}

#[derive(Clone)]
pub(crate) struct S3 {
    mål: S3Target,
    agent: ureq::Agent,
    /// Tiden i signaturen (SigV4).
    klocka: Klocka,
}

impl S3 {
    pub(crate) fn ny(mål: &S3Target) -> Self {
        let config = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_connect(Some(Duration::from_secs(30)))
            .build();
        S3 {
            mål: mål.clone(),
            agent: ureq::Agent::new_with_config(config),
            klocka: Klocka::system(),
        }
    }

    pub(crate) fn med_klocka(mut self, klocka: Klocka) -> Self {
        self.klocka = klocka;
        self
    }

    /// Bucketen och tjänstens adress, för meddelanden.
    pub(crate) fn beskrivning(&self) -> String {
        format!("{} på {}", self.mål.bucket, self.mål.endpoint)
    }

    /// Laddar upp filen `path`, vars SHA-256 (hex) är `sha256`, som `nyckel`.
    pub(crate) fn ladda_upp(&self, nyckel: &str, path: &Path, sha256: &str) -> Result<(), S3Fel> {
        let fil = File::open(path)?;
        let (url, auth, datum) = self.signera("PUT", nyckel, &[], sha256);
        let svar = self
            .agent
            .put(url.as_str())
            .header("authorization", auth)
            .header("x-amz-date", datum)
            .header("x-amz-content-sha256", sha256)
            .send(fil)?;
        self.kontrollera("PUT", nyckel, svar).map(|_| ())
    }

    /// Laddar upp `innehåll` som `nyckel`.
    pub(crate) fn lägg(&self, nyckel: &str, innehåll: &[u8]) -> Result<(), S3Fel> {
        let sha256 = hex::encode(Sha256::digest(innehåll));
        let (url, auth, datum) = self.signera("PUT", nyckel, &[], &sha256);
        let svar = self
            .agent
            .put(url.as_str())
            .header("authorization", auth)
            .header("x-amz-date", datum)
            .header("x-amz-content-sha256", sha256)
            .send(innehåll)?;
        self.kontrollera("PUT", nyckel, svar).map(|_| ())
    }

    /// Hämtar `nyckel` till filen `till`.
    pub(crate) fn hämta(&self, nyckel: &str, till: &Path) -> Result<(), S3Fel> {
        let svar = self.get(nyckel, &[])?;
        let mut fil = File::create(till)?;
        io::copy(&mut svar.into_body().into_reader(), &mut fil)?;
        fil.sync_all()?;
        Ok(())
    }

    /// Hämtar `nyckel` till minnet.
    pub(crate) fn läs(&self, nyckel: &str) -> Result<Vec<u8>, S3Fel> {
        let svar = self.get(nyckel, &[])?;
        let mut innehåll = Vec::new();
        svar.into_body().into_reader().read_to_end(&mut innehåll)?;
        Ok(innehåll)
    }

    pub(crate) fn radera(&self, nyckel: &str) -> Result<(), S3Fel> {
        let (url, auth, datum) = self.signera("DELETE", nyckel, &[], TOM);
        let svar = self
            .agent
            .delete(url.as_str())
            .header("authorization", auth)
            .header("x-amz-date", datum)
            .header("x-amz-content-sha256", TOM)
            .call()?;
        self.kontrollera("DELETE", nyckel, svar).map(|_| ())
    }

    /// Alla nycklar i bucketen som börjar med `prefix`.
    pub(crate) fn lista(&self, prefix: &str) -> Result<Vec<String>, S3Fel> {
        let mut nycklar = Vec::new();
        let mut fortsättning: Option<String> = None;
        loop {
            let mut fråga = vec![
                ("list-type", "2".to_string()),
                ("prefix", prefix.to_string()),
            ];
            if let Some(token) = &fortsättning {
                fråga.push(("continuation-token", token.clone()));
            }
            let svar = self.get("", &fråga)?;
            let mut xml = String::new();
            svar.into_body().into_reader().read_to_string(&mut xml)?;
            nycklar.extend(element(&xml, "Key"));
            fortsättning = element(&xml, "NextContinuationToken").into_iter().next();
            let trunkerad =
                element(&xml, "IsTruncated").first().map(String::as_str) == Some("true");
            if !trunkerad || fortsättning.is_none() {
                return Ok(nycklar);
            }
        }
    }

    fn get(
        &self,
        nyckel: &str,
        fråga: &[(&str, String)],
    ) -> Result<ureq::http::Response<ureq::Body>, S3Fel> {
        let (url, auth, datum) = self.signera("GET", nyckel, fråga, TOM);
        let svar = self
            .agent
            .get(url.as_str())
            .header("authorization", auth)
            .header("x-amz-date", datum)
            .header("x-amz-content-sha256", TOM)
            .call()?;
        if svar.status() == 404 && !nyckel.is_empty() {
            return Err(S3Fel::FinnsInte(nyckel.to_string()));
        }
        self.kontrollera("GET", nyckel, svar)
    }

    fn kontrollera(
        &self,
        metod: &'static str,
        nyckel: &str,
        svar: ureq::http::Response<ureq::Body>,
    ) -> Result<ureq::http::Response<ureq::Body>, S3Fel> {
        if svar.status().is_success() {
            return Ok(svar);
        }
        let status = svar.status().as_u16();
        let mut text = String::new();
        let _ = svar
            .into_body()
            .into_reader()
            .take(2000)
            .read_to_string(&mut text);
        let text = element(&text, "Message")
            .into_iter()
            .next()
            .or_else(|| element(&text, "Code").into_iter().next())
            .unwrap_or(text);
        Err(S3Fel::Svar {
            metod,
            nyckel: nyckel.to_string(),
            status,
            text,
        })
    }

    /// Adressen, `Authorization` och `x-amz-date` för en begäran nu.
    fn signera(
        &self,
        metod: &str,
        nyckel: &str,
        fråga: &[(&str, String)],
        sha256: &str,
    ) -> (Url, String, String) {
        let mut url = self.mål.endpoint.clone();
        let bas = url.path().trim_end_matches('/').to_string();
        let mut väg = format!("{bas}/{}", koda(&self.mål.bucket, false));
        if !nyckel.is_empty() {
            write!(väg, "/{}", koda(nyckel, false)).unwrap();
        }
        url.set_path(&väg);
        let frågetext = kanonisk_fråga(fråga);
        url.set_query((!frågetext.is_empty()).then_some(frågetext.as_str()));
        let datum = self.klocka.nu().strftime("%Y%m%dT%H%M%SZ").to_string();
        let auth = signatur(&Begäran {
            metod,
            url: &url,
            rubriker: &[],
            sha256,
            datum: &datum,
            region: &self.mål.region,
            nyckel_id: &self.mål.access_key_id,
            hemlighet: self.mål.secret_access_key.expose(),
        });
        (url, auth, datum)
    }
}

/// Det som signeras: metod, adress och värd, extra rubriker (utöver `host`,
/// `x-amz-content-sha256` och `x-amz-date`), kroppens hash och tidpunkten.
struct Begäran<'a> {
    metod: &'a str,
    url: &'a Url,
    rubriker: &'a [(&'a str, &'a str)],
    sha256: &'a str,
    /// `x-amz-date`, `ÅÅÅÅMMDDTHHMMSSZ`.
    datum: &'a str,
    region: &'a str,
    nyckel_id: &'a str,
    hemlighet: &'a str,
}

/// `Authorization`-rubriken enligt AWS Signature Version 4 för tjänsten `s3`.
fn signatur(b: &Begäran) -> String {
    let värd = match b.url.port() {
        Some(port) => format!("{}:{port}", b.url.host_str().unwrap_or_default()),
        None => b.url.host_str().unwrap_or_default().to_string(),
    };
    let mut rubriker: Vec<(String, String)> = vec![
        ("host".into(), värd),
        ("x-amz-content-sha256".into(), b.sha256.into()),
        ("x-amz-date".into(), b.datum.into()),
    ];
    rubriker.extend(
        b.rubriker
            .iter()
            .map(|(k, v)| (k.to_ascii_lowercase(), v.trim().to_string())),
    );
    rubriker.sort();
    let signerade = rubriker
        .iter()
        .map(|(k, _)| k.as_str())
        .collect::<Vec<_>>()
        .join(";");
    let mut kanonisk = format!(
        "{}\n{}\n{}\n",
        b.metod,
        b.url.path(),
        b.url.query().unwrap_or("")
    );
    for (k, v) in &rubriker {
        writeln!(kanonisk, "{k}:{v}").unwrap();
    }
    write!(kanonisk, "\n{signerade}\n{}", b.sha256).unwrap();

    let dag = &b.datum[..8];
    let omfång = format!("{dag}/{}/s3/aws4_request", b.region);
    let att_signera = format!(
        "AWS4-HMAC-SHA256\n{}\n{omfång}\n{}",
        b.datum,
        hex::encode(Sha256::digest(kanonisk.as_bytes()))
    );
    let mut nyckel = hmac(format!("AWS4{}", b.hemlighet).as_bytes(), dag.as_bytes());
    for del in [b.region, "s3", "aws4_request"] {
        nyckel = hmac(&nyckel, del.as_bytes());
    }
    let signatur = hex::encode(hmac(&nyckel, att_signera.as_bytes()));
    format!(
        "AWS4-HMAC-SHA256 Credential={}/{omfång}, SignedHeaders={signerade}, Signature={signatur}",
        b.nyckel_id
    )
}

/// HMAC-SHA256 (RFC 2104).
fn hmac(nyckel: &[u8], data: &[u8]) -> [u8; 32] {
    let mut block = [0u8; 64];
    if nyckel.len() > 64 {
        block[..32].copy_from_slice(&Sha256::digest(nyckel));
    } else {
        block[..nyckel.len()].copy_from_slice(nyckel);
    }
    let mut inre = Sha256::new();
    inre.update(block.map(|b| b ^ 0x36));
    inre.update(data);
    let mut yttre = Sha256::new();
    yttre.update(block.map(|b| b ^ 0x5c));
    yttre.update(inre.finalize());
    yttre.finalize().into()
}

/// URI-kodning enligt SigV4: allt utom `A-Z a-z 0-9 - _ . ~` (och `/` i en sökväg).
fn koda(text: &str, snedstreck: bool) -> String {
    let mut ut = String::new();
    for b in text.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                ut.push(b as char)
            }
            b'/' if !snedstreck => ut.push('/'),
            _ => write!(ut, "%{b:02X}").unwrap(),
        }
    }
    ut
}

fn kanonisk_fråga(fråga: &[(&str, String)]) -> String {
    let mut par: Vec<(String, String)> = fråga
        .iter()
        .map(|(k, v)| (koda(k, true), koda(v, true)))
        .collect();
    par.sort();
    par.iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("&")
}

/// Texten i varje `<namn>…</namn>` i `xml`, med XML-entiteterna avkodade.
fn element(xml: &str, namn: &str) -> Vec<String> {
    let (start, slut) = (format!("<{namn}>"), format!("</{namn}>"));
    let mut värden = Vec::new();
    let mut rest = xml;
    while let Some(i) = rest.find(&start) {
        rest = &rest[i + start.len()..];
        let Some(j) = rest.find(&slut) else { break };
        värden.push(
            rest[..j]
                .replace("&lt;", "<")
                .replace("&gt;", ">")
                .replace("&quot;", "\"")
                .replace("&apos;", "'")
                .replace("&amp;", "&"),
        );
        rest = &rest[j + slut.len()..];
    }
    värden
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hmac_stämmer_med_rfc_4231() {
        // RFC 4231, testfall 2.
        assert_eq!(
            hex::encode(hmac(b"Jefe", b"what do ya want for nothing?")),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }

    const NYCKEL_ID: &str = "AKIAIOSFODNN7EXAMPLE";
    const HEMLIGHET: &str = "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY";

    #[test]
    fn signaturen_stämmer_med_aws_exemplet_get_object() {
        // AWS-dokumentationens exempel "GET Object" för Signature Version 4 (S3).
        let url = Url::parse("https://examplebucket.s3.amazonaws.com/test.txt").unwrap();
        let auth = signatur(&Begäran {
            metod: "GET",
            url: &url,
            rubriker: &[("Range", "bytes=0-9")],
            sha256: TOM,
            datum: "20130524T000000Z",
            region: "us-east-1",
            nyckel_id: NYCKEL_ID,
            hemlighet: HEMLIGHET,
        });
        assert_eq!(
            auth,
            "AWS4-HMAC-SHA256 Credential=AKIAIOSFODNN7EXAMPLE/20130524/us-east-1/s3/aws4_request, \
             SignedHeaders=host;range;x-amz-content-sha256;x-amz-date, \
             Signature=f0e8bdb87c964420e857bd35b5d6ed310bd44f0170aba48dd91039c6036bdb41"
        );
    }

    #[test]
    fn signaturen_stämmer_med_aws_exemplet_list_objects() {
        // AWS-dokumentationens exempel "GET Bucket (List Objects)".
        let mut url = Url::parse("https://examplebucket.s3.amazonaws.com/").unwrap();
        url.set_query(Some(&kanonisk_fråga(&[
            ("prefix", "J".to_string()),
            ("max-keys", "2".to_string()),
        ])));
        let auth = signatur(&Begäran {
            metod: "GET",
            url: &url,
            rubriker: &[],
            sha256: TOM,
            datum: "20130524T000000Z",
            region: "us-east-1",
            nyckel_id: NYCKEL_ID,
            hemlighet: HEMLIGHET,
        });
        assert!(
            auth.ends_with(
                "Signature=34b48302e7b5fa45bde8084f4b7868a86f0a534bc59db6670ed5711ef69dc6f7"
            ),
            "{auth}"
        );
    }

    #[test]
    fn listans_nycklar_läses_ur_xml() {
        let xml = "<ListBucketResult><IsTruncated>false</IsTruncated>\
                   <Contents><Key>a&amp;b.db</Key></Contents><Contents><Key>c.json</Key></Contents>\
                   </ListBucketResult>";
        assert_eq!(element(xml, "Key"), ["a&b.db", "c.json"]);
        assert_eq!(element(xml, "IsTruncated"), ["false"]);
    }
}
