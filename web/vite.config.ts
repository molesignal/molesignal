import path from 'node:path';

import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite';

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
    // Prefer TypeScript source over any sibling `.js` left behind by a prior
    // `tsc -b` run; Vite's default order tries `.js` first which silently
    // shadows live `.tsx` edits.
    extensions: ['.mts', '.ts', '.tsx', '.mjs', '.js', '.jsx', '.json'],
  },
  define: {
    __BUILD_HASH__: JSON.stringify(process.env.BUILD_HASH ?? 'dev'),
    __BUILD_TIME__: JSON.stringify(new Date().toISOString()),
  },
  server: {
    port: 5173,
    proxy: {
      '/api': {
        target: 'http://localhost:5080',
        changeOrigin: true,
      },
      '^/s/': {
        target: 'http://localhost:5080',
        changeOrigin: true,
      },
    },
    // The dev server's fsevents watcher OOMs if it recurses into heavy or
    // constantly-churning trees — the 230 GB Rust `target/`, the backend's
    // `./data/wal` + `./data/objects` (rewritten every flush), build/test
    // output, and node_modules. Exclude them explicitly so the watcher only
    // tracks app source. (Run `pnpm -C web dev`, never bare `vite` from the
    // repo root, or the watch root becomes the whole monorepo.)
    watch: {
      ignored: [
        '**/node_modules/**',
        '**/.git/**',
        '**/dist/**',
        '**/test-results/**',
        '**/playwright/**',
        '**/coverage/**',
        '**/target/**',
        '**/data/**',
        '**/*.tsbuildinfo',
      ],
    },
  },
  build: {
    outDir: 'dist',
    sourcemap: true,
    target: 'es2022',
    rollupOptions: {
      output: {
        manualChunks: {
          react: ['react', 'react-dom', 'react-router-dom'],
          uplot: ['uplot'],
          reactflow: ['reactflow'],
          d3: ['d3-scale', 'd3-array', 'd3-force'],
          virtual: ['@tanstack/react-virtual'],
          // Phase 6 M3: Monaco is the largest single dep (~2 MB).
          // Splitting it keeps /home / /alerts / /settings off the
          // first-paint cost; Logs/Metrics/Functions/Pipelines lazy-load
          // via the shell/CodeEditor wrapper.
          monaco: ['monaco-editor'],
        },
      },
    },
  },
});
