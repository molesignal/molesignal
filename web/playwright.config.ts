import { defineConfig, devices } from '@playwright/test';

export default defineConfig({
  // `./playwright` (not `./playwright/tests`) so the perf suite under
  // `./playwright/perf` is discoverable via `--grep @perf`. The default `pnpm
  // playwright` invocation excludes perf via `--grep-invert @perf` in
  // package.json.
  testDir: './playwright',
  // Only .ts spec files; the .js siblings are leftovers from `tsc -b`
  // before the build script switched to `--noEmit`.
  testMatch: /.*\.spec\.ts$/,
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 2 : 0,
  reporter: process.env.CI ? [['blob'], ['line']] : 'list',
  timeout: 30_000,
  use: {
    baseURL: 'http://localhost:5173',
    viewport: { width: 1440, height: 900 },
    locale: 'en-US',
    timezoneId: 'UTC',
    trace: 'on-first-retry',
    screenshot: 'only-on-failure',
    video: 'retain-on-failure',
  },
  webServer: {
    command: 'pnpm dev',
    url: 'http://localhost:5173',
    reuseExistingServer: !process.env.CI,
    timeout: 60_000,
  },
  projects: [
    {
      name: 'chromium',
      use: { ...devices['Desktop Chrome'], colorScheme: 'dark' },
    },
  ],
});
