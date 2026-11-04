/**
 * The least width a row of a grid table takes, as CSS: each track at its
 * least (a fixed width, or the floor of a `minmax`), the gaps between them
 * and the row's own padding, in the pixels the classes that set them give
 * (`gap-3` is 12px, `px-3` 12px a side). A table set at least this wide
 * scrolls sideways as one piece, its head and rows together, instead of its
 * last columns spilling past the rows' edge, where a row drawn with
 * `content-visibility` cuts them off.
 */
export function gridMinWidth(tracks: readonly string[], gapPx = 12, paddingPx = 24): string {
  const least = tracks.reduce((sum, track) => sum + trackFloor(track), 0);
  return `${least + gapPx * Math.max(0, tracks.length - 1) + paddingPx}px`;
}

function trackFloor(track: string): number {
  const match = /^(?:minmax\(\s*)?(\d+(?:\.\d+)?)px/.exec(track.trim());
  return match ? Number(match[1]) : 0;
}
