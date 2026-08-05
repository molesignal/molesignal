import js from '@eslint/js';
import tseslint from '@typescript-eslint/eslint-plugin';
import tsparser from '@typescript-eslint/parser';
import importPlugin from 'eslint-plugin-import';
import react from 'eslint-plugin-react';
import reactHooks from 'eslint-plugin-react-hooks';

export default [
  js.configs.recommended,
  {
    files: ['src/**/*.{ts,tsx}', 'playwright/**/*.{ts,tsx}', 'scripts/**/*.ts'],
    languageOptions: {
      parser: tsparser,
      parserOptions: {
        ecmaVersion: 2022,
        sourceType: 'module',
        ecmaFeatures: { jsx: true },
        project: './tsconfig.json',
      },
      globals: {
        window: 'readonly',
        document: 'readonly',
        navigator: 'readonly',
        console: 'readonly',
        fetch: 'readonly',
        URL: 'readonly',
        URLSearchParams: 'readonly',
        crypto: 'readonly',
        performance: 'readonly',
        MutationObserver: 'readonly',
        ResizeObserver: 'readonly',
        EventSource: 'readonly',
        ReadableStream: 'readonly',
        AbortController: 'readonly',
        Worker: 'readonly',
        OffscreenCanvas: 'readonly',
        requestAnimationFrame: 'readonly',
        cancelAnimationFrame: 'readonly',
        setTimeout: 'readonly',
        clearTimeout: 'readonly',
        setInterval: 'readonly',
        clearInterval: 'readonly',
        localStorage: 'readonly',
        sessionStorage: 'readonly',
        location: 'readonly',
        __BUILD_HASH__: 'readonly',
        __BUILD_TIME__: 'readonly',
        HTMLElement: 'readonly',
        HTMLDivElement: 'readonly',
        HTMLCanvasElement: 'readonly',
        HTMLInputElement: 'readonly',
        KeyboardEvent: 'readonly',
        MouseEvent: 'readonly',
        WheelEvent: 'readonly',
        PointerEvent: 'readonly',
        Event: 'readonly',
        CustomEvent: 'readonly',
        MessageEvent: 'readonly',
        alert: 'readonly',
        atob: 'readonly',
        btoa: 'readonly',
        getComputedStyle: 'readonly',
        self: 'readonly',
        React: 'readonly',
        process: 'readonly',
      },
    },
    plugins: {
      '@typescript-eslint': tseslint,
      react,
      'react-hooks': reactHooks,
      import: importPlugin,
    },
    settings: {
      react: { version: 'detect' },
      'import/resolver': {
        typescript: true,
        node: true,
      },
    },
    rules: {
      ...tseslint.configs.recommended.rules,
      ...react.configs.recommended.rules,
      ...reactHooks.configs.recommended.rules,
      'react/react-in-jsx-scope': 'off',
      'react/prop-types': 'off',
      '@typescript-eslint/consistent-type-imports': ['error', { prefer: 'type-imports' }],
      '@typescript-eslint/no-unused-vars': [
        'error',
        { argsIgnorePattern: '^_', varsIgnorePattern: '^_' },
      ],
      'import/order': [
        'error',
        {
          groups: ['builtin', 'external', 'internal', ['parent', 'sibling', 'index']],
          'newlines-between': 'always',
          alphabetize: { order: 'asc', caseInsensitive: true },
        },
      ],
      // Hard ban: deprecated UI libs that previously lived here.
      'no-restricted-imports': [
        'error',
        {
          patterns: [
            {
              group: ['@mantine/*', 'echarts', 'echarts-for-react', '@tabler/icons-react'],
              message: 'Removed in web-investigation-shell — use shadcn/ui + uPlot/canvas/ReactFlow instead.',
            },
            // Feature modules must not import Radix directly — go through shadcn wrappers.
            {
              group: ['@radix-ui/*'],
              message: 'Import shadcn-wrapped components from @/shell/ui/* instead.',
            },
          ],
        },
      ],
    },
  },
  // Exception: shell/ui itself IS the shadcn wrapper layer — Radix imports allowed here.
  // shell/FormDrawer is a shell-level composition of Radix Dialog primitives that the
  // shadcn dialog wrapper does not cover, so Radix is allowed there too.
  {
    files: ['src/shell/ui/**/*.{ts,tsx}', 'src/shell/FormDrawer.tsx'],
    rules: {
      'no-restricted-imports': 'off',
    },
  },
  // Tests / scripts loosened
  {
    files: ['**/*.test.{ts,tsx}', 'playwright/**/*.ts', 'scripts/**/*.ts'],
    rules: {
      '@typescript-eslint/no-explicit-any': 'off',
      'import/order': 'off',
      // Playwright fixture idiom: `async ({}, use) => ...` requires an empty
      // destructure when the fixture has no dependencies, and `use` is the
      // fixture lifecycle callback (not a React hook).
      'no-empty-pattern': 'off',
      'react-hooks/rules-of-hooks': 'off',
    },
  },
  {
    // Emitted JS / d.ts left over from prior `tsc -b` runs should never be
    // linted as if they were source. Source-of-truth is .ts/.tsx; `vite build`
    // handles real bundling, and `tsc -b --noEmit` handles type checking.
    ignores: [
      'dist/**',
      'node_modules/**',
      'coverage/**',
      'playwright-report/**',
      'test-results/**',
      'src/**/*.js',
      'src/**/*.js.map',
      'src/**/*.d.ts',
      'playwright/**/*.js',
      'playwright/**/*.js.map',
      'playwright/**/*.d.ts',
      'scripts/**/*.js',
      'scripts/**/*.js.map',
      'scripts/**/*.d.ts',
      'vite.config.js',
      'vite.config.js.map',
      'vite.config.d.ts',
      'vitest.config.js',
      'vitest.config.js.map',
      'vitest.config.d.ts',
    ],
  },
];
