// Ett enskilt verifikat (A2): rubrik, nummer, metadata, rader, underlagsförteckning och historik.

#import "mall.typ": *

#let ver = data.dokument
#let bokfört = ver.nummer != none

#show: sida.with(titel: if bokfört { "Verifikat " + ver.nummer } else { "Verifikatutkast" })

#företag(ver.foretag)

#grid(
  columns: (1fr, auto),
  align: (left + bottom, right + bottom),
  [
    #text(size: 20pt, weight: "bold")[#if bokfört { ver.nummer } else { "Utkast" }]
    #v(-0.6em)
    #text(size: 13pt)[#ver.beskrivning]
  ],
  [
    #text(size: 9pt)[#if bokfört { "Bokfört" } else { "Utkast" }] \
    #text(size: 13pt, weight: "bold")[#ver.summa kr]
  ],
)

#v(0.4em)
#grid(
  columns: (auto, 1fr),
  column-gutter: 1em,
  row-gutter: 0.5em,
  [*Verifikatdatum*],
  [#if ver.datum != none { ver.datum } else { "–" }#if ver.period != none [ · period #ver.period]],
  ..if ver.bokford != none { ([*Bokfört*], signatur(ver.bokford)) } else { () },
  [*Skapat*], signatur(ver.skapad),
)

== Rader

#table(
  columns: (auto, 1fr, 1fr, auto, auto),
  align: (left, left, left, right, right),
  table.header([Konto], [Kontonamn], [Text], [Debet], [Kredit]),
  table.hline(stroke: 0.5pt),
  ..ver.rader.map(r => (r.konto, r.kontonamn, r.text, r.debet, r.kredit)).flatten(),
  table.hline(stroke: 0.5pt),
  table.cell(colspan: 3)[*Summa*], [*#ver.debet*], [*#ver.kredit*],
)

== Underlag

#if ver.underlag.len() == 0 [
  Inga underlag.
] else {
  table(
    columns: (1fr, auto),
    align: (left, right),
    table.header([Fil och SHA-256], [Storlek]),
    table.hline(stroke: 0.5pt),
    ..ver.underlag.map(u => (
      [
        #u.filnamn \
        #text(font: mono, size: 7pt)[#u.sha256]
        #if u.komplettering != none [
          \ #text(size: 8.5pt, style: "italic")[Komplettering #u.komplettering.tid av #u.komplettering.namn via #u.komplettering.kanal]
        ]
      ],
      u.storlek,
    )).flatten(),
  )
}

== Historik

#if ver.historik.len() == 0 [
  Ingen historik.
] else {
  table(
    columns: (auto, 1fr, auto),
    table.header([Tid], [Händelse], [Av]),
    table.hline(stroke: 0.5pt),
    ..ver.historik.map(h => (h.tid, h.typ, [#h.namn via #h.kanal])).flatten(),
  )
}
