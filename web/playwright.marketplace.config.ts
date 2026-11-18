import { defineConfig } from '@playwright/test';
import base from './playwright.config';
export default defineConfig({ ...base, webServer: { command: 'node node_modules/vite/bin/vite.js --host 127.0.0.1 --port 5191 --strictPort', url: 'http://127.0.0.1:5191', reuseExistingServer: false }, use: { ...base.use, baseURL: 'http://127.0.0.1:5191' } });
