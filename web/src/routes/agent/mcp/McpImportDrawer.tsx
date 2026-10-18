import { useMutation } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as agentApi from '@/api/agent';
import { toApiError } from '@/lib/http';
import { FormDrawer, FormField, FormSection, FormSelect, FormTextarea } from '@/shell/FormDrawer';
import { Button } from '@/shell/ui/button';
import { toast } from '@/shell/ui/sonner';

import { scopeTarget, useScopes } from '../scope/useScopes';

const EXAMPLE = `{
  "mcpServers": {
    "github": {
      "url": "https://api.githubcopilot.com/mcp/",
      "headers": { "Authorization": "Bearer <token>" }
    }
  }
}`;

/**
 * Makes servers out of the `mcpServers` configuration of Claude Desktop,
 * Cursor, VS Code and the like. Servers with a `url` are ready to test;
 * `command` ones are kept to run on a Mole Runner; values that refer to
 * variables are left to fill in.
 */
export function McpImportDrawer({
  open,
  onClose,
  onImported,
}: {
  open: boolean;
  onClose: () => void;
  onImported: () => Promise<void>;
}) {
  const { t } = useTranslation('agent');
  const scopes = useScopes();
  const [scope, setScope] = React.useState('personal');
  const [config, setConfig] = React.useState('');
  const [report, setReport] = React.useState<agentApi.McpImportReport | null>(null);

  React.useEffect(() => {
    if (open) {
      setConfig('');
      setReport(null);
    }
  }, [open]);

  const mutation = useMutation({
    mutationFn: () => agentApi.importMcpServers({ ...scopeTarget(scope), config }),
    onSuccess: async (result) => {
      setReport(result);
      if (result.created.length > 0) {
        toast.success(
          t('settings.tools.mcp.import.created', { count: result.created.length }),
        );
        await onImported();
      }
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  return (
    <FormDrawer
      open={open}
      onOpenChange={(next) => {
        if (!next) onClose();
      }}
      title={t('settings.tools.mcp.import.title')}
      subtitle={t('settings.tools.mcp.import.description')}
      width={640}
      footer={
        <>
          <Button variant="outline" onClick={onClose}>
            {t(report ? 'common.close' : 'common.cancel')}
          </Button>
          <Button
            onClick={() => mutation.mutate()}
            disabled={mutation.isPending || !config.trim()}
          >
            {t('settings.tools.mcp.import.submit')}
          </Button>
        </>
      }
    >
      <FormSection>
        <FormField label={t('scope.field')}>
          <FormSelect
            value={scope}
            onChange={setScope}
            options={scopes.options}
            ariaLabel={t('scope.field')}
            className="w-full"
          />
        </FormField>
        <FormField
          label={t('settings.tools.mcp.import.config')}
          hint={t('settings.tools.mcp.import.config_hint')}
        >
          <FormTextarea
            value={config}
            onChange={(event) => setConfig(event.target.value)}
            placeholder={EXAMPLE}
            rows={12}
            spellCheck={false}
            className="font-mono text-xs"
          />
        </FormField>
      </FormSection>
      {report && (
        <FormSection title={t('settings.tools.mcp.import.result')}>
          <ul className="space-y-2" data-agent-mcp-import-report>
            {report.created.map((server) => (
              <li key={server.id} className="rounded-md bg-bg-2 px-3 py-2 text-xs leading-5">
                <span className="font-strong text-tx-0">{server.name}</span>
                <span className="ml-2 text-[var(--green-ink)]">
                  {t('settings.tools.mcp.import.made')}
                </span>
                {server.transport === 'stdio' && (
                  <span className="block text-tx-3">
                    {t('settings.tools.mcp.import.stdio')}
                  </span>
                )}
                {server.missing.length > 0 && (
                  <span className="block text-[var(--yellow-ink)]">
                    {t('settings.tools.mcp.import.missing', { names: server.missing.join(', ') })}
                  </span>
                )}
              </li>
            ))}
            {report.skipped.map((skip) => (
              <li key={skip.name} className="rounded-md bg-bg-2 px-3 py-2 text-xs leading-5">
                <span className="font-strong text-tx-0">{skip.name}</span>
                <span className="ml-2 text-tx-3">{skip.reason}</span>
              </li>
            ))}
          </ul>
        </FormSection>
      )}
    </FormDrawer>
  );
}
