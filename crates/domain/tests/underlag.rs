use maya_domain::underlag::{self, STÖRSTA_STORLEK, Visning};

#[test]
fn ett_underlag_är_högst_25_mb() {
    assert_eq!(STÖRSTA_STORLEK, 25 * 1024 * 1024);
}

#[test]
fn filens_början_avgör_mime_typen_för_pdf_och_bilder() {
    assert_eq!(
        underlag::mime("text/plain", b"%PDF-1.7\n..."),
        "application/pdf"
    );
    assert_eq!(underlag::mime("", b"\x89PNG\r\n\x1a\n...."), "image/png");
    assert_eq!(
        underlag::mime("application/octet-stream", b"\xff\xd8\xff\xe0...."),
        "image/jpeg"
    );
    assert_eq!(underlag::mime("", b"GIF89a...."), "image/gif");
    assert_eq!(underlag::mime("", b"RIFF\x10\0\0\0WEBPVP8 "), "image/webp");
}

#[test]
fn annars_gäller_webbläsarens_mime_typ_om_den_ser_ut_som_en() {
    assert_eq!(
        underlag::mime("Text/CSV; charset=utf-8", b"a;b\n"),
        "text/csv"
    );
    assert_eq!(underlag::mime("", b"a;b\n"), "application/octet-stream");
    assert_eq!(
        underlag::mime("inte en typ", b"a;b\n"),
        "application/octet-stream"
    );
    // En bild som påstås vara en PDF visas inte som PDF.
    assert_eq!(
        underlag::mime("application/pdf", b"hej"),
        "application/octet-stream"
    );
    assert_eq!(
        underlag::mime("image/png", b"<html>"),
        "application/octet-stream"
    );
}

#[test]
fn pdf_och_bilder_visas_och_annat_laddas_ned() {
    assert_eq!(underlag::visning("application/pdf"), Visning::Pdf);
    for bild in ["image/png", "image/jpeg", "image/gif", "image/webp"] {
        assert_eq!(underlag::visning(bild), Visning::Bild, "{bild}");
    }
    for annat in [
        "image/svg+xml",
        "text/html",
        "text/csv",
        "application/octet-stream",
    ] {
        assert_eq!(underlag::visning(annat), Visning::Nedladdning, "{annat}");
    }
}

#[test]
fn storleken_visas_i_kb_eller_mb() {
    assert_eq!(underlag::storlek(512), "512 byte");
    assert_eq!(underlag::storlek(2_048), "2 kB");
    assert_eq!(underlag::storlek(1_572_864), "1,5 MB");
    assert_eq!(underlag::storlek(STÖRSTA_STORLEK as u64), "25,0 MB");
}
