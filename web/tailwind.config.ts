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

const config: Config = {
  darkMode: ['class', '[data-theme="dark"]'],
  content: ['./index.html', './src/**/*.{ts,tsx}'],
  theme: {
    extend: {
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

        /* Phase 4: Indigo is the brand color */
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
      fontSize: {
        body: 'var(--font-body, 13.5px)',
        chrome: '12.5px',
        xs: ['var(--font-caption, 12px)', { lineHeight: '1.45' }],
        sm: ['var(--font-label, 12.5px)', { lineHeight: '1.5' }],
        md: ['13.5px', { lineHeight: '1.5' }],
        lg: ['15px', { lineHeight: '1.45' }],
        xl: ['17px', { lineHeight: '1.35' }],
        '2xl': ['var(--font-page-title, 22px)', { lineHeight: '1.2' }],
        // Review calibration: keep the refreshed hierarchy while reducing display scale.
        kpi: ['var(--font-kpi, 32px)', { lineHeight: '1.05' }],
        noc: ['56px', { lineHeight: '1.0' }],
      },
      fontWeight: {
        body: '500',
        strong: '600',
        display: '600',
        'display-strong': '700',
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
        /* Phase 4: shadow tokens are theme-aware via CSS vars */
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
      },
      animation: {
        'fade-in': 'fade-in var(--duration-fast) var(--easing-out)',
        // Drawer slide is 200ms ease-out per Phase 4 motion tokens.
        'slide-in-right': 'slide-in-right var(--duration-normal) var(--easing-out)',
        'slide-out-right': 'slide-out-right var(--duration-normal) var(--easing-out) forwards',
      },
    },
  },
  plugins: [animate],
};

export default config;
