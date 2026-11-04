import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  ExternalLink,
  KeyRound,
  Link2,
  RefreshCw,
  Save,
  ServerCog,
  ShieldCheck,
} from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import * as inboundMcpApi from '@/api/agent/inboundMcp';
import * as instanceApi from '@/api/instance';
import { resolvePublicBaseUrl, resolvePublicUrl } from '@/lib/publicUrl';
import { useActionAccess } from '@/product/actionAccess';
import { useOpenablePages } from '@/product/moduleAccess';
import { ProductState } from '@/product/states';
import { uiSectionTitleClass } from '@/shell/chrome';
import { Badge } from '@/shell/ui/badge';
import { Button } from '@/shell/ui/button';
import { Label } from '@/shell/ui/label';
import { toast } from '@/shell/ui/sonner';
import { Switch } from '@/shell/ui/switch';
import { Textarea } from '@/shell/ui/textarea';

import {
  EndpointField,
  Field,
  NumberField,
  ConnectPrompt,
  OAuthConnections,
} from './components';

const CREDENTIAL_PAGES = [
  { to: '/iam/api-tokens', key: 'settings.inbound_mcp.credentials.api_tokens' },
  { to: '/iam/service-accounts', key: 'settings.inbound_mcp.credentials.service_accounts' },
] as const;

interface SettingsDraft {
  enabled: boolean;
  origins: string;
  requestSize: string;
  responseSize: string;
  concurrentCalls: string;
  callsPerMinute: string;
  readTimeoutMs: string;
}

function createDraft(
  settings: inboundMcpApi.InboundMcpSettings,
): SettingsDraft {
  return {
    enabled: settings.enabled,
    origins: settings.allowedOrigins.join('\n'),
    requestSize: String(settings.maxRequestSize),
    responseSize: String(settings.maxResponseSize),
    concurrentCalls: String(settings.maxConcurrentCalls),
    callsPerMinute: String(settings.callsPerMinute),
    readTimeoutMs: String(settings.readTimeoutMs),
  };
}

