import { useMutation, useQuery } from '@tanstack/react-query';
import { BookOpenCheck } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import * as agentApi from '@/api/agent';
import { toApiError } from '@/lib/http';
import { ProductState } from '@/product/states';
import { Button } from '@/shell/ui/button';
import { toast } from '@/shell/ui/sonner';
import { Switch } from '@/shell/ui/switch';

/**
 * The tools and prompts of a server. Those who can change the server turn its
 * tools on and off here, one by one or every read-only one at once; the
 * organization's tool policy and approvals still apply to them.
 */
export function McpServerDetails({
  server,
  onChanged,
}: {
  server: agentApi.McpServer;
  onChanged: () => Promise<void>;
}) {
  const { t } = useTranslation('agent');
  const detail = useQuery({
    queryKey: ['agent', 'mcp-server', server.id],
    queryFn: () => agentApi.getMcpServer(server.id),
    retry: false,
  });
  const fail = (error: unknown) => toast.error(toApiError(error).message);
  const toggle = useMutation({
    mutationFn: ({ tool, enabled }: { tool: agentApi.McpToolRow; enabled: boolean }) =>
      agentApi.setMcpToolEnabled(server.id, tool.id, enabled),
    onSuccess: async () => {
      await Promise.all([detail.refetch(), onChanged()]);
    },
    onError: fail,
  });
  const readOnly = useMutation({
    mutationFn: () => agentApi.enableReadOnlyMcpTools(server.id),
    onSuccess: async (result) => {
      toast.success(t('settings.tools.mcp.details.read_only_enabled', { count: result.enabled }));
      await Promise.all([detail.refetch(), onChanged()]);
    },
    onError: fail,
  });

  if (detail.isLoading) return <ProductState variant="loading" compact />;
  if (detail.isError) return <ProductState variant="error" error={detail.error} compact />;
  const tools = detail.data?.tools ?? [];
  const prompts = detail.data?.prompts ?? [];
  const readOnlyOff = tools.filter(
    (tool) =>
      !tool.enabled &&
      tool.capabilities.read_only === true &&
      tool.minimum_risk === 'l0' &&
      tool.status !== 'unavailable',
  ).length;

  return (
    <div className="space-y-4 bg-bg-2 px-4 py-3" data-agent-mcp-details={server.handle}>
      {tools.length > 0 && (
        <div>
          <div className="flex items-center gap-2">
            <h4 className="text-xs font-strong text-tx-1">
              {t('settings.tools.mcp.details.tools', { count: tools.length })}
            </h4>
            {server.editable && readOnlyOff > 0 && (
              <Button
                size="sm"
                variant="outline"
                className="ml-auto"
                disabled={readOnly.isPending}
                onClick={() => readOnly.mutate()}
              >
                <BookOpenCheck className="h-3.5 w-3.5" />
                {t('settings.tools.mcp.details.enable_read_only', { count: readOnlyOff })}
              </Button>
            )}
          </div>
          <ul className="mt-2 divide-y divide-bd-0 overflow-hidden rounded-md bg-bg-1">
            {tools.map((tool) => (
              <li key={tool.id} className="flex items-start gap-3 px-3 py-2.5">
                <span className="min-w-0 flex-1">
                  <span className="flex flex-wrap items-center gap-1.5">
                    <span className="font-mono text-xs font-strong text-tx-0">{tool.name}</span>
                    <span className="rounded bg-bg-3 px-1 text-type-micro text-tx-2">
                      {tool.risk.toUpperCase()}
                    </span>
                    {tool.capabilities.read_only === true && (
                      <span className="rounded bg-bg-3 px-1 text-type-micro text-tx-2">
                        {t('settings.tools.mcp.details.read_only')}
                      </span>
                    )}
                  </span>
                  {tool.description && (
                    <span className="mt-0.5 block text-xs text-tx-3">{tool.description}</span>
                  )}
                  {tool.unavailable_diagnostic && (
                    <span className="mt-0.5 block text-xs text-[var(--yellow-ink)]">
                      {tool.unavailable_diagnostic}
                    </span>
                  )}
                </span>
                <Switch
                  aria-label={t('settings.tools.mcp.details.tool_switch', { name: tool.name })}
                  checked={tool.enabled}
                  disabled={
                    !server.editable ||
                    toggle.isPending ||
                    (!tool.enabled && tool.status === 'unavailable')
                  }
                  onCheckedChange={(enabled) => toggle.mutate({ tool, enabled })}
                />
              </li>
            ))}
          </ul>
        </div>
      )}
      {prompts.length > 0 && (
        <div>
          <h4 className="text-xs font-strong text-tx-1">
            {t('settings.tools.mcp.details.prompts', { count: prompts.length })}
          </h4>
          <p className="mt-0.5 text-xs text-tx-3">{t('settings.tools.mcp.details.prompts_hint')}</p>
          <ul className="mt-2 divide-y divide-bd-0 overflow-hidden rounded-md bg-bg-1">
            {prompts.map((prompt) => (
              <li key={prompt.name} className="px-3 py-2.5 text-xs">
                <span className="font-mono font-strong text-tx-0">
                  /{server.handle}:{promptSlug(prompt.name)}
                </span>
                {prompt.arguments.length > 0 && (
                  <span className="ml-2 font-mono text-tx-3">
                    {prompt.arguments
                      .map((argument) => (argument.required ? `<${argument.name}>` : `[${argument.name}]`))
                      .join(' ')}
                  </span>
                )}
                {(prompt.description || prompt.title) && (
                  <span className="mt-0.5 block text-tx-3">{prompt.description || prompt.title}</span>
                )}
              </li>
            ))}
          </ul>
        </div>
      )}
    </div>
  );
}

/**
 * The name a prompt goes by in its command, as the backend makes it
 * (`crates/modules/agent/src/mcp/handle.rs`): lower case letters, digits, `-`,
 * and `_` for anything else.
 */
export function promptSlug(name: string): string {
  const slug = name
    .toLowerCase()
    .replace(/[^a-z0-9-]+/g, '_')
    .replace(/^_+|_+$/g, '')
    .slice(0, 48)
    .replace(/_+$/g, '');
  return slug || 'prompt';
}
