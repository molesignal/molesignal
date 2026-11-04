import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as networkApi from '@/api/agent/networkPolicy';
import { toApiError } from '@/lib/http';
import { formatMicrosActive } from '@/lib/time';
import { useActionAccess } from '@/product/actionAccess';
import { ProductState } from '@/product/states';
import { uiSectionTitleClass } from '@/shell/chrome';
import { FormField, FormRow, FormTextarea } from '@/shell/FormDrawer';
import { Button } from '@/shell/ui/button';
import { toast } from '@/shell/ui/sonner';

function lines(value: string): string[] {
  return value
    .split(/[\n,]/)
    .map((item) => item.trim())
    .filter(Boolean);
}

/**
 * The internal hosts and address ranges team members and individuals may
 * point their skill repositories and MCP servers at. Everything else they add
 * can only reach the internet; what administrators add for the organization
 * follows its own settings. It says itself what it lacks: the section's notice is for its profile.
 */
export function MemberNetworkPolicy() {
  const { t } = useTranslation('agent');
  const queryClient = useQueryClient();
  const readAccess = useActionAccess({ permission: 'agent.network_policy.read' });
  const manageAccess = useActionAccess({ permission: 'agent.network_policy.manage' });
  const policy = useQuery({
    queryKey: ['agent', 'network-policy'],
    queryFn: networkApi.getNetworkPolicy,
    enabled: readAccess.allowed,
    retry: false,
  });
  const [hosts, setHosts] = React.useState('');
  const [cidrs, setCidrs] = React.useState('');

  const saved = policy.data;
  React.useEffect(() => {
    setHosts((saved?.privateHosts ?? []).join('\n'));
    setCidrs((saved?.privateCidrs ?? []).join('\n'));
  }, [saved]);

  const save = useMutation({
    mutationFn: () =>
      networkApi.putNetworkPolicy({ privateHosts: lines(hosts), privateCidrs: lines(cidrs) }),
    onSuccess: async (next) => {
      queryClient.setQueryData(['agent', 'network-policy'], next);
      toast.success(t('settings.network.members.saved'));
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const changed =
    lines(hosts).join('\n') !== (saved?.privateHosts ?? []).join('\n') ||
    lines(cidrs).join('\n') !== (saved?.privateCidrs ?? []).join('\n');
  const lacking = readAccess.reason ?? manageAccess.reason;

  return (
    <section className="mt-6 first:mt-0" aria-labelledby="agent-member-network" data-agent-member-network>
      <h3 id="agent-member-network" className={uiSectionTitleClass}>
        {t('settings.network.members.title')}
      </h3>
      <p className="mt-1 max-w-3xl text-xs leading-5 text-tx-3">
        {t('settings.network.members.description')}
      </p>
      {lacking && (
        <p role="status" className="mt-1 max-w-3xl text-xs leading-5 text-tx-2">
          {lacking}
        </p>
      )}
      {readAccess.disabled ? null : policy.isLoading ? (
        <ProductState variant="loading" compact />
      ) : policy.isError ? (
        <ProductState variant="error" error={policy.error} compact />
      ) : (
        <>
          <FormRow className="mt-3">
            <FormField
              label={t('settings.network.members.hosts')}
              hint={t('settings.network.members.hosts_hint')}
            >
              <FormTextarea
                value={hosts}
                disabled={manageAccess.disabled}
                onChange={(event) => setHosts(event.target.value)}
                rows={4}
                spellCheck={false}
                className="font-mono text-xs"
              />
            </FormField>
            <FormField
              label={t('settings.network.members.cidrs')}
              hint={t('settings.network.members.cidrs_hint')}
            >
              <FormTextarea
                value={cidrs}
                disabled={manageAccess.disabled}
                onChange={(event) => setCidrs(event.target.value)}
                rows={4}
                spellCheck={false}
                className="font-mono text-xs"
              />
            </FormField>
          </FormRow>
          <div className="mt-3 flex items-center gap-3">
            <Button
              size="sm"
              disabled={manageAccess.disabled || !changed || save.isPending}
              onClick={() => save.mutate()}
            >
              {t('common.save')}
            </Button>
            {saved?.updatedAt ? (
              <span className="text-xs text-tx-3">
                {t('settings.network.members.updated', {
                  time: formatMicrosActive(saved.updatedAt),
                })}
              </span>
            ) : null}
          </div>
        </>
      )}
    </section>
  );
}
