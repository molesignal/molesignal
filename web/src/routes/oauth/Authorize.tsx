import { useMutation, useQuery } from '@tanstack/react-query';
import { ArrowRight, ExternalLink, ShieldCheck, X } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useSearchParams } from 'react-router-dom';

import * as oauthApi from '@/api/inboundMcpOAuth';
import { ProductState } from '@/product/states';
import { LogoMark } from '@/shell/LogoMark';
import { Badge } from '@/shell/ui/badge';
import { Button } from '@/shell/ui/button';
import { toast } from '@/shell/ui/sonner';

export function InboundMcpOAuthAuthorize() {
  const { t } = useTranslation('agent');
  const [params] = useSearchParams();
  const query = params.toString();
  const request = React.useMemo(() => authorizationRequest(params), [params]);
  const inspection = useQuery({
    queryKey: ['inbound-mcp', 'oauth', 'authorization', query],
    queryFn: () => oauthApi.inspectAuthorization(query),
    enabled: request !== null,
    retry: false,
  });
  const decision = useMutation({
    mutationFn: (approve: boolean) => {
      if (!request) throw new Error('Invalid OAuth authorization request');
      return oauthApi.decideAuthorization(request, approve);
    },
    onSuccess: (redirectTo) => window.location.replace(redirectTo),
    onError: (error) => toast.error(String(error)),
  });

  if (!request) {
    return (
      <ConsentShell>
        <ProductState
          variant="error"
          error={new Error(t('oauth.invalid_request'))}
        />
      </ConsentShell>
    );
  }
  if (inspection.isLoading) {
    return <ConsentShell><ProductState variant="loading" /></ConsentShell>;
  }
  if (inspection.isError || !inspection.data) {
    return (
      <ConsentShell>
        <ProductState variant="error" error={inspection.error} />
      </ConsentShell>
    );
  }

  const details = inspection.data;
  return (
    <ConsentShell>
      <div className="border-b border-bd-0 px-6 py-5">
        <div className="flex items-start gap-4">
          <div className="grid h-11 w-11 shrink-0 place-items-center rounded-lg border border-indigo/30 bg-indigo/10">
            <LogoMark size={28} />
          </div>
          <div className="min-w-0">
            <p className="text-xs font-strong uppercase tracking-wider text-indigo">
              {t('oauth.eyebrow')}
            </p>
            <h1 className="mt-1 text-xl font-strong text-tx-0">
              {t('oauth.title', { client: details.client.client_name })}
            </h1>
            <p className="mt-2 text-sm leading-6 text-tx-2">
              {t('oauth.description')}
            </p>
          </div>
        </div>
      </div>

      <div className="space-y-5 p-6">
        <div className="rounded-md border border-bd-0 bg-bg-2 p-4">
          <div className="flex items-center gap-2 text-sm font-strong text-tx-1">
            <ShieldCheck className="h-4 w-4 text-green-soft" />
            {t('oauth.access_title')}
          </div>
          <ul className="mt-3 space-y-2 text-sm leading-6 text-tx-2">
            <li>{t('oauth.access_tools')}</li>
            <li>{t('oauth.access_policy')}</li>
            <li>{t('oauth.access_credentials')}</li>
          </ul>
        </div>

        <dl className="grid gap-3 text-sm sm:grid-cols-2">
          <Detail label={t('oauth.client')} value={details.client.client_name} />
          <Detail label={t('oauth.organization')} value={details.organization.name} />
          <Detail label={t('oauth.resource')} value={details.resource} mono wide />
          <div className="sm:col-span-2">
            <dt className="text-xs text-tx-3">{t('oauth.scopes')}</dt>
            <dd className="mt-1.5 flex flex-wrap gap-1.5">
              {details.scope.split(/\s+/).filter(Boolean).map((scope) => (
                <Badge key={scope} variant="outline" className="font-mono">
                  {scope}
                </Badge>
              ))}
            </dd>
          </div>
        </dl>

        {details.client.client_uri && (
          <a
            href={details.client.client_uri}
            target="_blank"
            rel="noreferrer"
            className="inline-flex items-center gap-1.5 text-xs text-indigo hover:text-indigo-soft"
          >
            {t('oauth.client_website')} <ExternalLink className="h-3.5 w-3.5" />
          </a>
        )}

        <div className="flex flex-col-reverse gap-2 border-t border-bd-0 pt-5 sm:flex-row sm:justify-end">
          <Button
            variant="outline"
            disabled={decision.isPending}
            onClick={() => decision.mutate(false)}
          >
            <X /> {t('oauth.deny')}
          </Button>
          <Button
            disabled={decision.isPending}
            onClick={() => decision.mutate(true)}
          >
            {t('oauth.allow')} <ArrowRight />
          </Button>
        </div>
      </div>
    </ConsentShell>
  );
}

function ConsentShell({ children }: { children: React.ReactNode }) {
  const { t } = useTranslation('agent');
  return (
    <main className="grid min-h-screen place-items-center bg-bg-0 px-4 py-10">
      <div className="w-full max-w-2xl overflow-hidden rounded-xl border border-bd-0 bg-bg-1">
        {children}
        <div className="border-t border-bd-0 bg-bg-2 px-6 py-3 text-center text-xs text-tx-3">
          {t('oauth.footer')}
        </div>
      </div>
    </main>
  );
}

function Detail({ label, value, mono = false, wide = false }: { label: string; value: string; mono?: boolean; wide?: boolean }) {
  return (
    <div className={wide ? 'sm:col-span-2' : undefined}>
      <dt className="text-xs text-tx-3">{label}</dt>
      <dd className={`mt-1 break-all text-tx-1 ${mono ? 'font-mono text-xs' : 'font-strong'}`}>
        {value}
      </dd>
    </div>
  );
}

function authorizationRequest(
  params: URLSearchParams,
): oauthApi.OAuthAuthorizationRequest | null {
  const required = [
    'response_type',
    'client_id',
    'redirect_uri',
    'scope',
    'code_challenge',
    'code_challenge_method',
    'resource',
  ] as const;
  if (required.some((key) => !params.get(key))) return null;
  const request: oauthApi.OAuthAuthorizationRequest = {
    response_type: params.get('response_type') ?? '',
    client_id: params.get('client_id') ?? '',
    redirect_uri: params.get('redirect_uri') ?? '',
    scope: params.get('scope') ?? '',
    code_challenge: params.get('code_challenge') ?? '',
    code_challenge_method: params.get('code_challenge_method') ?? '',
    resource: params.get('resource') ?? '',
  };
  const state = params.get('state');
  if (state !== null) request.state = state;
  return request;
}
