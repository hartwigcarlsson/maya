// Behandlingshistoriken (A2): ett urval av företagets poster med tid, aktör, typ, objekt och
// ändringen, i den ordning de skrevs.

#import "mall.typ": *

#let h = data.dokument

#show: sida.with(titel: "Behandlingshistorik")

#företag(h.foretag)

#text(size: 20pt, weight: "bold")[Behandlingshistorik]
#v(-0.6em)
#text(size: 10pt)[Urval: #h.urval · #h.antal]

#if h.poster.len() == 0 [
  Inga poster i urvalet.
] else {
  table(
    columns: (auto, auto, auto, auto, 1fr),
    align: (left, left, left, left, left),
    table.header([Tid], [Aktör], [Typ], [Objekt], [Ändring]),
    table.hline(stroke: 0.5pt),
    ..h.poster.map(p => (
      text(size: 8.5pt, font: mono)[#p.tid],
      p.aktor,
      p.typ,
      p.objekt,
      p.beskrivning,
    )).flatten(),
  )
}
