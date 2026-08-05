/**
 * Endpoint reality audit: walk `crates/api/src/http/routes/*.rs` for axum
 * `.route("/path", get/post/put/delete(...))` declarations and walk
 * `web/src/api/*.ts` for `http.<verb>('/literal-path', ...)` calls. Any
 * frontend literal whose path-template (with `${id}` collapsed to `{}`)
 * has no matching backend declaration prints as `endpoint-mismatch`.
 *
 * Heuristic, not exhaustive — designed to catch typos / dropped prefixes,
 * not to validate request shapes.
 *
 * Usage: `tsx scripts/audit-api-endpoints.ts`
 */
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const HERE = dirname(fileURLToPath(import.meta.url));
const WEB = resolve(HERE, '..');
const REPO = resolve(WEB, '..');
const BACKEND_ROUTES_DIR = join(REPO, 'crates/api/src/http/routes');
const FRONTEND_API_DIR = join(WEB, 'src/api');

type Verb = 'GET' | 'POST' | 'PUT' | 'DELETE';
interface Endpoint {
  verb: Verb;
  path: string;
}

/**
 * Endpoints whose frontend client lives in this repo but whose backend
 * route is still pending. The audit prints them as warnings instead of
 * failing CI.
 */
const PENDING_BACKEND: Endpoint[] = [
  // web-backend-integration spec: spec'd, awaiting Rust handler. Tracked
  // separately in the spec; do not silently treat as drift.
  { verb: 'POST', path: '/orgs/{}/select' },
];

function walkRustFiles(dir: string): string[] {
  const out: string[] = [];
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    const s = statSync(p);
    if (s.isDirectory()) out.push(...walkRustFiles(p));
    else if (name.endsWith('.rs')) out.push(p);
  }
  return out;
}

/**
 * Read a parenthesized chunk starting at the index just after the opening
 * paren of `.route("path", ` and return its content + the index of the
 * matching close paren. Tracks string literals so `")"` inside a string
 * doesn't close early.
 */
function readUntilClose(src: string, from: number): { body: string; end: number } {
  let depth = 1;
  let i = from;
  let inStr: '"' | "'" | null = null;
  while (i < src.length) {
    const c = src[i]!;
    if (inStr) {
      if (c === '\\') {
        i += 2;
        continue;
      }
      if (c === inStr) inStr = null;
    } else {
      if (c === '"' || c === "'") inStr = c;
      else if (c === '(') depth++;
      else if (c === ')') {
        depth--;
        if (depth === 0) return { body: src.slice(from, i), end: i };
      }
    }
    i++;
  }
  return { body: src.slice(from), end: src.length };
}

function parseRustRoutes(file: string, prefixFromParent: string): Endpoint[] {
  const src = readFileSync(file, 'utf8');
  const out: Endpoint[] = [];
  // Local `.nest("/sub", ...)` (e.g. routes/web/mod.rs) — captured for the
  // file's own routes (parent prefix already handled by caller).
  const localPrefixMatch = src.match(/\.nest\("([^"]+)"/);
  const localPrefix = localPrefixMatch ? localPrefixMatch[1]! : '';
  const prefix = prefixFromParent + localPrefix;

  const routeStart = /\.route\(\s*"([^"]+)"\s*,\s*/g;
  let m: RegExpExecArray | null;
  while ((m = routeStart.exec(src)) !== null) {
    const path = (prefix + m[1]!).replace(/\{[^}]+\}/g, '{}');
    const open = routeStart.lastIndex;
    const { body, end } = readUntilClose(src, open);
    routeStart.lastIndex = end + 1;

    const verbs: Verb[] = [];
    if (/\bget\(/.test(body)) verbs.push('GET');
    if (/\bpost\(/.test(body)) verbs.push('POST');
    if (/\bput\(/.test(body)) verbs.push('PUT');
    if (/\bdelete\(/.test(body)) verbs.push('DELETE');
    if (verbs.length === 0) verbs.push('GET');
    for (const v of verbs) out.push({ verb: v, path });
  }
  return out;
}

function parseFrontendApi(file: string): Endpoint[] {
  const src = readFileSync(file, 'utf8');
  const out: Endpoint[] = [];
  const re = /http\.(get|post|put|delete)\s*<[^>]*>?\s*\(\s*([`'"])([^`'"]+)\2/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(src)) !== null) {
    const verb = m[1]!.toUpperCase() as Verb;
    let path = m[3]!;
    path = path.replace(/\$\{[^}]+\}/g, '{}');
    out.push({ verb, path });
  }
  return out;
}

function pathMatches(beSeg: string[], feSeg: string[]): boolean {
  if (beSeg.length !== feSeg.length) return false;
  return beSeg.every((seg, i) => {
    const f = feSeg[i]!;
    if (seg === '{}') return f === '{}' || f.length > 0;
    return seg === f;
  });
}

function backendMatches(fe: Endpoint, backend: Endpoint[]): boolean {
  const feSeg = fe.path.split('/');
  return backend.some((be) => be.verb === fe.verb && pathMatches(be.path.split('/'), feSeg));
}

function pendingMatches(fe: Endpoint): boolean {
  const feSeg = fe.path.split('/');
  return PENDING_BACKEND.some((p) => p.verb === fe.verb && pathMatches(p.path.split('/'), feSeg));
}

function collectBackend(): Endpoint[] {
  const out: Endpoint[] = [];
  for (const f of walkRustFiles(BACKEND_ROUTES_DIR)) {
    // Files under `routes/web/` inherit the `/web` nest prefix declared in
    // `routes/web/mod.rs`. The submodule files only declare relative paths
    // like `/search`, so we have to add `/web` here.
    const rel = relative(BACKEND_ROUTES_DIR, f);
    const parentPrefix = rel.split('/')[0] === 'web' && !rel.endsWith('web/mod.rs') ? '/web' : '';
    out.push(...parseRustRoutes(f, parentPrefix));
  }
  return out;
}

function main(): number {
  const backend = collectBackend();
  const feFiles = readdirSync(FRONTEND_API_DIR)
    .filter((n) => n.endsWith('.ts') && n !== 'index.ts')
    .map((n) => join(FRONTEND_API_DIR, n));

  let mismatches = 0;
  let warnings = 0;
  for (const f of feFiles) {
    for (const ep of parseFrontendApi(f)) {
      if (backendMatches(ep, backend)) continue;
      if (pendingMatches(ep)) {
        warnings++;
        console.warn(
          `endpoint-pending [${ep.verb} ${ep.path}] in ${relative(REPO, f)} — backend not implemented yet (tracked)`,
        );
        continue;
      }
      mismatches++;
      console.error(
        `endpoint-mismatch [${ep.verb} ${ep.path}] in ${relative(REPO, f)} — no matching backend route`,
      );
    }
  }

  console.log(
    `\naudit-api-endpoints: ${feFiles.length} client files, ${backend.length} backend routes, ${warnings} pending, ${mismatches} mismatch.`,
  );
  return mismatches > 0 ? 1 : 0;
}

process.exit(main());
