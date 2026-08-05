/**
 * Standalone Express server that mirrors the Playwright `mockBackend`
 * fixture for use by `pnpm dev:mock`. Reuses `registerRoutes` from
 * `playwright/fixtures/mockBackend` so e2e tests and mock-backed development
 * see the same canned responses.
 *
 * Boots on port 5080 (the same port `vite.config.ts` proxies `/api` to),
 * with `cors` allowing the vite dev server origin. Boots in background;
 * cleanly killable via SIGINT.
 */
import express, { type Express } from 'express';

// Pull `registerRoutes` out of the test fixture. The fixture is in TS and
// `tsx` handles it directly when this script runs through `pnpm dev:mock`.
import { registerRoutes } from '../playwright/fixtures/mockBackend';

const PORT = Number(process.env.MOCK_PORT ?? 5080);

function applyCors(app: Express): void {
  // Permissive: dev-only; the SPA passes a normal mock Bearer token.
  app.use((_req, res, next) => {
    res.setHeader('access-control-allow-origin', '*');
    res.setHeader('access-control-allow-headers', 'authorization,content-type');
    res.setHeader('access-control-allow-methods', 'GET,POST,PUT,DELETE,OPTIONS');
    next();
  });
  app.options('*', (_req, res) => {
    res.status(204).end();
  });
}

function main(): void {
  const app = express();
  app.use(express.json({ limit: '8mb' }));
  applyCors(app);
  registerRoutes(app);

  const server = app.listen(PORT, '127.0.0.1', () => {
    console.log(`[dev-mock] listening on http://127.0.0.1:${PORT}`);
    console.log('[dev-mock] vite proxy "/api" → this server');
  });

  const shutdown = (sig: string): void => {
    console.log(`\n[dev-mock] ${sig} received, shutting down`);
    server.close(() => process.exit(0));
    // Hard cap if connections linger.
    setTimeout(() => process.exit(0), 1500).unref();
  };
  process.on('SIGINT', () => shutdown('SIGINT'));
  process.on('SIGTERM', () => shutdown('SIGTERM'));
}

main();
