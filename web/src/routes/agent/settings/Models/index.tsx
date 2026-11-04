import type { UseQueryResult } from '@tanstack/react-query';
import { BrainCircuit, KeyRound, Pencil, Plus } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type * as providersApi from '@/api/agent/modelProviders';
import { useActionAccess } from '@/product/actionAccess';
import { ProductState } from '@/product/states';
import { Badge } from '@/shell/ui/badge';
import { Button } from '@/shell/ui/button';

import { ModelProviderEditorDrawer, type ProviderEditorTarget } from './Editor';
import { ProviderPrices } from './ProviderPrices';
import { CurrenciesSection } from '../Currencies';
import { AgentSettingsSection } from '../Surfaces';
import { useCurrencies } from '../useCurrencies';

type ProvidersQuery = UseQueryResult<providersApi.ModelProvider[], Error>;

/**
 * Each control answers to what its API asks for: providers to `agent.models.manage`, a new key for
 * one of them to `agent.models.rotate`, and the currencies to the usage permissions. The section
 * shows what the reader may read: a role can keep the currencies without seeing a provider.
 */
export function ModelsPanel({ providers }: { providers: ProvidersQuery }) {
  const readAccess = useActionAccess({ permission: 'agent.models.read' });
  const currenciesAccess = useActionAccess({ permission: 'agent.usage.read' });
  const usageAccess = useActionAccess({ permission: 'agent.usage.manage' });

  if (readAccess.disabled && currenciesAccess.disabled) {
    return <ProductState variant="permission-denied" description={readAccess.reason} />;
  }

  return (
    <div className="flex flex-col gap-6">
      {readAccess.allowed && <ProvidersSection providers={providers} />}
      {currenciesAccess.allowed && <CurrenciesSection canManage={usageAccess.allowed} />}
    </div>
  );
}

function ProvidersSection({ providers }: { providers: ProvidersQuery }) {
  const { t } = useTranslation('agent');
  const [editor, setEditor] = React.useState<ProviderEditorTarget>(null);
  const { symbolOf } = useCurrencies();
  const manageAccess = useActionAccess({ permission: 'agent.models.manage' });
  const rotateAccess = useActionAccess({ permission: 'agent.models.rotate' });
  if (providers.isLoading) return <ProductState variant="loading" />;
  if (providers.isError) return <ProductState variant="error" error={providers.error} />;
  return (
    <AgentSettingsSection
      title={t('settings.models.title')}
      description={t('settings.models.description')}
      action={<Button size="sm" disabled={manageAccess.disabled} onClick={() => setEditor('new')}><Plus />{t('settings.models.create')}</Button>}
    >
      {providers.data?.length ? (
        <div className="divide-y divide-bd-0 overflow-hidden rounded-lg border border-bd-0 bg-bg-2">
          {providers.data.map((provider) => (
            <div key={provider.id} className="flex flex-wrap items-center gap-4 px-4 py-3">
              <div className="grid h-8 w-8 place-items-center rounded-md border border-bd-0 bg-bg-1">
                <BrainCircuit className="h-4 w-4 text-tx-2" />
              </div>
              <div className="min-w-0 flex-1">
                <div className="font-strong text-tx-0">{provider.name}</div>
                <div className="mt-0.5 font-mono text-xs text-tx-3">{provider.defaultModel}</div>
                <div className="mt-0.5 max-w-xl truncate font-mono text-xs text-tx-3">
                  {provider.baseUrl ?? t('settings.models.default_endpoint')}
                </div>
                <ProviderPrices provider={provider} symbolOf={symbolOf} />
              </div>
              <Badge variant="outline">{provider.provider}</Badge>
              <span className="inline-flex items-center gap-1.5 text-xs text-tx-3">
                <KeyRound className="h-3.5 w-3.5" />
                {provider.keySet ? t('settings.models.key_set') : t('settings.models.key_unset')}
              </span>
              <Badge variant="outline">{provider.enabled ? t('status.enabled') : t('status.disabled')}</Badge>
              <Button
                variant="ghost"
                size="icon"
                aria-label={t('settings.models.edit')}
                disabled={manageAccess.disabled && rotateAccess.disabled}
                onClick={() => setEditor(provider)}
              >
                <Pencil />
              </Button>
            </div>
          ))}
        </div>
      ) : (
        <ProductState
          variant="empty"
          compact
          title={t('settings.models.empty_title')}
          // On the content surface itself, not in a card set into it.
          className="bg-transparent [box-shadow:none]"
        />
      )}
      <ModelProviderEditorDrawer
        target={editor}
        onClose={() => setEditor(null)}
      />
    </AgentSettingsSection>
  );
}
