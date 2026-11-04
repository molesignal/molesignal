import { clsx, type ClassValue } from 'clsx';
import { extendTailwindMerge } from 'tailwind-merge';

// The product's own weight and size names (tailwind.config.ts). Unknown to
// tailwind-merge, `font-strong` passed for a font family, so `font-strong
// font-mono` lost its weight and `font-medium font-body` kept both, leaving the
// stylesheet's order to pick one; `text-kpi` passed for a colour.
const twMerge = extendTailwindMerge({
  extend: {
    classGroups: {
      'font-weight': [{ font: ['body', 'strong', 'display', 'display-strong'] }],
      'font-size': [{ text: ['body', 'chrome', 'kpi', 'noc'] }],
    },
  },
});

export function cn(...inputs: ClassValue[]): string {
  return twMerge(clsx(inputs));
}
