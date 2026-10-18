// Systemdokumentationen (S1–S6): avsnitten med sina block, som de genereras i core. Mallen
// ritar bara blocken; all text kommer ur datat.

#import "mall.typ": *

#let s = data.dokument

#show: sida.with(titel: "Systemdokumentation")
#show heading.where(level: 1): set text(size: 14pt)
#show heading.where(level: 1): it => { pagebreak(weak: true); it }

// Text med fetstil och kursiv: en lista av delar.
#let tx(delar) = if delar.len() == 0 { [] } else {
  delar.map(d => if d.stil == "fet" { strong(d.text) } else if d.stil == "kursiv" { emph(d.text) } else { d.text }).join()
}

#let rita(b) = {
  if b.typ == "underrubrik" {
    heading(level: 2, b.text)
  } else if b.typ == "stycke" {
    par(tx(b.text))
  } else if b.typ == "lista" {
    list(..b.punkter.map(tx))
  } else if b.typ == "fakta" {
    show table.cell.where(y: 0): set text(weight: "regular")
    table(
      columns: (auto, 1fr),
      ..b.rader.map(r => (strong(r.rubrik), tx(r.varde))).flatten(),
    )
  } else if b.typ == "tabell" {
    let n = b.kolumner.len()
    set text(size: 8.5pt)
    table(
      columns: (auto,) * n,
      table.header(..b.kolumner.map(k => [#k])),
      table.hline(stroke: 0.5pt),
      ..b.rader.map(r => if "rubrik" in r {
        (table.cell(colspan: n, inset: (top: 6pt, bottom: 3pt, x: 4pt))[*#r.rubrik*],)
      } else {
        r.celler.map(tx)
      }).flatten(),
    )
  } else if b.typ == "lank" {
    block(above: 0.4em, below: 0.4em, text(size: 8.5pt)[→ #b.text: #text(font: mono)[#b.href]])
  }
}

#företag(s.foretag)

#text(size: 20pt, weight: "bold")[Systemdokumentation]
#v(-0.6em)
#text(size: 10pt)[Maya #s.version · INK2-tabell #s.ink2 · #s.bas]

#outline(title: [Innehåll], depth: 1)

#for a in s.avsnitt {
  heading(level: 1, a.rubrik)
  text(size: 8.5pt, fill: luma(90))[Krav: #a.krav]
  for b in a.block {
    rita(b)
  }
}
