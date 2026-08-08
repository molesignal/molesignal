import type { Config } from 'tailwindcss';
import animate from 'tailwindcss-animate';

const config: Config = {
  darkMode: ['class', '[data-theme="dark"]'],
  content: ['./index.html', './src/**/*.{ts,tsx}'],
  theme: {
    extend: {
      colors: {
        /* layer 1 — design-handoff terminal-dark scale */
        'bg-0': 'var(--bg-0)',
        'bg-1': 'var(--bg-1)',
        'bg-2': 'var(--bg-2)',
        'bg-3': 'var(--bg-3)',
        'bg-4': 'var(--bg-4)',
        'bg-hover': 'var(--bg-hover)',

        'bd-0': 'var(--bd-0)',
        'bd-1': 'var(--bd-1)',
        'bd-2': 'var(--bd-2)',

        'tx-0': 'var(--tx-0)',
        'tx-1': 'var(--tx-1)',
        'tx-2': 'var(--tx-2)',
        'tx-3': 'var(--tx-3)',
        'tx-4': 'var(--tx-4)',

        /* Phase 4: Indigo is the brand color */
        indigo: {
          DEFAULT: 'var(--indigo)',
          soft: 'var(--indigo-soft)',
          dim: 'var(--indigo-dim)',
        },
        orange: {
          DEFAULT: 'var(--orange)',
          soft: 'var(--orange-soft)',
          dim: 'var(--orange-dim)',
        },
        purple: {
          DEFAULT: 'var(--purple)',
          soft: 'var(--purple-soft)',
          dim: 'var(--purple-dim)',
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
        bg: 'var(--bg)',
        surface: 'var(--surface)',
        'surface-muted': 'var(--surface-muted)',
        primary: {
          DEFAULT: 'var(--primary)',
          bg: 'var(--primary-bg)',
          muted: 'var(--primary-muted)',
        },
        accent: {
          DEFAULT: 'var(--accent)',
          bg: 'var(--accent-bg)',
          muted: 'var(--accent-muted)',
        },
        red: {
          DEFAULT: 'var(--red)',
          bg: 'var(--red-bg)',
          muted: 'var(--red-muted)',
          soft: 'var(--red-soft)',
          dim: 'var(--red-dim)',
        },
        green: {
          DEFAULT: 'var(--green)',
          bg: 'var(--green-bg)',
          muted: 'var(--green-muted)',
          soft: 'var(--green-soft)',
          dim: 'var(--green-dim)',
        },
        yellow: {
          DEFAULT: 'var(--yellow)',
          bg: 'var(--yellow-bg)',
          muted: 'var(--yellow-muted)',
          soft: 'var(--yellow-soft)',
          dim: 'var(--yellow-dim)',
        },
        blue: {
          DEFAULT: 'var(--blue)',
          bg: 'var(--blue-bg)',
          muted: 'var(--blue-muted)',
          soft: 'var(--blue-soft)',
          dim: 'var(--blue-dim)',
        },

        /* shadcn token aliases — internal use within shell/ui only */
        background: 'var(--bg)',
        foreground: 'var(--fg)',
        card: 'var(--surface)',
        'card-foreground': 'var(--fg)',
        popover: 'var(--surface)',
        'popover-foreground': 'var(--fg)',
        muted: 'var(--surface-muted)',
        'muted-foreground': 'var(--fg-muted)',
        border: 'var(--border)',
        input: 'var(--border)',
        ring: 'var(--accent)',
        destructive: {
          DEFAULT: 'var(--red)',
          foreground: 'var(--red-fg)',
        },
        'primary-foreground': 'var(--primary-fg)',
        'accent-foreground': 'var(--accent-fg)',
        secondary: 'var(--surface-muted)',
        'secondary-foreground': 'var(--fg)',

        /* Modal/drawer scrim. Use `bg-overlay` / `bg-overlay-soft` rather
         * than `bg-black/N` so a future palette swap (or a colorblind
         * palette) doesn't fight the cascade. */
        overlay: 'var(--overlay)',
        'overlay-soft': 'var(--overlay-soft)',
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
