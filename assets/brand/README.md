# Sakshi brand assets

The logo, its cuts, and the icon set. Every file here is generated from one geometry;
nothing is hand-traced, so a redraw must reproduce the numbers below exactly.

## The mark

A filled disc — the recording — resolving into three bars — the transcript. The disc is the
only warm element in the system and the only thing that changes state.

Three cuts, each drawn on its own content-tight field. Scale a cut whole; never mix
their numbers.

| Cut | Field | Geometry | Use |
|---|---|---|---|
| Wide | 86 × 32 | disc `cx 16 cy 16 r 16`; bars `x 42`, `y 0 / 12.5 / 25`, `w 44 / 36 / 24`, `h 7`, `rx 3.5` | Lockups, headers, anywhere 64 px and up |
| Stacked | 44 × 64 | disc `cx 16 cy 16 r 16`; bars `x 0`, `y 40 / 49 / 58`, `w 44 / 34 / 22`, `h 6`, `rx 3` | Squares: app icons, avatars, 24 px and up |
| Micro | 44 × 46 | disc `cx 15 cy 15 r 15`; bar `x 0 y 38 w 44 h 8 rx 4` | Menu bar, tray, favicon, below 24 px |

Bars are flush left and ragged right, longest to shortest. The recording is the fixed
thing; the transcript is what varies.

## Colour

| Token | Hex | Role |
|---|---|---|
| Ink | `#12151C` | Bars, wordmark, dark surfaces |
| Tally | `#E8921E` | The disc only. Never type, never behind type |
| Paper | `#F7F7F5` | The page, and the mark reversed on ink |
| Graphite | `#4A5160` | Secondary type — 7.4:1 on Paper, passes AA |
| Rule | `#E0E1DE` | Hairlines and card edges. Never carries text |

Tally is a graphic colour, not a text colour: at `#E8921E` it is 2.29:1 on Paper and fails
AA at every size. Set type in Ink or Graphite.

## Type

Wordmark and headings in **Space Grotesk Medium** (SIL OFL 1.1), tracked −0.035em on the
wordmark and −0.015em on headings. Interface and body in **Inter**, 16 px floor. The name
in Devanagari — साक्षी — in **Noto Sans Devanagari**, +0.1em, Graphite.

The wordmark files here carry outlined paths, so they need no font installed. Do not
re-set the wordmark as live text.

## Files

```
logo-horizontal.svg          primary lockup, Ink wordmark
logo-horizontal-reverse.svg  for Ink and photographic backgrounds
logo-horizontal-mono.svg     every element currentColor
logo-stacked.svg             mark above wordmark, narrow columns
logo-stacked-reverse.svg
wordmark.svg                 outlined wordmark alone
wordmark-reverse.svg
mark-{wide,stacked,micro}.svg          Tally disc, Ink bars
mark-{wide,stacked,micro}-reverse.svg  Tally disc, Paper bars
mark-{wide,stacked,micro}-mono.svg     every element currentColor
mark-micro-recording.svg               Tally disc, bar follows currentColor
icons/icon-{16,24,32,48,64,128,256,512,1024}.png   full-bleed square masters
icon-macos.iconset/                                squircle baked in, for iconutil
tauri-icons/                                       drop-in for src-tauri/icons
tray/trayTemplate{,@2x,@3x}.png                    macOS template icons, pure black
tray/tray-dark{,@2x}.png                           white, for non-template toolkits
tray/tray-recording{,@2x}.png                      Tally disc while recording
favicon.svg, favicon-{16,32}.png
social-card.{svg,png}                              1200 × 630
```

## Wiring it up

Desktop app icons:

```bash
cp assets/brand/tauri-icons/* apps/sakshi-desktop/src-tauri/icons/
```

`tauri-icons/source.png` is the 1024 px master if you would rather regenerate with
`pnpm tauri icon`.

A real `.icns` (the squircle version, which `tauri icon` does not apply) needs macOS:

```bash
iconutil -c icns assets/brand/icon-macos.iconset -o apps/sakshi-desktop/src-tauri/icons/icon.icns
```

Tray: use `tray/trayTemplate*.png` as a macOS template image so the system inverts it with
the menu bar. Swap to `tray/tray-recording*.png` while a recording is running — the disc
turning warm is the only state the icon carries.

## Rules

1. One disc, filled, never outlined and never doubled. It is the recording; there is only ever one.
2. The disc keeps its Tally on every background. In one-colour printing it goes solid with the bars, never to a tint.
3. Bars stay flush left and ragged right, longest to shortest. Centring or reordering them breaks the reading.
4. The wide cut never goes in a square. Squares take the stacked cut; anything under 24 px takes the micro cut.
5. Clear space is half the mark's height on all four sides. Nothing enters it, including the wordmark.
6. No gradient, no shadow, no outline, no rotation. The mark is flat because the product is literal.
