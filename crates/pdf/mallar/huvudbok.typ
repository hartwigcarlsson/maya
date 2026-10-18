// Huvudboken (L4): per konto ingående balans, transaktionerna i registreringsordning med
// löpande saldo och utgående balans.

#import "mall.typ": *

#let hb = data.dokument

#show: sida.with(titel: "Huvudbok")

#företag(hb.foretag)

#text(size: 20pt, weight: "bold")[Huvudbok]
#v(-0.6em)
#text(size: 10pt)[Räkenskapsår #hb.urval.ar · #hb.urval.perioder · #hb.konton_urval]

#if hb.konton.len() == 0 [
  Inga konton med saldo eller transaktioner i urvalet.
]

#for konto in hb.konton [
  == #konto.rubrik

  #table(
    columns: (auto, auto, 1fr, auto, auto, auto),
    align: (left, left, left, right, right, right),
    table.header([Datum], [Ver], [Text], [Debet], [Kredit], [Saldo]),
    table.hline(stroke: 0.5pt),
    [], [], [#hb.ingaende], [], [], [#konto.ingaende],
    ..konto.transaktioner.map(t => (t.datum, t.nummer, t.text, t.debet, t.kredit, t.saldo)).flatten(),
    table.hline(stroke: 0.5pt),
    [], [], [*Utgående balans*], [], [], [*#konto.utgaende*],
  )
]
