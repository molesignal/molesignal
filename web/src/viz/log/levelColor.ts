import type { PaletteKey } from '@/viz/timeseries/themeAdapter';
import { colorKeyForService } from '@/viz/trace/colors';

export function colorKeyForLevel(level: string | undefined): PaletteKey | undefined {
  switch ((level ?? '').toLowerCase()) {
    case 'fatal':
    case 'error':
      return '--red';
    case 'warn':
    case 'warning':
      return '--yellow';
    case 'info':
      return undefined;
    case 'debug':
      return '--blue';
    default:
      return undefined;
  }
}

export { colorKeyForService };
