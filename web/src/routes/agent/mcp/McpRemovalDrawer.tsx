import { useTranslation } from 'react-i18next';

import type * as agentApi from '@/api/agent';
import { FormDrawer, FormSection } from '@/shell/FormDrawer';
import { cn } from '@/shell/lib/cn';
import { Button } from '@/shell/ui/button';

export type McpRemovalTarget = {
  server: agentApi.McpServer;
  tools: agentApi.McpToolRow[];
  dependencies: agentApi.ToolDependencies[];
};

/** Removes a server once none of its tools is on or used by anything. */
export function McpRemovalDrawer({
  target,
  pending,
  onClose,
  onConfirm,
}: {
  target: McpRemovalTarget | null;
  pending: boolean;
  onClose: () => void;
  onConfirm: (server: agentApi.McpServer) => void;
}) {
  const { t } = useTranslation('agent');
  if (!target) return null;
  const enabledTools = target.tools.filter((tool) => tool.enabled);
  const dependencyCount = target.dependencies.reduce(
    (total, dependency) => total + dependency.total,
    0,
  );
  const canRemove = enabledTools.length === 0 && dependencyCount === 0;
  return (
    <FormDrawer
      open
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
      title={t('settings.tools.mcp.remove_drawer.title', {
        name: target.server.name,
      })}
      subtitle={t('settings.tools.mcp.remove_drawer.description')}
      width={620}
      footer={
        <>
          <Button variant="outline" onClick={onClose}>
            {t('common.cancel')}
          </Button>
          <Button
            variant="destructive"
            disabled={!canRemove || pending}
            onClick={() => onConfirm(target.server)}
          >
            {t('settings.tools.mcp.remove_drawer.confirm')}
          </Button>
        </>
      }
    >
      <div
        className={cn(
          'rounded-md p-3 text-xs leading-5',
          canRemove
            ? 'bg-[var(--green-tint)] text-[var(--green-ink)]'
            : 'bg-[var(--yellow-tint)] text-[var(--yellow-ink)]',
        )}
      >
        {canRemove
          ? t('settings.tools.mcp.remove_drawer.safe')
          : t('settings.tools.mcp.remove_drawer.blocked', {
              enabled: enabledTools.length,
              dependencies: dependencyCount,
            })}
      </div>
      <FormSection title={t('settings.tools.mcp.remove_drawer.tools_title')} className="mt-6">
        {target.tools.length === 0 ? (
          <p className="text-xs text-tx-3">{t('settings.tools.mcp.remove_drawer.no_tools')}</p>
        ) : (
          <div className="divide-y divide-bd-0 overflow-hidden rounded-md">
            {target.tools.map((tool, index) => {
              const dependencies = target.dependencies[index];
              return (
                <div key={tool.id} className="flex items-center gap-3 bg-bg-2 px-3 py-2.5">
                  <span className="min-w-0 flex-1 truncate font-mono text-xs text-tx-1">
                    {tool.name}
                  </span>
                  {tool.enabled && (
                    <span className="rounded bg-[var(--yellow-tint)] px-1.5 text-type-micro font-strong text-[var(--yellow-ink)]">
                      {t('settings.tools.status.enabled')}
                    </span>
                  )}
                  <span className="rounded bg-bg-3 px-1.5 text-type-micro text-tx-2">
                    {t('settings.tools.mcp.remove_drawer.dependency_count', {
                      count: dependencies?.total ?? 0,
                    })}
                  </span>
                </div>
              );
            })}
          </div>
        )}
      </FormSection>
    </FormDrawer>
  );
}
