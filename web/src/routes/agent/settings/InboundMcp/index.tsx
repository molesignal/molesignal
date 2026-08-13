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
import { ProductState } from '@/product/states';
import { Badge } from '@/shell/ui/badge';
import { Button } from '@/shell/ui/button';
import { Label } from '@/shell/ui/label';
import { toast } from '@/shell/ui/sonner';
import { Switch } from '@/shell/ui/switch';
import { Textarea } from '@/shell/ui/textarea';

import {
  CopyButton,
  EndpointField,
  Field,
  NumberField,
  OAuthConnections,
} from './components';

interface SettingsDraft {
  enabled: boolean;
  origins: string;
  requestBytes: string;
  responseBytes: string;
  concurrentCalls: string;
  callsPerMinute: string;
  readTimeoutMs: string;
}

function createDraft(
  settings: inboundMcpApi.InboundMcpSettings,
): SettingsDraft {
  return {
    enabled: settings.enabled,
    origins: settings.allowed_origins.join('\n'),
    requestBytes: String(settings.max_request_bytes),
    responseBytes: String(settings.max_response_bytes),
    concurrentCalls: String(settings.max_concurrent_calls),
    callsPerMinute: String(settings.calls_per_minute),
    readTimeoutMs: String(settings.read_timeout_ms),
  };
}

