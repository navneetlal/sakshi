// The Sakshi mark, copied from assets/brand/mark-{wide,stacked,micro}.svg.
// The bars take the current text colour, so the mark follows the palette
// and light or dark; the disc is always Tally. Pick the cut for the size
// (assets/brand/README.md): wide for 64 px and up, stacked for squares
// 24 px and up, micro below 24 px.

export type Cut = "wide" | "stacked" | "micro";

const FIELD: Record<Cut, [number, number]> = {
  wide: [86, 32],
  stacked: [44, 64],
  micro: [44, 46],
};

// SVG presentation attributes can't read CSS variables; a style can.
const disc = { fill: "var(--tally)" };

function Shapes({ cut }: { cut: Cut }) {
  switch (cut) {
    case "wide":
      return (
        <>
          <circle cx="16" cy="16" r="16" style={disc} />
          <rect x="42" y="0" width="44" height="7" rx="3.5" fill="currentColor" />
          <rect x="42" y="12.5" width="36" height="7" rx="3.5" fill="currentColor" />
          <rect x="42" y="25" width="24" height="7" rx="3.5" fill="currentColor" />
        </>
      );
    case "stacked":
      return (
        <>
          <circle cx="16" cy="16" r="16" style={disc} />
          <rect x="0" y="40" width="44" height="6" rx="3" fill="currentColor" />
          <rect x="0" y="49" width="34" height="6" rx="3" fill="currentColor" />
          <rect x="0" y="58" width="22" height="6" rx="3" fill="currentColor" />
        </>
      );
    case "micro":
      return (
        <>
          <circle cx="15" cy="15" r="15" style={disc} />
          <rect x="0" y="38" width="44" height="8" rx="4" fill="currentColor" />
        </>
      );
  }
}

/** The mark at `height` px. With a `label` it's an image; without, decoration. */
export function Mark({ cut, height, label }: { cut: Cut; height: number; label?: string }) {
  const [w, h] = FIELD[cut];
  const width = (height * w) / h;
  return (
    <svg
      viewBox={`0 0 ${w} ${h}`}
      width={width}
      height={height}
      // Inline, so surrounding rules for icon SVGs can't resize the mark or
      // outline it (the mark is never outlined). Selecting it would tint it.
      style={{ width, height, flexShrink: 0, stroke: "none", userSelect: "none" }}
      role={label ? "img" : undefined}
      aria-label={label}
      aria-hidden={label ? undefined : true}
    >
      <Shapes cut={cut} />
    </svg>
  );
}
