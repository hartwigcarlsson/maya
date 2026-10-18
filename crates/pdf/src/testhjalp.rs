//! Läser tillbaka Mayas PDF:er i tester (feature `test-support`): PDF/A-2b-märkningen och
//! texten per sida.

use hayro_cmap::BfString;
use hayro_interpret::font::Glyph;
use hayro_interpret::{
    BlendMode, ClipPath, Context, Device, GlyphDrawMode, Image, InterpreterCache,
    InterpreterSettings, Paint, PathDrawMode, SoftMask, interpret_page,
};
use hayro_syntax::Pdf;
use kurbo::{Affine, BezPath, Rect};

/// Texten på varje sida, utan blanksteg: glyferna bär inga mellanrum, så jämför med
/// [`innehåller`].
pub fn sidor(pdf: &[u8]) -> Vec<String> {
    läs(pdf).into_iter().map(|t| t.0).collect()
}

/// Antalet bilder som ritas på sidorna, t.ex. en logotyp.
pub fn bilder(pdf: &[u8]) -> usize {
    läs(pdf).iter().map(|t| t.1).sum()
}

fn läs(pdf: &[u8]) -> Vec<Text> {
    let pdf = Pdf::new(pdf.to_vec()).expect("PDF:en går att läsa");
    let cache = InterpreterCache::new();
    pdf.pages()
        .iter()
        .map(|sida| {
            let mut context = Context::new(
                Affine::IDENTITY,
                Rect::new(0.0, 0.0, 1.0, 1.0),
                &cache,
                pdf.xref(),
                InterpreterSettings::default(),
            );
            let mut text = Text(String::new(), 0);
            interpret_page(sida, &mut context, &mut text);
            text
        })
        .collect()
}

/// Om texten ur [`sidor`] innehåller `text`, blanksteg oräknade.
pub fn innehåller(sida: &str, text: &str) -> bool {
    sida.contains(&utan_blanksteg(text))
}

/// Panikar om PDF:en inte är märkt PDF/A-2b eller har ett typsnitt som inte är inbäddat. Typst
/// vägrar själv att skriva en PDF som bryter mot standarden (krillas validator); här syns att
/// standarden faktiskt valdes. Ingen veraPDF körs.
pub fn är_pdfa_2b(pdf: &[u8]) {
    let s = String::from_utf8_lossy(pdf);
    assert!(s.starts_with("%PDF-1.7"), "PDF 1.7, som PDF/A-2 bygger på");
    assert!(s.contains("<pdfaid:part>2</pdfaid:part>"), "PDF/A del 2");
    assert!(
        s.contains("<pdfaid:conformance>B</pdfaid:conformance>"),
        "nivå B"
    );
    assert!(
        s.contains("/OutputIntents"),
        "PDF/A kräver en output intent"
    );
    assert!(
        s.contains("/FontFile2") || s.contains("/FontFile3"),
        "typsnitten är inbäddade"
    );
}

fn utan_blanksteg(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

/// Texten och antalet bilder på en sida.
struct Text(String, usize);

impl Device<'_> for Text {
    fn set_soft_mask(&mut self, _: Option<SoftMask<'_>>) {}
    fn set_blend_mode(&mut self, _: BlendMode) {}
    fn draw_path(&mut self, _: &BezPath, _: Affine, _: &Paint<'_>, _: &PathDrawMode) {}
    fn push_clip_path(&mut self, _: &ClipPath) {}
    fn push_transparency_group(&mut self, _: f32, _: Option<SoftMask<'_>>, _: BlendMode) {}
    fn draw_glyph(
        &mut self,
        g: &Glyph<'_>,
        _: Affine,
        _: Affine,
        _: &Paint<'_>,
        _: &GlyphDrawMode,
    ) {
        match g.as_unicode() {
            Some(BfString::Char(c)) if !c.is_whitespace() => self.0.push(c),
            Some(BfString::String(s)) => self.0.push_str(&utan_blanksteg(&s)),
            _ => {}
        }
    }
    fn draw_image(&mut self, _: Image<'_, '_>, _: Affine) {
        self.1 += 1;
    }
    fn pop_clip_path(&mut self) {}
    fn pop_transparency_group(&mut self) {}
}
