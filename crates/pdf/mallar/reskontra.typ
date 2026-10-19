// Reskontran per datum (R4, A2): öppna poster per faktura med det som återstår och det som
// står på reskontrakontot, och avstämningen mot 1510 och 2440 med differensen.

#import "mall.typ": *

#let r = data.dokument

#show: sida.with(titel: "Reskontran per " + r.dag)

#företag(r.foretag)

#text(size: 20pt, weight: "bold")[Reskontran per #r.dag]
#v(-0.6em)
#text(size: 10pt)[Öppna poster vid dagens utgång och avstämning mot huvudboken]

#for d in r.delar [
  == #d.rubrik

  #if d.poster.len() == 0 [
    #text(size: 9pt, fill: luma(80))[Inga öppna poster.]
  ] else {
    table(
      columns: (auto, 1fr, auto, auto, auto, auto, auto, auto),
      align: (left, left, left, left, right, right, right, left),
      table.header(
        [Faktura], [Motpart], [Fakturadatum], [Förfaller], [Total], [Återstår], [På kontot], [Läge],
      ),
      table.hline(stroke: 0.5pt),
      ..d.poster.map(p => (
        p.fakturanummer, p.motpart, p.fakturadatum, p.forfallodag, p.total, p.aterstar, p.bokfort,
        p.lage,
      )).flatten(),
    )
  }

  #table(
    columns: (1fr, auto),
    align: (left, right),
    stroke: none,
    [Summa återstår], [#d.aterstar],
    [Reskontrans belopp på #d.konto], [#d.bokfort],
    [Saldo på #d.konto i huvudboken], [#d.saldo],
    table.hline(stroke: 0.5pt),
    [*Differens*], [*#d.differens*],
  )
  #if d.stammer [
    #text(size: 9pt)[Reskontran stämmer med huvudboken.]
  ] else [
    #text(size: 9pt, weight: "bold")[Reskontran avviker från huvudboken med #d.differens kr.]
  ]
]
