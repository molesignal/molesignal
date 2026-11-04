import type { Config } from 'tailwindcss';
import animate from 'tailwindcss-animate';

type TokenColorOptions = {
  opacityValue?: string;
};

/**
 * Tailwind's `/opacity` modifiers need a color function when the source is a
 * CSS variable containing a hex or rgba value. Returning `var(--token)`
 * directly makes utilities such as `border-indigo/30` invalid, so browsers
 * fall back to Tailwind's light default border even in dark mode.
 */
const tokenColor = (token: string) =>
  ({ opacityValue }: TokenColorOptions) => {
    if (opacityValue === undefined) return `var(${token})`;
    const numericOpacity = Number(opacityValue);
    const percentage = Number.isFinite(numericOpacity)
      ? `${numericOpacity * 100}%`
      : `calc(${opacityValue} * 100%)`;
    return `color-mix(in srgb, var(${token}) ${percentage}, transparent)`;
  };

/**
 * The root is 14px, the body size, so Tailwind's rem steps stood 3.5px apart and fell between
 * pixels (`h-9` was 31.5px, `gap-2.5` 8.75px). The scale is set in pixels instead: each step on
 * the nearest 4px to where it stood (2px apart under 8px), so boxes land on whole pixels and on
 * one grid, and the density stays as it was.
 */
const GRID_SPACING = {
  px: '1px',
  0: '0px',
  0.5: '2px',
  1: '4px',
  1.5: '6px',
  2: '8px',
  2.5: '8px',
  3: '12px',
  3.5: '12px',
  4: '16px',
  5: '16px',
  6: '20px',
  7: '24px',
  8: '28px',
  9: '32px',
  10: '36px',
  11: '40px',
  12: '44px',
  14: '48px',
  16: '56px',
  20: '72px',
  24: '84px',
  28: '96px',
  32: '112px',
  36: '128px',
  40: '140px',
  44: '152px',
  48: '168px',
  52: '184px',
  56: '196px',
  60: '208px',
  64: '224px',
  72: '252px',
  80: '280px',
  96: '336px',
};

/** Line heights on whole pixels too, at the even step nearest to where the rem ones stood. */
const GRID_LINE_HEIGHT = {
  3: '12px',
  4: '16px',
  5: '18px',
  6: '20px',
  7: '24px',
  8: '28px',
  9: '32px',
  10: '36px',
  // `relaxed` keeps its 1.625 and ends on the whole pixel nearest to it at the size it is set
  // at (12px → 20px, 13px → 21px, 14px → 23px). Being a length, it is inherited as one: text of
  // another size inside it takes the same height unless it sets its own.
  relaxed: 'round(1.625em, 1px)',
};

