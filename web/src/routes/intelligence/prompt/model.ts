import type {
  AgentPrompt,
  PromptPurpose,
} from '@/api/intelligence/prompts';

const PURPOSE_ORDER: PromptPurpose[] = [
  'system',
  'anomaly_analysis',
  'root_cause',
  'alert_explain',
  'query_generation',
];

export interface ParsedPromptSchema {
  ok: true;
  schema: Record<string, unknown>;
  variables: string[];
}

export interface InvalidPromptSchema {
  ok: false;
  message: string;
}

export type PromptSchemaResult = ParsedPromptSchema | InvalidPromptSchema;

export function promptTemplateVariables(body: string): string[] {
  const variables: string[] = [];
  const pattern = /\{\{\s*([^{}]+?)\s*\}\}/g;
  for (const match of body.matchAll(pattern)) {
    const name = match[1]?.trim();
    if (name && !variables.includes(name)) variables.push(name);
  }
  return variables;
}

export function parsePromptSchema(source: string): PromptSchemaResult {
  let parsed: unknown;
  try {
    parsed = JSON.parse(source);
  } catch (error) {
    return {
      ok: false,
      message: error instanceof Error ? error.message : String(error),
    };
  }
  if (!isRecord(parsed)) {
    return { ok: false, message: 'schema_object_required' };
  }
  const properties = parsed.properties;
  if (properties !== undefined && !isRecord(properties)) {
    return { ok: false, message: 'schema_properties_required' };
  }
  return {
    ok: true,
    schema: parsed,
    variables: properties ? Object.keys(properties) : [],
  };
}

export function unknownPromptVariables(
  body: string,
  schemaVariables: string[],
): string[] {
  return promptTemplateVariables(body).filter(
    (variable) => !schemaVariables.includes(variable),
  );
}

export function effectivePromptIds(prompts: AgentPrompt[]): Set<string> {
  const effective = new Set<string>();
  for (const purpose of PURPOSE_ORDER) {
    const selected = prompts
      .filter(
        (prompt) =>
          prompt.purpose === purpose &&
          prompt.enabled &&
          prompt.is_default,
      )
      .sort((a, b) => scopePriority(a.scope) - scopePriority(b.scope))[0];
    if (selected) effective.add(selected.id);
  }
  return effective;
}

export function groupPromptsByPurpose(
  prompts: AgentPrompt[],
): Array<{ purpose: PromptPurpose; prompts: AgentPrompt[] }> {
  return PURPOSE_ORDER.map((purpose) => ({
    purpose,
    prompts: prompts
      .filter((prompt) => prompt.purpose === purpose)
      .sort((a, b) => {
        const scope = scopePriority(a.scope) - scopePriority(b.scope);
        if (scope !== 0) return scope;
        return b.updated_at_micros - a.updated_at_micros;
      }),
  })).filter((group) => group.prompts.length > 0);
}

function scopePriority(scope: AgentPrompt['scope']): number {
  if (scope === 'user') return 0;
  if (scope === 'org') return 1;
  return 2;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return Boolean(value) && typeof value === 'object' && !Array.isArray(value);
}