export function InboundMcpPanel() {
  const { t } = useTranslation('agent');
  const queryClient = useQueryClient();
  // Only changing the settings takes it: the addresses and the prompt are there to copy.
  const locked = useActionAccess({ permission: 'agent.inbound_mcp.manage' }).disabled;
  const credentialPages = useOpenablePages(CREDENTIAL_PAGES, (page) => page.to);
  const settingsQuery = useQuery({
    queryKey: ['agent', 'inbound-mcp', 'settings'],
    queryFn: inboundMcpApi.getSettings,
    retry: false,
  });
  const instanceQuery = useQuery({
    queryKey: ['instance'],
    queryFn: instanceApi.get,
    staleTime: 300_000,
  });
  const connectionsQuery = useQuery({
    queryKey: ['agent', 'inbound-mcp', 'oauth-connections'],
    queryFn: inboundMcpApi.listOAuthConnections,
    retry: false,
  });
  const [draft, setDraft] = React.useState<SettingsDraft | null>(null);

  React.useEffect(() => {
    if (settingsQuery.data) setDraft(createDraft(settingsQuery.data.settings));
  }, [settingsQuery.data]);

  const save = useMutation({
    mutationFn: inboundMcpApi.updateSettings,
    onSuccess: (data) => {
      queryClient.setQueryData(['agent', 'inbound-mcp', 'settings'], data);
      setDraft(createDraft(data.settings));
      toast.success(t('settings.inbound_mcp.saved'));
    },
    onError: (error) => toast.error(String(error)),
  });
  const revoke = useMutation({
    mutationFn: inboundMcpApi.revokeOAuthConnection,
    onSuccess: async () => {
      await queryClient.invalidateQueries({
        queryKey: ['agent', 'inbound-mcp', 'oauth-connections'],
      });
      toast.success(t('settings.inbound_mcp.oauth.revoked'));
    },
    onError: (error) => toast.error(String(error)),
  });

  if (settingsQuery.isLoading) return <ProductState variant="loading" />;
  if (settingsQuery.isError) {
    return <ProductState variant="error" error={settingsQuery.error} />;
  }
  if (!settingsQuery.data || !draft) return null;

  const publicBaseUrl = resolvePublicBaseUrl(
    instanceQuery.data?.externalUrl,
    window.location.origin,
  );
  const endpointUrl = resolvePublicUrl(
    settingsQuery.data.endpoint,
    publicBaseUrl,
  );
  const oauthUrls = {
    protectedResourceMetadata: resolvePublicUrl(
      settingsQuery.data.oauth.protectedResourceMetadata,
      publicBaseUrl,
    ),
    authorizationServerMetadata: resolvePublicUrl(
      settingsQuery.data.oauth.authorizationServerMetadata,
      publicBaseUrl,
    ),
    registrationEndpoint: resolvePublicUrl(
      settingsQuery.data.oauth.registrationEndpoint,
      publicBaseUrl,
    ),
    tokenEndpoint: resolvePublicUrl(
      settingsQuery.data.oauth.tokenEndpoint,
      publicBaseUrl,
    ),
  };

  const submit = (event: React.FormEvent) => {
    event.preventDefault();
    const maxRequestSize = Number(draft.requestSize);
    const maxResponseSize = Number(draft.responseSize);
    const maxConcurrentCalls = Number(draft.concurrentCalls);
    const callsPerMinute = Number(draft.callsPerMinute);
    const readTimeoutMs = Number(draft.readTimeoutMs);
    const values = [
      maxRequestSize,
      maxResponseSize,
      maxConcurrentCalls,
      callsPerMinute,
      readTimeoutMs,
    ];
    if (
      values.some(
        (value) => value === undefined || !Number.isInteger(value) || value <= 0,
      )
    ) {
      toast.error(t('settings.inbound_mcp.invalid_limits'));
      return;
    }
    save.mutate({
      enabled: draft.enabled,
      allowedOrigins: draft.origins
        .split(/[,\n]/)
        .map((value) => value.trim())
        .filter(Boolean),
      maxRequestSize: maxRequestSize,
      maxResponseSize: maxResponseSize,
      maxConcurrentCalls: maxConcurrentCalls,
      callsPerMinute: callsPerMinute,
      readTimeoutMs: readTimeoutMs,
    });
  };

  return (
    <form className="grid gap-0" onSubmit={submit}>
      <section className="pb-5">
        <div className="flex flex-wrap items-start gap-4">
          <div className="min-w-0 flex-1">
            <div className="flex items-center gap-2">
              <ServerCog className="h-4 w-4 text-indigo" />
              <h2 className={uiSectionTitleClass}>
                {t('settings.inbound_mcp.title')}
              </h2>
            </div>
            <p className="mt-1 text-xs leading-5 text-tx-3">
              {t('settings.inbound_mcp.description')}
            </p>
          </div>
          <div className="flex items-center gap-2">
            <Label htmlFor="inbound-mcp-enabled" className="text-xs text-tx-2">
              {t('settings.inbound_mcp.enabled')}
            </Label>
            <Switch
              id="inbound-mcp-enabled"
              checked={draft.enabled}
              disabled={locked}
              onCheckedChange={(enabled) =>
                setDraft((value) => (value ? { ...value, enabled } : value))
              }
            />
          </div>
        </div>
        <div className="mt-4 grid gap-4 lg:grid-cols-[minmax(0,1fr)_auto]">
          <EndpointField
            label={t('settings.inbound_mcp.endpoint')}
            value={endpointUrl}
          />
          <div>
            <Label className="text-xs text-tx-2">
              {t('settings.inbound_mcp.protocols')}
            </Label>
            {/* As high as the endpoint field beside it, the versions centred on it. */}
            <div className="mt-1.5 flex min-h-9 flex-wrap items-center gap-1.5">
              {settingsQuery.data.protocolVersions.map((version) => (
                <Badge key={version} variant="outline" className="font-mono">
                  {version}
                </Badge>
              ))}
            </div>
          </div>
        </div>
      </section>

      <section className="border-t border-bd-0 py-5">
        <div className="flex items-start gap-3">
          <Link2 className="mt-1 h-4 w-4 text-indigo" />
          <div>
            <h3 className={uiSectionTitleClass}>
              {t('settings.inbound_mcp.oauth.discovery_title')}
            </h3>
            <p className="mt-1 text-xs leading-5 text-tx-3">
              {t('settings.inbound_mcp.oauth.discovery_description')}
            </p>
          </div>
        </div>
        <div className="mt-4 grid gap-3 lg:grid-cols-2">
          <EndpointField
            label={t('settings.inbound_mcp.oauth.protected_metadata')}
            value={oauthUrls.protectedResourceMetadata}
          />
          <EndpointField
            label={t('settings.inbound_mcp.oauth.authorization_metadata')}
            value={oauthUrls.authorizationServerMetadata}
          />
          <EndpointField
            label={t('settings.inbound_mcp.oauth.registration_endpoint')}
            value={oauthUrls.registrationEndpoint}
          />
          <EndpointField
            label={t('settings.inbound_mcp.oauth.token_endpoint')}
            value={oauthUrls.tokenEndpoint}
          />
        </div>
      </section>

      <section className="border-t border-bd-0 py-5">
        <div className="flex items-start gap-3">
          <ShieldCheck className="mt-1 h-4 w-4 text-[var(--green-ink)]" />
          <div>
            <h3 className={uiSectionTitleClass}>
              {t('settings.inbound_mcp.security.title')}
            </h3>
            <p className="mt-1 text-xs leading-5 text-tx-3">
              {t('settings.inbound_mcp.security.description')}
            </p>
          </div>
        </div>
        <div className="mt-4 grid gap-4 lg:grid-cols-2">
          <Field
            label={t('settings.inbound_mcp.allowed_origins')}
            htmlFor="inbound-mcp-allowed-origins"
          >
            <Textarea
              id="inbound-mcp-allowed-origins"
              value={draft.origins}
              disabled={locked}
              onChange={(event) =>
                setDraft((value) =>
                  value ? { ...value, origins: event.target.value } : value,
                )
              }
              placeholder="https://mcp-client.example.com"
              className="min-h-24 font-mono text-xs"
            />
            <p className="mt-1 text-xs leading-5 text-tx-3">
              {t('settings.inbound_mcp.origins_hint')}
            </p>
          </Field>
          <div className="lg:pl-4">
            <div className="flex items-center gap-2 text-sm font-strong text-tx-1">
              <KeyRound className="h-4 w-4 text-amber-soft" />
              {t('settings.inbound_mcp.credentials.title')}
            </div>
            <p className="mt-2 text-xs leading-5 text-tx-3">
              {t('settings.inbound_mcp.credentials.description')}
            </p>
            {credentialPages.length > 0 && (
              <div className="mt-3 flex flex-wrap gap-2">
                {credentialPages.map((page) => (
                  <Button key={page.to} asChild size="sm" variant="outline">
                    <Link to={page.to}>
                      {t(page.key)}
                      <ExternalLink />
                    </Link>
                  </Button>
                ))}
              </div>
            )}
          </div>
        </div>
      </section>

      <section className="border-t border-bd-0 py-5">
        <div className="flex items-center gap-2">
          <RefreshCw className="h-4 w-4 text-indigo" />
          <h3 className={uiSectionTitleClass}>
            {t('settings.inbound_mcp.runtime.title')}
          </h3>
        </div>
        <p className="mt-1 text-xs leading-5 text-tx-3">
          {t('settings.inbound_mcp.runtime.description')}
        </p>
        <div className="mt-4 grid gap-4 sm:grid-cols-2 xl:grid-cols-5">
          <NumberField label={t('settings.inbound_mcp.runtime.request_size')} value={draft.requestSize} min={1} max={settingsQuery.data.hardMaxSize} disabled={locked} onChange={(requestSize) => setDraft({ ...draft, requestSize })} />
          <NumberField label={t('settings.inbound_mcp.runtime.response_size')} value={draft.responseSize} min={1} max={settingsQuery.data.hardMaxSize} disabled={locked} onChange={(responseSize) => setDraft({ ...draft, responseSize })} />
          <NumberField label={t('settings.inbound_mcp.runtime.concurrent_calls')} value={draft.concurrentCalls} min={1} max={128} disabled={locked} onChange={(concurrentCalls) => setDraft({ ...draft, concurrentCalls })} />
          <NumberField label={t('settings.inbound_mcp.runtime.calls_per_minute')} value={draft.callsPerMinute} min={1} max={10_000} disabled={locked} onChange={(callsPerMinute) => setDraft({ ...draft, callsPerMinute })} />
          <NumberField label={t('settings.inbound_mcp.runtime.read_timeout')} value={draft.readTimeoutMs} min={100} max={300_000} disabled={locked} onChange={(readTimeoutMs) => setDraft({ ...draft, readTimeoutMs })} />
        </div>
      </section>

      <OAuthConnections
        connections={connectionsQuery.data ?? []}
        loading={connectionsQuery.isLoading}
        error={connectionsQuery.error}
        revoking={revoke.isPending}
        locked={locked}
        onRevoke={(id) => revoke.mutate(id)}
      />

      <ConnectPrompt endpoint={endpointUrl} />

      <div className="flex justify-end border-t border-bd-0 pt-4">
        <Button type="submit" disabled={locked || save.isPending}>
          <Save />
          {save.isPending
            ? t('common.saving')
            : t('settings.inbound_mcp.save')}
        </Button>
      </div>
    </form>
  );
}
