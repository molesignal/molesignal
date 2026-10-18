import type { TFunction } from 'i18next';

/** What a tool call did, as an answer's one-line account of its work names it. */
export type ActivityKind =
  | 'look'
  | 'query'
  | 'knowledge'
  | 'knowledge_edit'
  | 'memory'
  | 'remember'
  | 'question'
  | 'plan'
  | 'change'
  | 'tool';

const KNOWLEDGE_READS = new Set(['list_knowledge', 'read_knowledge', 'search_knowledge']);

/** The kind of a tool, from its name. */
export function activityKind(tool: string): ActivityKind {
  if (KNOWLEDGE_READS.has(tool)) return 'knowledge';
  if (tool === 'edit_knowledge') return 'knowledge_edit';
  if (tool === 'read_memory') return 'memory';
  if (tool === 'remember' || tool === 'forget') return 'remember';
  if (tool === 'ask_user') return 'question';
  if (tool === 'todo_write') return 'plan';
  if (/^(query|search|run)_|_query$/.test(tool)) return 'query';
  if (/^(propose|create|update|delete|set|write|acknowledge|resolve|silence|mute)_/.test(tool)) {
    return 'change';
  }
  if (/^(list|get|describe|read|show|find|lookup|inspect)_/.test(tool)) return 'look';
  return 'tool';
}

/**
 * An answer's work in one line, the kinds in the order they first came:
 * "查看了 3 项，读了 1 次知识库" / "Looked at 3 items, read the knowledge base".
 */
export function activitySummary(tools: readonly string[], t: TFunction<'agent'>): string {
  const counts = new Map<ActivityKind, number>();
  for (const tool of tools) {
    const kind = activityKind(tool);
    counts.set(kind, (counts.get(kind) ?? 0) + 1);
  }
  const text = [...counts]
    .map(([kind, count]) => t(`activity.${kind}`, { count }))
    .join(t('activity.separator'));
  return text.charAt(0).toLocaleUpperCase() + text.slice(1);
}
