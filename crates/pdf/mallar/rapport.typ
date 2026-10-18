// Resultat- eller balansräkningen (A2), med nivån och kolumnerna som visades. Raderna är redan
// uppställda och beloppen formaterade; mallen sätter bara stilen efter radens slag.

#import "mall.typ": *

#let r = data.dokument
#let antal = r.kolumner.len()

#show: sida.with(titel: r.titel)

#företag(r.foretag)

#grid(
  columns: (1fr, auto),
  align: (left + bottom, right + bottom),
  text(size: 20pt, weight: "bold")[#r.titel],
  text(size: 9pt)[Räkenskapsår #r.ar \ Period #r.period],
)

#for a in r.anmarkningar [
  #text(size: 8.5pt, style: "italic")[#a] \
]

#let grå = luma(100)

// En rad i tabellen: texten och ett värde per kolumn, stilade efter slaget.
#let rad(x) = {
  let fet = x.slag in ("sida", "rubrik", "underrubrik", "summa", "delsumma", "slutsumma")
  let kursiv = x.slag in ("underrubrik", "arets-resultat")
  let storlek = if x.slag in ("konto", "kontroll") { 8.5pt } else { 9.5pt }
  let färg = if x.slag in ("konto", "kontroll") { grå } else { black }
  let indrag = if x.slag == "rad" or x.slag == "arets-resultat" { 1em } else if x.slag == "konto" { 2em } else { 0em }
  let stil(innehåll) = text(
    size: storlek,
    fill: färg,
    weight: if fet { "bold" } else { "regular" },
    style: if kursiv { "italic" } else { "normal" },
    innehåll,
  )
  let celler = (pad(left: indrag, stil(if x.slag == "sida" { upper(x.text) } else { x.text })),)
  if x.varden.len() == 0 {
    celler += range(antal).map(_ => [])
  } else {
    celler += x.varden.zip(r.kolumner).map(((v, k)) => {
      if k.foregaende { text(size: storlek, fill: grå, v) } else { stil(v) }
    })
  }
  let linje = if x.slag in ("summa", "delsumma") {
    table.hline(stroke: 0.4pt + luma(160))
  } else if x.slag in ("slutsumma", "kontroll") {
    table.hline(stroke: 0.6pt)
  } else { none }
  let ut = ()
  if linje != none { ut.push(linje) }
  if x.slag == "sida" { ut.push(table.cell(colspan: antal + 1, inset: (top: 10pt, bottom: 2pt, x: 4pt), stil(upper(x.text)))) } else { ut += celler }
  ut
}

#table(
  columns: (1fr,) + range(antal).map(_ => auto),
  align: (left,) + range(antal).map(_ => right),
  table.header(
    [],
    ..r.kolumner.map(k => text(size: 8.5pt, fill: if k.foregaende { grå } else { black })[#k.rubrik \ #text(weight: "regular")[#k.underrubrik]]),
  ),
  table.hline(stroke: 0.5pt),
  ..r.rader.map(rad).flatten(),
)
