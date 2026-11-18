import { ORG_LOGO_MAX_BYTES } from '@/api/orgs';

/** The side of the square a logo is drawn into before it is kept: twice the largest it is shown. */
export const LOGO_SIDE = 128;

/** The image types a logo can be chosen from; whatever it is, it is kept as a small PNG or WEBP. */
export const LOGO_ACCEPT = 'image/png,image/jpeg,image/webp,image/svg+xml';

export class LogoImageError extends Error {
  constructor(readonly reason: 'unreadable' | 'too_large') {
    super(reason);
  }
}

/** How many bytes a base64 `data:` URL holds once decoded. */
export function dataUrlBytes(dataUrl: string): number {
  const data = dataUrl.slice(dataUrl.indexOf(',') + 1);
  const padding = data.endsWith('==') ? 2 : data.endsWith('=') ? 1 : 0;
  return Math.floor((data.length * 3) / 4) - padding;
}

function loadImage(url: string): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const image = document.createElement('img');
    image.onload = () => resolve(image);
    image.onerror = () => reject(new LogoImageError('unreadable'));
    image.src = url;
  });
}

/**
 * The chosen file drawn whole into a transparent square of {@link LOGO_SIDE} pixels, as a `data:`
 * URL small enough to keep: PNG, or WEBP when the PNG of a busy image is too large.
 */
export async function toLogoDataUrl(file: File): Promise<string> {
  const url = URL.createObjectURL(file);

  try {
    const image = await loadImage(url);
    // An SVG without a size of its own reports none: it is drawn to fill the square.
    const width = image.naturalWidth || LOGO_SIDE;
    const height = image.naturalHeight || LOGO_SIDE;
    const scale = Math.min(LOGO_SIDE / width, LOGO_SIDE / height);
    const canvas = document.createElement('canvas');
    canvas.width = LOGO_SIDE;
    canvas.height = LOGO_SIDE;
    const context = canvas.getContext('2d');
    if (!context) throw new LogoImageError('unreadable');

    context.drawImage(
      image,
      (LOGO_SIDE - width * scale) / 2,
      (LOGO_SIDE - height * scale) / 2,
      width * scale,
      height * scale,
    );

    for (const candidate of [canvas.toDataURL('image/png'), canvas.toDataURL('image/webp', 0.9)]) {
      if (dataUrlBytes(candidate) <= ORG_LOGO_MAX_BYTES) return candidate;
    }
    throw new LogoImageError('too_large');
  } finally {
    URL.revokeObjectURL(url);
  }
}
