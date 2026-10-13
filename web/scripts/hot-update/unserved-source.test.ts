// @vitest-environment node
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

import type { Plugin as PostcssPlugin } from 'postcss';
import { createServer, type HotPayload, type ViteDevServer } from 'vite';
import { afterAll, beforeAll, describe, expect, it, vi } from 'vitest';

import { unservedSourceUpdate } from './unserved-source';

/**
 * A small app on a real dev server. Its stylesheet depends on every file under
 * `src/` and on a config file next to it, the way Tailwind reports its
 * `content` files and its config. A page load is the requests a browser makes;
 * a save is the event the file watcher emits. What is asserted is the message
 * the server sends to the open pages.
 */
const FILES: Record<string, string> = {
  'theme.config.ts': 'export default {};\n',
  'src/style.css': 'body { color: red; }\n',
  'src/main.ts': [
    "import './style.css';",
    "import { label } from './label';",
    "import './panel';",
    "export const openLazy = () => import('./lazy');",
    "export const search = () => new Worker(new URL('./worker.ts', import.meta.url), { type: 'module' });",
    'console.log(label);',
    '',
  ].join('\n'),
  'src/label.ts': "export const label = 'label';\n",
  'src/panel.ts': 'if (import.meta.hot) import.meta.hot.accept();\nexport const panel = 1;\n',
  'src/types.ts': 'export type Label = string;\n',
  'src/lazy.ts': 'export const lazy = 1;\n',
  'src/worker.ts': 'self.onmessage = () => {};\n',
  'src/broken.ts': 'export const = ;\n',
  'src/incomplete.ts': "import './missing';\n",
};

const PAGE_LOAD = ['/src/main.ts', '/src/style.css', '/src/label.ts', '/src/panel.ts'];

const stylesheetScans = (root: string): PostcssPlugin => ({
  postcssPlugin: 'stylesheet-scans',
  Once(_css, { result }) {
    result.messages.push(
      { type: 'dir-dependency', plugin: 'stylesheet-scans', dir: path.join(root, 'src'), glob: '**/*.ts' },
      { type: 'dependency', plugin: 'stylesheet-scans', file: path.join(root, 'theme.config.ts') },
    );
  },
});

const describePayload = (payload: HotPayload) =>
  payload.type === 'update'
    ? `update ${payload.updates.map((update) => update.path).sort().join(' ')}`
    : payload.type;

describe('unservedSourceUpdate', { timeout: 30_000 }, () => {
  let root: string;
  let server: ViteDevServer;

  const request = (url: string) => server.environments.client.transformRequest(url);

  /** The messages the open pages get when the watcher reports the file. */
  const sentAfter = async (event: 'add' | 'change' | 'unlink', file: string) => {
    const sent: string[] = [];
    const send = vi
      .spyOn(server.environments.client.hot, 'send')
      .mockImplementation((payload: HotPayload | string) => {
        sent.push(typeof payload === 'string' ? payload : describePayload(payload));
      });
    try {
      server.watcher.emit(event, path.join(root, file));
      await vi.waitFor(() => expect(sent).not.toHaveLength(0), { timeout: 20_000 });
      return sent;
    } finally {
      send.mockRestore();
    }
  };
  const save = (file: string) => sentAfter('change', file);
  const remove = (file: string) => sentAfter('unlink', file);

  beforeAll(async () => {
    // Vite files modules under their real path; the temp directory is a symlink on macOS.
    root = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'unserved-source-')));
    for (const [file, content] of Object.entries(FILES)) {
      fs.mkdirSync(path.dirname(path.join(root, file)), { recursive: true });
      fs.writeFileSync(path.join(root, file), content);
    }

    server = await createServer({
      root,
      configFile: false,
      logLevel: 'silent',
      appType: 'custom',
      cacheDir: path.join(root, '.vite'),
      server: { middlewareMode: true, ws: false, watch: null },
      optimizeDeps: { noDiscovery: true, include: [] },
      css: { postcss: { plugins: [stylesheetScans(root)] } },
      plugins: [unservedSourceUpdate(path.join(root, 'src'))],
    });
    for (const url of PAGE_LOAD) await request(url);
  }, 60_000);

  afterAll(async () => {
    await server.close();
    fs.rmSync(root, { recursive: true, force: true });
  });

  it('updates only the stylesheet for a module whose types alone are used', async () => {
    expect(await save('src/types.ts')).toEqual(['update /src/style.css']);
    expect(await remove('src/types.ts')).toEqual(['update /src/style.css']);
  });

  it('updates only the stylesheet for a lazy module until a page follows the import', async () => {
    expect(await save('src/lazy.ts')).toEqual(['update /src/style.css']);

    await request('/src/lazy.ts');
    expect(await save('src/lazy.ts')).toEqual(['full-reload']);
  });

  it('updates only the stylesheet for a worker entry until a page starts the worker', async () => {
    expect(await save('src/worker.ts')).toEqual(['update /src/style.css']);

    await request('/src/worker.ts?worker_file&type=module');
    expect(await save('src/worker.ts')).toEqual(['full-reload']);
  });

  it('leaves a module a page loaded to Vite', async () => {
    expect(await save('src/panel.ts')).toEqual(['update /src/panel.ts /src/style.css']);
    expect(await save('src/label.ts')).toEqual(['full-reload']);
  });

  it('counts a module as loaded when its first compile fails', async () => {
    await expect(request('/src/broken.ts')).rejects.toThrow();
    expect(await save('src/broken.ts')).toEqual(['full-reload']);
  });

  it('leaves a module whose import failed to Vite when the missing file is created', async () => {
    await expect(request('/src/incomplete.ts')).rejects.toThrow();

    fs.writeFileSync(path.join(root, 'src/missing.ts'), 'export {};\n');
    expect(await sentAfter('add', 'src/missing.ts')).toEqual(['full-reload']);
  });

  it('still reloads the page for the entry', async () => {
    expect(await save('src/main.ts')).toEqual(['full-reload']);
  });

  it('still reloads the page for a stylesheet dependency outside the source directory', async () => {
    expect(await save('theme.config.ts')).toEqual(['full-reload']);
  });
});
