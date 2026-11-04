import type { CodeEditorMarker } from '@/shell/codeEditor';

/**
 * A policy document read the way the server reads it, so the editor can say
 * what is wrong before saving. A statement allows the organization permissions
 * its actions name; `*` stands for any run of characters. An empty list allows
 * nothing, which is where a new policy starts, not a mistake in it.
 */

export const POLICY_VERSION = '1';

/** Keys of the AWS language this one leaves out, each with its own explanation. */
const UNSUPPORTED_KEYS = ['Resource', 'NotResource', 'NotAction', 'Condition', 'Principal'] as const;

const DOCUMENT_KEYS = new Set(['Version', 'Statement']);
const STATEMENT_KEYS = new Set(['Sid', 'Effect', 'Action']);

export type PolicyProblemCode =
  | 'json'
  | 'not_object'
  | 'unknown_key'
  | 'unsupported_key'
  | 'version'
  | 'statement_list'
  | 'statement_not_object'
  | 'sid'
  | 'effect'
  | 'action_missing'
  | 'action_not_string'
  | 'unknown_action';

export interface PolicyProblem {
  code: PolicyProblemCode;
  /** Where it is, as the server names it: `Statement[0].Action[1]`. */
  path?: string;
  /** The key or action at fault. */
  value?: string;
  /** Text to underline in the document, as written there. */
  find?: string;
}

export type PolicyReading =
  | {
      ok: true;
      permissions: string[];
      /** At most one statement, listing permissions by name: what ticking boxes writes. */
      simple: boolean;
    }
  | { ok: false; problem: PolicyProblem };

/** The document a list of ticked permissions makes. */
export function documentAllowing(permissions: readonly string[]): string {
  const document = {
    Version: POLICY_VERSION,
    Statement: [{ Effect: 'Allow', Action: [...permissions].sort() }],
  };

  return JSON.stringify(document, null, 2);
}

/** The permissions a document allows among `catalog`, or the first thing wrong with it. */
export function readPolicy(text: string, catalog: readonly string[]): PolicyReading {
  let value: unknown;

  try {
    value = JSON.parse(text);
  } catch {
    return { ok: false, problem: { code: 'json' } };
  }

  if (!isObject(value)) return { ok: false, problem: { code: 'not_object' } };

  const statements = value.Statement;

  if (Array.isArray(statements)) {
    for (const [index, statement] of statements.entries()) {
      if (!isObject(statement)) continue;

      const unsupported = UNSUPPORTED_KEYS.find((key) => key in statement);

      if (unsupported) {
        return {
          ok: false,
          problem: {
            code: 'unsupported_key',
            path: `Statement[${index}].${unsupported}`,
            value: unsupported,
            find: `"${unsupported}"`,
          },
        };
      }
    }
  }

  const unknown = Object.keys(value).find((key) => !DOCUMENT_KEYS.has(key));

  if (unknown) {
    return { ok: false, problem: { code: 'unknown_key', path: unknown, value: unknown, find: `"${unknown}"` } };
  }

  if (value.Version !== POLICY_VERSION) {
    return { ok: false, problem: { code: 'version', path: 'Version', find: '"Version"' } };
  }

  if (!Array.isArray(statements)) {
    return { ok: false, problem: { code: 'statement_list', path: 'Statement', find: '"Statement"' } };
  }

  const allowed = new Set<string>();
  let simple = statements.length <= 1;

  for (const [index, statement] of statements.entries()) {
    const at = `Statement[${index}]`;

    if (!isObject(statement)) {
      return { ok: false, problem: { code: 'statement_not_object', path: at } };
    }

    const stray = Object.keys(statement).find((key) => !STATEMENT_KEYS.has(key));

    if (stray) {
      return {
        ok: false,
        problem: { code: 'unknown_key', path: `${at}.${stray}`, value: stray, find: `"${stray}"` },
      };
    }

    if ('Sid' in statement) {
      if (typeof statement.Sid !== 'string') {
        return { ok: false, problem: { code: 'sid', path: `${at}.Sid`, find: '"Sid"' } };
      }

      simple = false;
    }

    if (statement.Effect !== 'Allow') {
      return { ok: false, problem: { code: 'effect', path: `${at}.Effect`, find: '"Effect"' } };
    }

    if (!('Action' in statement)) {
      return { ok: false, problem: { code: 'action_missing', path: `${at}.Action` } };
    }

    const actions = typeof statement.Action === 'string' ? [statement.Action] : statement.Action;

    if (!Array.isArray(actions)) {
      return { ok: false, problem: { code: 'action_not_string', path: `${at}.Action`, find: '"Action"' } };
    }

    for (const [position, raw] of actions.entries()) {
      const path = `${at}.Action[${position}]`;

      if (typeof raw !== 'string') {
        return { ok: false, problem: { code: 'action_not_string', path } };
      }

      const action = raw.trim().toLowerCase();
      const matched = catalog.filter((permission) => actionMatches(action, permission));

      if (matched.length === 0) {
        return {
          ok: false,
          problem: { code: 'unknown_action', path, value: action, find: JSON.stringify(raw) },
        };
      }

      if (action.includes('*')) simple = false;
      matched.forEach((permission) => allowed.add(permission));
    }
  }

  return { ok: true, permissions: [...allowed].sort(), simple };
}

/** Whether an action names `permission`, by name or by a pattern with `*`. */
export function actionMatches(action: string, permission: string): boolean {
  if (!action.includes('*')) return action === permission;

  const parts = action.split('*');
  const first = parts[0] ?? '';
  const last = parts[parts.length - 1] ?? '';

  if (!permission.startsWith(first)) return false;

  let rest = permission.slice(first.length);

  for (const part of parts.slice(1, -1)) {
    const at = rest.indexOf(part);
    if (at < 0) return false;
    rest = rest.slice(at + part.length);
  }

  return rest.length >= last.length && rest.endsWith(last);
}

/** Where a problem is in the text, underlined, when the text says it once. */
export function problemMarkers(text: string, problem: PolicyProblem, message: string): CodeEditorMarker[] {
  if (!problem.find) return [];

  const at = text.indexOf(problem.find);
  if (at < 0) return [];

  const before = text.slice(0, at).split('\n');
  const line = before.length;
  const startColumn = (before[before.length - 1]?.length ?? 0) + 1;

  return [
    {
      line,
      startColumn,
      endColumn: startColumn + problem.find.length,
      message,
      severity: 'error',
    },
  ];
}

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}
