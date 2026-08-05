import path from 'node:path';

import { defineConfig } from 'vitest/config';

export default defineConfig({
  resolve: {
    alias: { '@': path.resolve(__dirname, './src') },
    // Same as vite.config.ts — prefer TypeScript source over stale `.js`
    // sibling emit artifacts.
    extensions: ['.mts', '.ts', '.tsx', '.mjs', '.js', '.jsx', '.json'],
  },
  test: {
    environment: 'jsdom',
    globals: false,
    setupFiles: ['./vitest.setup.ts'],
    // Playwright specs live under playwright/ and use @playwright/test, which
    // is not compatible with vitest's runner. Keep them out of the unit run.
    exclude: ['node_modules/**', 'dist/**', 'playwright/**'],
    coverage: {
      provider: 'v8',
      reporter: ['text', 'lcov'],
      thresholds: { lines: 80, branches: 75, statements: 80 },
      exclude: [
        'src/main.tsx',
        'src/shell/ui/**', // shadcn vendored components
        'src/viz/_demo/**',
      ],
    },
  },
});