const config: Config = {
  darkMode: ['class', '[data-theme="dark"]'],
  // Unit tests are not content. The patterns leave them out themselves,
  // `!(*.test)` by file name and `!(__tests__)` by the directory a file sits in
  // (hence one pattern for `src/` itself and one for everything below it). A
  // `!` exclusion would not do: Tailwind honours it, but Vite expands the
  // positive patterns on its own, keeps every test file as a dependency of the
  // stylesheet, and reloads all open pages whenever one is saved.
  content: [
    './index.html',
    './src/!(*.test).{ts,tsx}',
    './src/**/!(__tests__)/!(*.test).{ts,tsx}',
  ],
  theme: {
    spacing: GRID_SPACING,
    extend: {
      lineHeight: GRID_LINE_HEIGHT,
      colors: {
        /* layer 1 — design-handoff terminal-dark scale */
        'bg-0': tokenColor('--bg-0'),
        'bg-1': tokenColor('--bg-1'),
        'bg-2': tokenColor('--bg-2'),
        'bg-3': tokenColor('--bg-3'),
        'bg-4': tokenColor('--bg-4'),
        'bg-hover': tokenColor('--bg-hover'),

        'bd-0': tokenColor('--bd-0'),
        'bd-1': tokenColor('--bd-1'),
        'bd-2': tokenColor('--bd-2'),

        'tx-0': tokenColor('--tx-0'),
        'tx-1': tokenColor('--tx-1'),
        'tx-2': tokenColor('--tx-2'),
        'tx-3': tokenColor('--tx-3'),
        'tx-4': tokenColor('--tx-4'),

        /* Indigo is the brand color. */
        indigo: {
          DEFAULT: tokenColor('--indigo'),
          soft: tokenColor('--indigo-soft'),
          dim: tokenColor('--indigo-dim'),
        },
        orange: {
          DEFAULT: tokenColor('--orange'),
          soft: tokenColor('--orange-soft'),
          dim: tokenColor('--orange-dim'),
        },
        purple: {
          DEFAULT: tokenColor('--purple'),
          soft: tokenColor('--purple-soft'),
          dim: tokenColor('--purple-dim'),
        },

        /* Data viz series — 8-color cycle, OKLCH-equalized */
        chart: {
          1: 'var(--chart-1)',
          2: 'var(--chart-2)',
          3: 'var(--chart-3)',
          4: 'var(--chart-4)',
          5: 'var(--chart-5)',
          6: 'var(--chart-6)',
          7: 'var(--chart-7)',
          8: 'var(--chart-8)',
        },

        /* layer 2 — legacy 9-color (now aliasing layer 1 via tokens.css) */
        bg: tokenColor('--bg'),
        surface: tokenColor('--surface'),
        'surface-muted': tokenColor('--surface-muted'),
        primary: {
          DEFAULT: tokenColor('--primary'),
          bg: tokenColor('--primary-bg'),
          muted: tokenColor('--primary-muted'),
        },
        accent: {
          DEFAULT: tokenColor('--accent'),
          bg: tokenColor('--accent-bg'),
          muted: tokenColor('--accent-muted'),
        },
        red: {
          DEFAULT: tokenColor('--red'),
          bg: tokenColor('--red-bg'),
          muted: tokenColor('--red-muted'),
          soft: tokenColor('--red-soft'),
          dim: tokenColor('--red-dim'),
        },
        green: {
          DEFAULT: tokenColor('--green'),
          bg: tokenColor('--green-bg'),
          muted: tokenColor('--green-muted'),
          soft: tokenColor('--green-soft'),
          dim: tokenColor('--green-dim'),
        },
        yellow: {
          DEFAULT: tokenColor('--yellow'),
          bg: tokenColor('--yellow-bg'),
          muted: tokenColor('--yellow-muted'),
          soft: tokenColor('--yellow-soft'),
          dim: tokenColor('--yellow-dim'),
        },
        blue: {
          DEFAULT: tokenColor('--blue'),
          bg: tokenColor('--blue-bg'),
          muted: tokenColor('--blue-muted'),
          soft: tokenColor('--blue-soft'),
          dim: tokenColor('--blue-dim'),
        },

        /* shadcn token aliases — internal use within shell/ui only */
        background: tokenColor('--bg'),
        foreground: tokenColor('--fg'),
        card: tokenColor('--surface'),
        'card-foreground': tokenColor('--fg'),
        popover: tokenColor('--surface'),
        'popover-foreground': tokenColor('--fg'),
        muted: tokenColor('--surface-muted'),
        'muted-foreground': tokenColor('--fg-muted'),
        border: tokenColor('--border'),
        input: tokenColor('--border'),
        ring: tokenColor('--accent'),
        destructive: {
          DEFAULT: tokenColor('--red'),
          foreground: tokenColor('--red-fg'),
        },
        'primary-foreground': tokenColor('--primary-fg'),
        'accent-foreground': tokenColor('--accent-fg'),
        secondary: tokenColor('--surface-muted'),
        'secondary-foreground': tokenColor('--fg'),

        /* Modal/drawer scrim. Use `bg-overlay` / `bg-overlay-soft` rather
         * than `bg-black/N` so a future palette swap (or a colorblind
         * palette) doesn't fight the cascade. */
        overlay: tokenColor('--overlay'),
        'overlay-soft': tokenColor('--overlay-soft'),
      },
      fontFamily: {
        sans: ['var(--font-sans)'],
        /* Code/data utilities intentionally share the global product family. */
        mono: ['var(--font-mono)'],
        /* Editors and query expressions use the dedicated code stack. */
        code: ['var(--font-code)'],
      },
      /* The type scale in tokens.css: running text 14 / 22, dense data 13 / 20,
       * supporting text 12 / 18. Line heights are those pixel steps as ratios,
       * so they follow a size that a token changes. */
      fontSize: {
        body: ['var(--font-body, 14px)', { lineHeight: 'calc(22 / 14)' }],
        chrome: ['var(--font-label, 13px)', { lineHeight: '1.5385' }],
        xs: ['var(--font-caption, 12px)', { lineHeight: '1.5' }],
        sm: ['var(--font-label, 13px)', { lineHeight: '1.5385' }],
        md: ['var(--font-body, 14px)', { lineHeight: 'calc(22 / 14)' }],
        lg: ['15px', { lineHeight: '1.4667' }],
        xl: ['var(--font-heading, 18px)', { lineHeight: 'calc(26 / 18)' }],
        '2xl': ['var(--font-page-title, 20px)', { lineHeight: '1.4' }],
        kpi: ['var(--font-kpi, 30px)', { lineHeight: '1.2667' }],
        noc: ['56px', { lineHeight: '1.0' }],
      },
      /* The weights are the type scale in tokens.css (`--fw-*`), so the whole
       * product moves together when the scale does. Tailwind's own names land
       * on its steps: `font-medium` and `font-semibold` are the strong one
       * (navigation, buttons, table heads, labels), `font-bold` the title one. */
      fontWeight: {
        body: 'var(--fw-body)',
        strong: 'var(--fw-strong)',
        display: 'var(--fw-display)',
        'display-strong': 'var(--fw-display-strong)',
        medium: 'var(--fw-strong)',
        semibold: 'var(--fw-strong)',
        bold: 'var(--fw-display-strong)',
      },
      spacing: {
        topbar: 'var(--topbar-h)',
        sidebar: 'var(--sidebar-w)',
        'sidebar-collapsed': 'var(--sidebar-w-collapsed)',
        'sidebar-item': 'var(--sidebar-item-h)',
        subsidebar: 'var(--subsidebar-w)',
        row: 'var(--row-height)',
        'row-pad-x': 'var(--row-pad-x)',
        'row-pad-y': 'var(--row-pad-y)',
        /* legacy compatibility */
        rail: '52px',
        strip: '32px',
        drawer: '720px',
      },
      borderRadius: {
        lg: '12px',
        md: '8px',
        sm: '6px',
        full: '9999px',
      },
      boxShadow: {
        /* Shadow tokens are theme-aware via CSS variables. */
        sm: 'var(--shadow-sm)',
        md: 'var(--shadow-md)',
        lg: 'var(--shadow-lg)',
        drawer: 'var(--shadow-drawer)',
        popup: 'var(--shadow-popup)',
        login: 'var(--shadow-login)',
      },
      transitionDuration: {
        instant: 'var(--duration-instant)',
        fast: 'var(--duration-fast)',
        normal: 'var(--duration-normal)',
        slow: 'var(--duration-slow)',
        slower: 'var(--duration-slower)',
      },
      transitionTimingFunction: {
        'ease-default': 'var(--easing-default)',
        'ease-in-default': 'var(--easing-in)',
        'ease-out-default': 'var(--easing-out)',
        'expo-out': 'var(--easing-expo-out)',
        spring: 'var(--easing-spring)',
      },
      keyframes: {
        'fade-in': { from: { opacity: '0' }, to: { opacity: '1' } },
        'slide-in-right': {
          from: { transform: 'translateX(100%)' },
          to: { transform: 'translateX(0)' },
        },
        'slide-out-right': {
          from: { transform: 'translateX(0)' },
          to: { transform: 'translateX(100%)' },
        },
        // Floating surfaces grow out of their trigger (transform-origin comes
        // from Radix) and leave a little faster than they arrive, both easing
        // out: a leave that starts slowly reads as lag.
        'pop-in': {
          from: { opacity: '0', transform: 'scale(0.96)' },
          to: { opacity: '1', transform: 'scale(1)' },
        },
        'pop-out': {
          from: { opacity: '1', transform: 'scale(1)' },
          to: { opacity: '0', transform: 'scale(0.97)' },
        },
        // Centered dialogs carry their -50% offsets, otherwise the animation's
        // transform would replace the centering utilities for its duration.
        'dialog-in': {
          from: { opacity: '0', transform: 'translate(-50%, -48%) scale(0.97)' },
          to: { opacity: '1', transform: 'translate(-50%, -50%) scale(1)' },
        },
        'dialog-out': {
          from: { opacity: '1', transform: 'translate(-50%, -50%) scale(1)' },
          to: { opacity: '0', transform: 'translate(-50%, -49%) scale(0.98)' },
        },
        'fade-out': { from: { opacity: '1' }, to: { opacity: '0' } },
      },
      animation: {
        'fade-in': 'fade-in var(--duration-fast) var(--easing-out)',
        // Drawer animations use the normal duration and ease-out motion tokens.
        'slide-in-right': 'slide-in-right var(--duration-normal) var(--easing-out)',
        'slide-out-right': 'slide-out-right var(--duration-normal) var(--easing-out) forwards',
        'pop-in': 'pop-in 160ms var(--easing-expo-out)',
        'pop-out': 'pop-out 100ms var(--easing-out) forwards',
        'dialog-in': 'dialog-in var(--duration-enter) var(--easing-expo-out)',
        'dialog-out': 'dialog-out 120ms var(--easing-out) forwards',
        'fade-out': 'fade-out var(--duration-fast) var(--easing-out) forwards',
      },
    },
  },
  plugins: [animate],
};

export default config;
