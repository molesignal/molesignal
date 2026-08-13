import type { TFunction } from 'i18next';

import type { RegisteredTool } from '@/api/agent';

export function localizeToolMetadata(
  tool: RegisteredTool,
  t: TFunction<'agent'>,
): RegisteredTool {
  if (tool.source.kind !== 'builtin') return tool;

  const key = `tools.${tool.name}`;
  return {
    ...tool,
    display_name: t(`${key}.title`, { defaultValue: tool.display_name }),
    description: t(`${key}.description`, { defaultValue: tool.description }),
  };
}

export function localizeToolMetadataList(
  tools: RegisteredTool[],
  t: TFunction<'agent'>,
): RegisteredTool[] {
  return tools.map((tool) => localizeToolMetadata(tool, t));
}
