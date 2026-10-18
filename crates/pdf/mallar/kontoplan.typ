// Kontoplanen (S1): alla konton per kontoklass, med status och INK2-rad.

#import "mall.typ": *

#let k = data.dokument

#show: sida.with(titel: "Kontoplan")

#företag(k.foretag)

#text(size: 20pt, weight: "bold")[Kontoplan]
#v(-0.6em)
#text(size: 10pt)[Bygger på #k.bas_version · #k.antal konton]

#table(
  columns: (auto, 1fr, auto, 1.1fr),
  align: (left, left, left, left),
  table.header([Konto], [Namn], [Status], [INK2-rad]),
  table.hline(stroke: 0.5pt),
  ..k.klasser.map(klass => (
    table.cell(colspan: 4, inset: (top: 8pt, bottom: 3pt, x: 4pt))[*#klass.rubrik*],
    ..klass.konton.map(konto => (
      konto.nummer,
      konto.namn,
      if konto.aktiv [Aktiv] else [Inaktiv],
      text(size: 8.5pt)[#konto.ink2],
    )).flatten(),
  )).flatten(),
)
