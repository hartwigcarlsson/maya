//! CSV för svenska kalkylprogram, gemensam för varje CSV-export i Maya: UTF-8 med BOM,
//! semikolon som avgränsare, CRLF och decimalkomma. Ett fält med semikolon, citattecken eller
//! radbrytning sätts inom citattecken, med citattecknen dubblerade.

/// En CSV-fil under uppbyggnad: rubrikraden först, sedan en rad i taget.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Csv(String);

impl Csv {
    pub fn new(rubriker: &[&str]) -> Self {
        let mut csv = Csv("\u{feff}".into());
        csv.rad(rubriker);
        csv
    }

    pub fn rad<S: AsRef<str>>(&mut self, fält: impl IntoIterator<Item = S>) {
        for (i, f) in fält.into_iter().enumerate() {
            if i > 0 {
                self.0.push(';');
            }
            let f = f.as_ref();
            if f.contains([';', '"', '\n', '\r']) {
                self.0.push('"');
                self.0.push_str(&f.replace('"', "\"\""));
                self.0.push('"');
            } else {
                self.0.push_str(f);
            }
        }
        self.0.push_str("\r\n");
    }

    pub fn text(self) -> String {
        self.0
    }

    pub fn innehåll(self) -> Vec<u8> {
        self.0.into_bytes()
    }
}

/// Ett belopp i öre som ett kalkylprogram läser som tal: decimalkomma, inga
/// tusentalsavgränsare och minus framför, t.ex. `-1234,50`.
pub fn belopp(öre: i64) -> String {
    let tecken = if öre < 0 { "-" } else { "" };
    let öre = öre.unsigned_abs();
    format!("{tecken}{},{:02}", öre / 100, öre % 100)
}
