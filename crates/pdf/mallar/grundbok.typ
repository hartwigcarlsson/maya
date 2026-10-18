// Grundboken (L4): de bokförda verifikaten per period i registreringsordning, med datum,
// nummer, text, rader och summa.

#import "mall.typ": *

#let g = data.dokument

#show: sida.with(titel: "Grundbok")

#företag(g.foretag)

#text(size: 20pt, weight: "bold")[Grundbok]
#v(-0.6em)
#text(size: 10pt)[Räkenskapsår #g.urval.ar · #g.urval.perioder · bokförda verifikat i registreringsordning]

#for period in g.perioder [
  == #period.namn

  #if period.verifikat.len() == 0 [
    #text(size: 9pt, fill: luma(80))[Inga bokförda verifikat.]
  ] else {
    table(
      columns: (auto, 1fr, 1fr, auto, auto),
      align: (left, left, left, right, right),
      table.header([Konto], [Kontonamn], [Text], [Debet], [Kredit]),
      table.hline(stroke: 0.5pt),
      ..period.verifikat.map(ver => (
        table.cell(colspan: 5, inset: (top: 7pt, bottom: 2pt, x: 4pt))[
          *#ver.nummer* · #ver.datum · #ver.beskrivning
          #h(1fr)
          *#ver.summa*
          \ #text(size: 8pt, fill: luma(80))[Bokfört #ver.bokford]
        ],
        ..ver.rader.map(r => (r.konto, r.kontonamn, r.text, r.debet, r.kredit)).flatten(),
      )).flatten(),
    )
  }
]