export function InboundMcpPanel() {
  const { t } = useTranslation('agent');
  const queryClient = useQueryClient();
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
    instanceQuery.data?.external_url,
    window.location.origin,
  );
  const endpointUrl = resolvePublicUrl(
    settingsQuery.data.endpoint,
    publicBaseUrl,
  );
  const oauthUrls = {
    protectedResourceMetadata: resolvePublicUrl(
      settingsQuery.data.oauth.protected_resource_metadata,
      publicBaseUrl,
    ),
    authorizationServerMetadata: resolvePublicUrl(
      settingsQuery.data.oauth.authorization_server_metadata,
      publicBaseUrl,
    ),
    registrationEndpoint: resolvePublicUrl(
      settingsQuery.data.oauth.registration_endpoint,
      publicBaseUrl,
    ),
    tokenEndpoint: resolvePublicUrl(
      settingsQuery.data.oauth.token_endpoint,
      publicBaseUrl,
    ),
  };

  const submit = (event: React.FormEvent) => {
    event.preventDefault();
    const maxRequestBytes = Number(draft.requestBytes);
    const maxResponseBytes = Number(draft.responseBytes);
    const maxConcurrentCalls = Number(draft.concurrentCalls);
    const callsPerMinute = Number(draft.callsPerMinute);
    const readTimeoutMs = Number(draft.readTimeoutMs);
    const values = [
      maxRequestBytes,
      maxResponseBytes,
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
      allowed_origins: draft.origins
        .split(/[,\n]/)
        .map((value) => value.trim())
        .filter(Boolean),
      max_request_bytes: maxRequestBytes,
      max_response_bytes: maxResponseBytes,
      max_concurrent_calls: maxConcurrentCalls,
      calls_per_minute: callsPerMinute,
      read_timeout_ms: readTimeoutMs,
    });
  };

  return (
    <form className="grid gap-4" onSubmit={submit}>
      <section className="overflow-hidden rounded-lg border border-bd-0 bg-bg-1">
        <div className="flex flex-wrap items-start gap-4 border-b border-bd-0 px-4 py-3">
          <div className="min-w-0 flex-1">
            <div className="flex items-center gap-2">
              <ServerCog className="h-4 w-4 text-indigo" />
              <h2 className="font-strong text-tx-0">
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
              onCheckedChange={(enabled) =>
                setDraft((value) => (value ? { ...value, enabled } : value))
              }
            />
          </div>
        </div>
        <div className="grid gap-4 p-4 lg:grid-cols-[minmax(0,1fr)_auto]">
          <div>
            <Label className="text-xs text-tx-3">
              {t('settings.inbound_mcp.endpoint')}
            </Label>
            <div className="mt-1 flex min-w-0 items-center gap-2 rounded-md border border-bd-0 bg-bg-2 px-3 py-2">
              <code className="min-w-0 flex-1 truncate text-xs text-tx-1">
                {endpointUrl}
              </code>
              <CopyButton value={endpointUrl} />
            </div>
          </div>
          <div>
            <Label className="text-xs text-tx-3">
              {t('settings.inbound_mcp.protocols')}
            </Label>
            <div className="mt-2 flex flex-wrap gap-1.5">
              {settingsQuery.data.protocol_versions.map((version) => (
                <Badge key={version} variant="outline" className="font-mono">
                  {version}
                </Badge>
              ))}
            </div>
          </div>
        </div>
      </section>

      <section className="rounded-lg border border-bd-0 bg-bg-1 p-4">
        <div className="flex items-start gap-3">
          <Link2 className="mt-0.5 h-4 w-4 text-indigo" />
          <div>
            <h3 className="font-strong text-tx-0">
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

      <section className="rounded-lg border border-bd-0 bg-bg-1 p-4">
        <div className="flex items-start gap-3">
          <ShieldCheck className="mt-0.5 h-4 w-4 text-green-soft" />
          <div>
            <h3 className="font-strong text-tx-0">
              {t('settings.inbound_mcp.security.title')}
            </h3>
            <p className="mt-1 text-xs leading-5 text-tx-3">
              {t('settings.inbound_mcp.security.description')}
            </p>
          </div>
        </div>
        <div className="mt-4 grid gap-4 lg:grid-cols-2">
          <Field label={t('settings.inbound_mcp.allowed_origins')}>
            <Textarea
              value={draft.origins}
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
          <div className="rounded-md border border-bd-0 bg-bg-2 p-3">
            <div className="flex items-center gap-2 text-sm font-strong text-tx-1">
              <KeyRound className="h-4 w-4 text-amber-soft" />
              {t('settings.inbound_mcp.credentials.title')}
            </div>
            <p className="mt-2 text-xs leading-5 text-tx-3">
              {t('settings.inbound_mcp.credentials.description')}
            </p>
            <div className="mt-3 flex flex-wrap gap-2">
              <Button asChild size="sm" variant="outline">
                <Link to="/iam/api-tokens">
                  {t('settings.inbound_mcp.credentials.api_tokens')}
                  <ExternalLink />
                </Link>
              </Button>
              <Button asChild size="sm" variant="outline">
                <Link to="/iam/service-accounts">
                  {t('settings.inbound_mcp.credentials.service_accounts')}
                  <ExternalLink />
                </Link>
              </Button>
            </div>
          </div>
        </div>
      </section>

      <section className="rounded-lg border border-bd-0 bg-bg-1 p-4">
        <div className="flex items-center gap-2">
          <RefreshCw className="h-4 w-4 text-indigo" />
          <h3 className="font-strong text-tx-0">
            {t('settings.inbound_mcp.runtime.title')}
          </h3>
        </div>
        <p className="mt-1 text-xs leading-5 text-tx-3">
          {t('settings.inbound_mcp.runtime.description')}
        </p>
        <div className="mt-4 grid gap-4 sm:grid-cols-2 xl:grid-cols-5">
          <NumberField label={t('settings.inbound_mcp.runtime.request_bytes')} value={draft.requestBytes} min={1_024} max={settingsQuery.data.hard_max_body_bytes} onChange={(requestBytes) => setDraft({ ...draft, requestBytes })} />
          <NumberField label={t('settings.inbound_mcp.runtime.response_bytes')} value={draft.responseBytes} min={1_024} max={settingsQuery.data.hard_max_body_bytes} onChange={(responseBytes) => setDraft({ ...draft, responseBytes })} />
          <NumberField label={t('settings.inbound_mcp.runtime.concurrent_calls')} value={draft.concurrentCalls} min={1} max={128} onChange={(concurrentCalls) => setDraft({ ...draft, concurrentCalls })} />
          <NumberField label={t('settings.inbound_mcp.runtime.calls_per_minute')} value={draft.callsPerMinute} min={1} max={10_000} onChange={(callsPerMinute) => setDraft({ ...draft, callsPerMinute })} />
          <NumberField label={t('settings.inbound_mcp.runtime.read_timeout')} value={draft.readTimeoutMs} min={100} max={300_000} onChange={(readTimeoutMs) => setDraft({ ...draft, readTimeoutMs })} />
        </div>
      </section>

      <OAuthConnections
        connections={connectionsQuery.data ?? []}
        loading={connectionsQuery.isLoading}
        error={connectionsQuery.error}
        revoking={revoke.isPending}
        onRevoke={(id) => revoke.mutate(id)}
      />

      <div className="flex justify-end">
        <Button type="submit" disabled={save.isPending}>
          <Save />
          {save.isPending
            ? t('common.saving')
            : t('settings.inbound_mcp.save')}
        </Button>
      </div>
    </form>
  );
}
