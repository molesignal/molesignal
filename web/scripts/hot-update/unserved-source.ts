import path from 'node:path';

import { isCSSRequest, normalizePath, type EnvironmentModuleNode, type Plugin } from 'vite';

/**
 * A change to a source file that no page was ever served reloads no page.
 *
 * Tailwind names every `content` file as a dependency of the stylesheet, and
 * Vite reloads all open pages when the file that changed is imported by
 * stylesheets only. That is the state of every source file no browser has
 * asked for: a module holding nothing but types (the `import type` lines that
 * name it are erased), or one behind a lazy import nobody followed yet. No
 * page runs such a file, so the stylesheets that scan it are updated and
 * nothing else is.
 *
 * A file a page did ask for is left to Vite, the entry and worker entries
 * included: nothing imports those, and a save of one still reloads the page.
 */
export function unservedSourceUpdate(sourceDir: string): Plugin {
  const sourceRoot = `${normalizePath(path.resolve(sourceDir))}/`;
  const served = new Set<string>();

  return {
    name: 'molesignal:unserved-source-update',
    apply: 'serve',
    applyToEnvironment: (environment) => environment.name === 'client',
    transform: {
      // Ahead of the compilers: a file a page asked for counts as served even
      // when it then fails to compile.
      order: 'pre',
      handler(_code, id) {
        const { environment } = this;
        if (environment.mode !== 'dev') return null;

        const file = environment.moduleGraph.getModuleById(id)?.file;
        if (file?.startsWith(sourceRoot)) served.add(file);
        return null;
      },
    },
    hotUpdate({ file, modules }) {
      // Stylesheets stay with Vite as well: one that is `@import`ed reaches the
      // page inside another sheet and is never served under its own name.
      if (!file.startsWith(sourceRoot) || isCSSRequest(file) || served.has(file)) return;

      // Modules of other files stay: when a file is created Vite adds the
      // modules whose imports had failed to resolve.
      const updated = new Set<EnvironmentModuleNode>();
      for (const mod of modules) {
        if (mod.file !== file) {
          updated.add(mod);
          continue;
        }
        for (const importer of mod.importers) {
          if (isCSSRequest(importer.url)) updated.add(importer);
        }
      }
      return [...updated];
    },
  };
}
