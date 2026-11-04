import type { ScrollBand } from '@/api/rum';

import { heatColor, type Spot } from './model';

/** How wide the heat of one click is, in pixels of the drawing. */
const RADIUS = 28;

/** The strongest a spot's darkness is drawn; the colours are read off what the spots add up to. */
const STRENGTH = 0.5;

/**
 * Draws the spots as heat: each adds to the darkness of what is around it, and the darkness is then read
 * as a colour, so that where clicks gather the map turns hot whatever the number of them.
 */
export function drawClicks(canvas: HTMLCanvasElement, spots: readonly Spot[], scale: number): void {
  const context = canvas.getContext('2d');

  if (!context) return;

  context.clearRect(0, 0, canvas.width, canvas.height);

  const hottest = spots.reduce((most, spot) => Math.max(most, spot.clicks), 0);

  if (hottest === 0) return;

  for (const spot of spots) {
    const x = spot.x * scale;
    const y = spot.y * scale;
    const glow = context.createRadialGradient(x, y, 0, x, y, RADIUS);

    glow.addColorStop(0, `rgba(0,0,0,${STRENGTH * (0.25 + 0.75 * (spot.clicks / hottest))})`);
    glow.addColorStop(1, 'rgba(0,0,0,0)');
    context.fillStyle = glow;
    context.fillRect(x - RADIUS, y - RADIUS, RADIUS * 2, RADIUS * 2);
  }

  const image = context.getImageData(0, 0, canvas.width, canvas.height);
  const { data } = image;

  for (let index = 0; index < data.length; index += 4) {
    const darkness = data[index + 3] ?? 0;

    if (darkness === 0) continue;

    const [red, green, blue, alpha] = heatColor(darkness / 255);

    data[index] = red;
    data[index + 1] = green;
    data[index + 2] = blue;
    data[index + 3] = alpha;
  }

  context.putImageData(image, 0, 0);
}

/**
 * Draws how far views got as bands across the page: the part most views saw is left clear and the part
 * few saw is shaded more and more, so the fold people stop at can be read off the page.
 */
export function drawScroll(canvas: HTMLCanvasElement, bands: readonly ScrollBand[], scale: number): void {
  const context = canvas.getContext('2d');

  if (!context) return;

  context.clearRect(0, 0, canvas.width, canvas.height);

  bands.forEach((band, index) => {
    const top = band.depth * scale;
    const next = bands[index + 1];
    const bottom = next ? next.depth * scale : canvas.height;

    context.fillStyle = `rgba(0, 0, 0, ${0.62 * (1 - band.reached)})`;
    context.fillRect(0, top, canvas.width, bottom - top);
  });

  const last = bands[bands.length - 1];

  if (last) {
    const after = (last.depth + 100) * scale;

    context.fillStyle = 'rgba(0, 0, 0, 0.62)';
    context.fillRect(0, after, canvas.width, Math.max(0, canvas.height - after));
  }
}
