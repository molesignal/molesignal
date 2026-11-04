/**
 * The conditions a template can hold, worked out as the server does
 * (`bin/molesignal/src/app/notify/engine/blocks.rs`):
 *
 * - `{{#if path}}…{{/if}}` keeps what is inside when the value is not empty;
 * - `{{#if path == "value"}}` and `{{#if path != "value"}}` compare it as text;
 * - `{{else}}` holds what is kept otherwise.
 *
 * Blocks nest at most five deep and are worked out before the placeholders, which are left for later.
 */

export const MAX_BLOCK_DEPTH = 5;

type Condition =
  | { kind: 'present'; path: string }
  | { kind: 'equals' | 'differs'; path: string; value: string };

type Tag = { kind: 'if'; condition: Condition } | { kind: 'else' } | { kind: 'end' } | { kind: 'other' };

/** Why a template's blocks do not hold together; `null` when they do. */
export type BlockProblem = string | null;

const VARIABLE = /^[\p{L}\p{N}_.-]+$/u;

function quoted(value: string): string | null {
  const trimmed = value.trim();
  const quote = trimmed[0];
  if ((quote === '"' || quote === "'") && trimmed.length >= 2 && trimmed.endsWith(quote)) {
    return trimmed.slice(1, -1);
  }
  return null;
}

function parseTag(tag: string): Tag | string {
  if (tag === 'else') return { kind: 'else' };
  if (tag === '/if') return { kind: 'end' };
  if (!tag.startsWith('#')) return { kind: 'other' };

  const match = /^#if\s+(.+)$/s.exec(tag);
  if (!match) return `{{${tag}}} is not a condition: write {{#if …}}`;

  const condition = match[1]!.trim();

  for (const [operator, kind] of [
    ['==', 'equals'],
    ['!=', 'differs'],
  ] as const) {
    const at = condition.indexOf(operator);
    if (at >= 0) {
      const path = condition.slice(0, at).trim();
      const value = quoted(condition.slice(at + operator.length));
      if (!VARIABLE.test(path)) return `${path} is not a variable a condition can test`;
      if (value === null) return 'compare with a quoted value';
      return { kind: 'if', condition: { kind, path, value } };
    }
  }

  if (!VARIABLE.test(condition)) return `${condition} is not a variable a condition can test`;
  return { kind: 'if', condition: { kind: 'present', path: condition } };
}

function holds(condition: Condition, values: Record<string, string>): boolean {
  const value = values[condition.path] ?? '';
  if (condition.kind === 'present') return value.trim() !== '';
  return condition.kind === 'equals' ? value === condition.value : value !== condition.value;
}

interface Frame {
  showing: boolean;
  held: boolean;
  inElse: boolean;
}

function resolve(body: string, values: Record<string, string>): { text: string; problem: BlockProblem } {
  let output = '';
  const stack: Frame[] = [];
  let rest = body;
  const visible = () => stack.every((frame) => frame.showing);

  for (;;) {
    const start = rest.indexOf('{{');
    if (start < 0) break;
    if (visible()) output += rest.slice(0, start);

    const after = rest.slice(start + 2);
    const end = after.indexOf('}}');
    if (end < 0) {
      if (visible()) output += rest.slice(start);
      rest = '';
      break;
    }

    const raw = after.slice(0, end);
    const tag = parseTag(raw.trim());
    if (typeof tag === 'string') return { text: body, problem: tag };

    if (tag.kind === 'if') {
      if (stack.length >= MAX_BLOCK_DEPTH) {
        return { text: body, problem: `conditions nest at most ${MAX_BLOCK_DEPTH} deep` };
      }
      const held = holds(tag.condition, values);
      stack.push({ showing: held, held, inElse: false });
    } else if (tag.kind === 'else') {
      const frame = stack[stack.length - 1];
      if (!frame) return { text: body, problem: '{{else}} outside an {{#if}} block' };
      if (frame.inElse) return { text: body, problem: 'an {{#if}} block has two {{else}}' };
      frame.inElse = true;
      frame.showing = !frame.held;
    } else if (tag.kind === 'end') {
      if (!stack.pop()) return { text: body, problem: '{{/if}} without an {{#if}} block' };
    } else if (visible()) {
      output += `{{${raw}}}`;
    }

    rest = after.slice(end + 2);
  }

  if (stack.length > 0) return { text: body, problem: 'an {{#if}} block is not closed with {{/if}}' };
  return { text: output + rest, problem: null };
}

/** The template with the branches that do not hold taken out; as written when its blocks are wrong. */
export function resolveBlocks(body: string, values: Record<string, string>): string {
  if (!body.includes('{{#')) return body;
  return resolve(body, values).text;
}

/** What is wrong with a template's blocks, as the server would say when it is saved. */
export function blockProblem(body: string): BlockProblem {
  return resolve(body, {}).problem;
}
