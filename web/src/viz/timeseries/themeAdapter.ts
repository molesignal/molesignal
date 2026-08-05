import * as React from 'react';

/**
 * Resolve the current values of the 9-color CSS vars to concrete hex/rgba
 * strings for uPlot's command-line drawing config. Re-read whenever the theme
 * attribute on <body> changes.
 */
const PALETTE_KEYS = [
  '--bg',
  '--surface',
  '--surface-muted',
  '--fg',
  '--fg-muted',
  '--border',
  '--primary',
  '--accent',
  '--red',
  '--green',
  '--yellow',
  '--blue',
  '--purple',
] as const;

export type PaletteKey = (typeof PALETTE_KEYS)[number];

export type Palette = Record<PaletteKey, string>;

export function readPalette(): Palette {
  if (typeof globalThis === 'undefined' || typeof document === 'undefined') {
    return Object.fromEntries(PALETTE_KEYS.map((k) => [k, '#000'])) as Palette;
  }
  const css = getComputedStyle(document.documentElement);
  const out = {} as Palette;
  for (const k of PALETTE_KEYS) {
    out[k] = css.getPropertyValue(k).trim() || '#888';
  }
  return out;
}

/**
 * Series stroke palette derived from the 9 semantic colors (excluding bg /
 * surface variants). uPlot picks colors round-robin from this list.
 */
export const SERIES_PALETTE_KEYS: PaletteKey[] = [
  '--accent',
  '--blue',
  '--green',
  '--yellow',
  '--red',
  '--purple',
  '--primary',
];

/**
 * Subscribe to body[data-theme] mutations. Returns the current palette and a
 * version number that bumps each time the theme attribute changes — drive a
 * `useEffect` off that to call `plot.redraw()`.
 */
export function useThemePalette(): { palette: Palette; version: number } {
  const [version, setVersion] = React.useState(0);
  const [palette, setPalette] = React.useState<Palette>(readPalette);

  React.useEffect(() => {
    const body = document.body;
    const refresh = () => {
      setPalette(readPalette());
      setVersion((v) => v + 1);
    };
    const obs = new MutationObserver((entries) => {
      for (const m of entries) {
        if (m.attributeName === 'data-theme') {
          refresh();
        }
      }
    });
    obs.observe(body, { attributes: true });
    return () => obs.disconnect();
  }, []);

  return { palette, version };
}
