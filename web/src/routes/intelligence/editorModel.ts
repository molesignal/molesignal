export type JsonEditorResult =
  | { ok: true; value: unknown }
  | { ok: false; message: string };

export function formatJsonEditor(value: unknown, fallback: unknown = {}): string {
  return JSON.stringify(value ?? fallback, null, 2);
}

export function parseJsonEditor(source: string): JsonEditorResult {
  try {
    return { ok: true, value: JSON.parse(source) as unknown };
  } catch (error) {
    return {
      ok: false,
      message: error instanceof Error ? error.message : String(error),
    };
  }
}

export function parseDelimitedList(source: string): string[] {
  return Array.from(
    new Set(
      source
        .split(/[,\n]/)
        .map((value) => value.trim())
        .filter(Boolean),
    ),
  );
}

export function stringListValue(value: unknown): string {
  return Array.isArray(value)
    ? value.filter((item): item is string => typeof item === 'string').join(', ')
    : '';
}
