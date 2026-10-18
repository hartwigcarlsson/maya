// Gemensam sida för Mayas PDF:er. Datat kommer bara via `sys.inputs.data` (JSON) och sätts in
// som text, så det tolkas aldrig som markup.

#let data = json(sys.inputs.data)
#let sidfot = data.sidfot
#let mono = "DejaVu Sans Mono"

#let sida(titel: none, body) = {
  set document(title: titel, date: auto)
  set text(font: "Libertinus Serif", lang: "sv", region: "se", size: 10pt)
  set page(
    paper: "a4",
    margin: (x: 2cm, top: 2cm, bottom: 2.4cm),
    footer: context {
      set text(size: 7.5pt, fill: luma(80))
      line(length: 100%, stroke: 0.4pt + luma(160))
      v(-0.4em)
      [Utskrivet #sidfot.tidpunkt av #sidfot.anvandare · Maya #sidfot.version · INK2-tabell #sidfot.ink2]
      h(1fr)
      [Sida #counter(page).display() av #counter(page).final().first()]
    },
  )
  set table(stroke: none, inset: (x: 4pt, y: 3pt))
  show table.cell.where(y: 0): set text(weight: "bold")
  show heading: set text(size: 11pt)
  body
}

// Företagets namn och organisationsnummer överst.
#let företag(f) = text(size: 9pt, fill: luma(60))[#f.namn · #f.orgnr]

// Vem och när: "2026-03-14 10:05 av Anna via webb".
#let signatur(s) = [#s.tid av #s.namn via #s.kanal]
