import { useMutation } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as agentApi from '@/api/agent';
import { toApiError } from '@/lib/http';
import { formatMicrosActive } from '@/lib/time';
import { FormDrawer, FormField, FormInput, FormSection } from '@/shell/FormDrawer';
import { Button } from '@/shell/ui/button';
import { toast } from '@/shell/ui/sonner';

/**
 * The signed-in member's own credential for a server that takes one from
 * every member. Only their own calls use it; nobody else, administrators
 * included, can read it back.
 */
export function MyCredentialDrawer({
  server,
  onClose,
  onSaved,
}: {
  server: agentApi.McpServer | null;
  onClose: () => void;
  onSaved: () => Promise<void>;
}) {
  const { t } = useTranslation('agent');
  const [value, setValue] = React.useState('');

  React.useEffect(() => setValue(''), [server]);

  const save = useMutation({
    mutationFn: () => agentApi.putMyMcpCredential(server?.id ?? '', value),
    onSuccess: async () => {
      toast.success(t('settings.tools.mcp.my_credential.saved'));
      await onSaved();
      onClose();
    },
    onError: (error) => toast.error(toApiError(error).message),
  });
  const clear = useMutation({
    mutationFn: () => agentApi.deleteMyMcpCredential(server?.id ?? ''),
    onSuccess: async () => {
      toast.success(t('settings.tools.mcp.my_credential.cleared'));
      await onSaved();
      onClose();
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  if (!server) return null;
  const spec = server.member_credential;
  const saved = server.my_credential;

  return (
    <FormDrawer
      open
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
      title={t('settings.tools.mcp.my_credential.title', { name: server.name })}
      subtitle={t('settings.tools.mcp.my_credential.description')}
      width={560}
      footer={
        <>
          {saved && (
            <Button
              variant="outline"
              className="mr-auto"
              disabled={clear.isPending}
              onClick={() => clear.mutate()}
            >
              {t('settings.tools.mcp.my_credential.clear')}
            </Button>
          )}
          <Button variant="outline" onClick={onClose}>
            {t('common.cancel')}
          </Button>
          <Button onClick={() => save.mutate()} disabled={save.isPending || !value.trim()}>
            {t('common.save')}
          </Button>
        </>
      }
    >
      <FormSection>
        {spec?.instructions && (
          <p
            data-agent-mcp-instructions
            className="whitespace-pre-line rounded-md bg-bg-2 p-3 text-xs leading-5 text-tx-1"
          >
            {spec.instructions}
          </p>
        )}
        <p className="text-xs leading-5 text-tx-3">
          {saved
            ? t('settings.tools.mcp.my_credential.state_saved', {
                last4: saved.last4,
                time: formatMicrosActive(saved.updated_at),
              })
            : t('settings.tools.mcp.my_credential.state_missing')}
        </p>
        <FormField
          label={t('settings.tools.mcp.my_credential.value')}
          hint={t(
            spec?.bearer
              ? 'settings.tools.mcp.my_credential.value_bearer_hint'
              : 'settings.tools.mcp.my_credential.value_hint',
            { header: spec?.header ?? 'authorization' },
          )}
        >
          <FormInput
            type="password"
            autoComplete="new-password"
            value={value}
            onChange={(event) => setValue(event.target.value)}
          />
        </FormField>
      </FormSection>
    </FormDrawer>
  );
}
