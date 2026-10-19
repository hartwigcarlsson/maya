// Kundfakturan, svensk och fast, med momslagens fakturauppgifter (17 kap. 24 §). Den har ingen
// utskriftssidfot: den är kundens exemplar. Sidfoten har säljarens uppgifter i stället.

#import "mall.typ": data, mono

#let f = data.dokument
#let s = f.saljare
#let k = f.kund
// En kreditfaktura (#140) har rubriken Kreditfaktura och en hänvisning till originalet.
#let rubrik = if f.kredit != none { "Kreditfaktura" } else { "Faktura" }

#set document(title: rubrik + " " + f.nummer, date: auto)
#set text(font: "Libertinus Serif", lang: "sv", region: "se", size: 10pt)
#set page(
  paper: "a4",
  margin: (x: 2cm, top: 2cm, bottom: 3cm),
  background: if f.utkast {
    rotate(-35deg, text(size: 90pt, fill: luma(225), weight: "bold")[UTKAST])
  },
  footer: context {
    set text(size: 7.5pt, fill: luma(70))
    line(length: 100%, stroke: 0.4pt + luma(160))
    v(-0.4em)
    grid(
      columns: (1fr, 1fr, 1fr),
      column-gutter: 1em,
      [
        *#s.namn* \
        #s.adress.join(linebreak())
      ],
      [
        Org.nr #s.orgnr \
        Styrelsens säte #s.sate \
        #if s.momsregnr != "" [Momsreg.nr #s.momsregnr \ ]
        #if s.f_skatt [Godkänd för F-skatt]
      ],
      align(right)[
        #for (sätt, nummer) in s.betalsatt [#sätt #nummer \ ]
        Sida #counter(page).display() av #counter(page).final().first()
      ],
    )
  },
)
#set table(stroke: none, inset: (x: 4pt, y: 4pt))

#grid(
  columns: (1fr, auto),
  align: (left + top, right + top),
  [
    #if f.logotyp != none {
      image(sys.inputs.logotyp, format: f.logotyp, height: 1.6cm)
      v(0.2em)
    }
    #text(size: 12pt, weight: "bold")[#s.namn] \
    #s.adress.join(linebreak())
  ],
  [
    #text(size: 22pt, weight: "bold")[#rubrik #f.nummer]
    #if f.kredit != none [ \ #f.kredit.text]
  ],
)

#v(1.2em)
#grid(
  columns: (1fr, auto),
  column-gutter: 2em,
  [
    #text(size: 8.5pt, fill: luma(80))[Faktureras till] \
    #text(weight: "bold")[#k.namn] \
    #k.adress.join(linebreak())
    #if k.momsregnr != "" [ \ Momsreg.nr #k.momsregnr]
  ],
  grid(
    columns: (auto, auto),
    column-gutter: 1em,
    row-gutter: 0.55em,
    [Fakturadatum], [#f.fakturadatum],
    [Förfallodag], [*#f.forfallodag*],
    [Leveransdatum], [#f.leveransdatum],
    ..if f.er_referens != "" { ([Er referens], [#f.er_referens]) } else { () },
    ..if f.ocr { ([OCR], [#text(font: mono)[#f.referens]]) } else { ([Fakturanummer], [#f.nummer]) },
  ),
)

#v(1.5em)
// Ett företag som inte är momsregistrerat har ingen momskolumn och inga momsrader.
#let rubriker = ([Beskrivning], [Antal], [À-pris])
#if f.har_rabatt { rubriker.push([Rabatt]) }
#if f.med_moms { rubriker.push([Moms]) }
#rubriker.push([Belopp])
#let kolumner = (1fr,) + (auto,) * (rubriker.len() - 1)
#table(
  columns: kolumner,
  align: (col, _) => if col == 0 { left } else { right },
  table.header(..rubriker.map(r => text(weight: "bold")[#r])),
  table.hline(stroke: 0.5pt),
  ..f.rader.map(r => {
    let celler = ([#r.beskrivning], [#r.antal], [#r.a_pris])
    if f.har_rabatt { celler.push([#r.rabatt]) }
    if f.med_moms { celler.push([#r.moms]) }
    celler + ([#r.netto],)
  }).flatten(),
  table.hline(stroke: 0.5pt),
)
#if f.med_moms [#text(size: 8.5pt, fill: luma(80))[À-pris och belopp är exklusive moms.]]

#v(0.6em)
#align(right, grid(
  columns: (auto, auto),
  column-gutter: 2em,
  row-gutter: 0.55em,
  align: (left, right),
  ..if f.med_moms { ([Summa exklusive moms], [#f.netto]) } else { ([Summa], [#f.netto]) },
  ..if f.med_moms { f.momsrader.map(m => ([#m.text], [#m.moms])).flatten() } else { () },
  grid.hline(stroke: 0.5pt),
  pad(top: 0.3em, text(size: 12pt, weight: "bold")[#if f.kredit != none [Att få tillbaka] else [Att betala]]),
  pad(top: 0.3em, text(size: 12pt, weight: "bold")[#f.total kr]),
))

// Vid omvänd skattskyldighet: texten som momslagen kräver (17 kap. 24 §).
#if f.omvand != none {
  v(1em)
  block(width: 100%, inset: 8pt, stroke: (left: 2pt + luma(60)), text(weight: "bold")[#f.omvand])
}

#v(1.5em)
#block(
  width: 100%,
  inset: 8pt,
  stroke: 0.5pt + luma(160),
  if f.kredit != none [
    *Kredit* \
    Beloppet räknas av mot faktura #f.kredit.nummer. Är fakturan redan betald betalar vi tillbaka det.
  ] else [
    *Betalning* \
    Betala senast #f.forfallodag till
    #f.saljare.betalsatt.map(((sätt, nummer)) => [#sätt #nummer]).join(", ").
    #if f.ocr [Ange OCR #text(font: mono)[#f.referens] vid betalning.] else [Ange fakturanummer #f.nummer vid betalning.]
  ],
)
