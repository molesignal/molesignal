export function parseSampleInput(value: string): unknown {
  const source = value.trim();
  return JSON.parse(source || '{}') as unknown;
}

export function formatSampleInput(value: string): string {
  return JSON.stringify(parseSampleInput(value), null, 2);
}

/**
 * Keep source formatting deliberately lossless. VRL and JavaScript do not
 * share a formatter in the browser bundle, so the workbench normalizes line
 * endings and trailing whitespace without rewriting executable code.
 */
export function formatFunctionSource(value: string): string {
  const lines = value
    .replace(/\r\n?/g, '\n')
    .split('\n')
    .map((line) => line.replace(/[ \t]+$/u, ''));

  while (lines.length > 1 && lines[0]?.trim() === '') lines.shift();
  while (lines.length > 1 && lines.at(-1)?.trim() === '') lines.pop();

  return lines.join('\n');
}

