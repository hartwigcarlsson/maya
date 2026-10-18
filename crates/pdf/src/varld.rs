//! Typsts `World` för de inbyggda mallarna: inga filer utöver mallarna, inga paket, bara de
//! inbäddade typsnitten.

use std::sync::LazyLock;

use jiff::Timestamp;
use typst::diag::{FileError, FileResult, SourceDiagnostic};
use typst::foundations::{Bytes, Datetime, Dict, Duration, Value};
use typst::syntax::{FileId, RootedPath, Source, VirtualPath, VirtualRoot};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Library, LibraryExt, World};
use typst_layout::PagedDocument;
use typst_pdf::{PdfOptions, PdfStandard, PdfStandards, Timestamp as PdfTid};

use crate::PdfFel;

/// Mallarna, inbäddade i binären. `mall.typ` är den gemensamma sidan med sidfoten.
const MALLAR: &[(&str, &str)] = &[
    ("mall.typ", include_str!("../mallar/mall.typ")),
    ("verifikat.typ", include_str!("../mallar/verifikat.typ")),
    ("kontoplan.typ", include_str!("../mallar/kontoplan.typ")),
    (
        "behandlingshistorik.typ",
        include_str!("../mallar/behandlingshistorik.typ"),
    ),
];

/// Typsnitten (SIL OFL respektive Bitstream Vera-licensen; båda tillåter inbäddning, som PDF/A
/// kräver). Licenserna ligger bredvid filerna.
const TYPSNITT: &[&[u8]] = &[
    include_bytes!("../fonts/LibertinusSerif-Regular.otf"),
    include_bytes!("../fonts/LibertinusSerif-Bold.otf"),
    include_bytes!("../fonts/LibertinusSerif-Italic.otf"),
    include_bytes!("../fonts/DejaVuSansMono.ttf"),
];

struct Typsnitt {
    bok: LazyHash<FontBook>,
    fonts: Vec<Font>,
}

static TYPSNITTEN: LazyLock<Typsnitt> = LazyLock::new(|| {
    let fonts: Vec<Font> = TYPSNITT
        .iter()
        .flat_map(|data| Font::iter(Bytes::new(*data)))
        .collect();
    Typsnitt {
        bok: LazyHash::new(FontBook::from_fonts(&fonts)),
        fonts,
    }
});

fn fil_id(namn: &str) -> FileId {
    RootedPath::new(
        VirtualRoot::Project,
        VirtualPath::new(format!("/{namn}")).expect("mallens namn är en giltig sökväg"),
    )
    .intern()
}

struct Värld {
    bibliotek: LazyHash<Library>,
    huvudfil: FileId,
}

impl World for Värld {
    fn library(&self) -> &LazyHash<Library> {
        &self.bibliotek
    }

    fn book(&self) -> &LazyHash<FontBook> {
        &TYPSNITTEN.bok
    }

    fn main(&self) -> FileId {
        self.huvudfil
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        MALLAR
            .iter()
            .find(|(namn, _)| fil_id(namn) == id)
            .map(|(_, text)| Source::new(id, (*text).to_string()))
            .ok_or_else(|| FileError::NotFound(id.vpath().get_without_slash().into()))
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        self.source(id)
            .map(|s| Bytes::from_string(s.text().to_string()))
    }

    fn font(&self, index: usize) -> Option<Font> {
        TYPSNITTEN.fonts.get(index).cloned()
    }

    /// Mallarna läser aldrig dagens datum; PDF:ens datum kommer ur sidfoten.
    fn today(&self, _offset: Option<Duration>) -> Option<Datetime> {
        None
    }
}

/// Kompilerar mallen med `data` i `sys.inputs.data` och skriver PDF/A-2b med `tidpunkt` som
/// dokumentets datum.
pub(crate) fn skriv(
    mall: &str,
    data: serde_json::Value,
    tidpunkt: Timestamp,
) -> Result<Vec<u8>, PdfFel> {
    let json = serde_json::to_vec(&data).map_err(|e| PdfFel::Mall(e.to_string()))?;
    let mut inputs = Dict::new();
    inputs.insert("data".into(), Value::Bytes(Bytes::new(json)));
    let värld = Värld {
        bibliotek: LazyHash::new(Library::builder().with_inputs(inputs).build()),
        huvudfil: fil_id(mall),
    };
    let dokument: PagedDocument = typst::compile(&värld).output.map_err(fel)?;

    let lokal = tidpunkt.to_zoned(maya_domain::tid::stockholm());
    let datetime = Datetime::from_ymd_hms(
        lokal.year().into(),
        lokal.month() as u8,
        lokal.day() as u8,
        lokal.hour() as u8,
        lokal.minute() as u8,
        lokal.second() as u8,
    )
    .expect("en giltig tidpunkt");
    let offset_minuter = lokal.offset().seconds() / 60;
    let options = PdfOptions {
        timestamp: PdfTid::new_local(datetime, offset_minuter),
        standards: PdfStandards::new(&[PdfStandard::A_2b]).expect("PDF/A-2b ensam är giltig"),
        ..PdfOptions::default()
    };
    let pdf = typst_pdf::pdf(&dokument, &options).map_err(fel);
    // Mallarna kompileras om varje gång; släpp Typsts minnescache så att den inte växer.
    typst::comemo::evict(10);
    pdf
}

fn fel(diagnoser: impl IntoIterator<Item = SourceDiagnostic>) -> PdfFel {
    PdfFel::Mall(
        diagnoser
            .into_iter()
            .map(|d| d.message.to_string())
            .collect::<Vec<_>>()
            .join("; "),
    )
}
