/**
 * The pieces of VRL a processor is written in. Every form here was run on the
 * product's VRL runtime: paths quote what is not an identifier, strings carry
 * no escape VRL does not know (it has no `\u`), and `{{` is escaped because a
 * quoted string is a template.
 */

const IDENTIFIER = /^[A-Za-z_][A-Za-z0-9_]*$/;

function segment(name: string): string {
  if (IDENTIFIER.test(name)) return `.${name}`;

  return `."${name.replace(/\\/g, '\\\\').replace(/"/g, '\\"')}"`;
}

/** The path of a field written the way people write it: `user.email` is `email` inside `user`. */
export function vrlPath(field: string): string {
  const parts = field
    .trim()
    .split('.')
    .map((part) => part.trim())
    .filter(Boolean);

  return parts.length > 0 ? parts.map(segment).join('') : '.';
}

/**
 * Where a field may be found, most literal first. An event can carry
 * `service.name` as one key or as `name` inside `service`; routing looks for
 * the key before the nested path, and so does everything that reads a field.
 */
export function vrlReadPaths(field: string): string[] {
  const name = field.trim();
  const nested = vrlPath(name);
  if (!name.includes('.')) return [nested];

  const literal = segment(name);
  return literal === nested ? [nested] : [literal, nested];
}

/** A quoted VRL string holding exactly `value`. */
export function vrlString(value: string): string {
  let body = '';

  for (const char of value) {
    if (char === '\\') body += '\\\\';
    else if (char === '"') body += '\\"';
    else if (char === '\n') body += '\\n';
    else if (char === '\r') body += '\\r';
    else if (char === '\t') body += '\\t';
    // The other control characters have no escape in VRL and are left out.
    else if ((char.codePointAt(0) ?? 0) >= 0x20) body += char;
  }

  return `"${body.replace(/\{\{/g, '\\{{')}"`;
}

/** A VRL regex literal. The pattern is written as is; only the quote that ends the literal is escaped. */
export function vrlRegex(pattern: string): string {
  return `r'${pattern.replace(/'/g, "\\'")}'`;
}

/**
 * Runs `body` on the field wherever it is found: under the first of its read
 * paths that passes `guard` (`exists`, or `is_string` for what needs text).
 */
export function onField(
  field: string,
  guard: 'exists' | 'is_string',
  body: (path: string) => string[],
): string[] {
  const lines: string[] = [];

  vrlReadPaths(field).forEach((path, index) => {
    lines.push(`${index === 0 ? 'if' : '} else if'} ${guard}(${path}) {`);
    lines.push(...body(path).map((line) => `  ${line}`));
  });
  lines.push('}');

  return lines;
}
