import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useSearchParams } from 'react-router-dom';

import * as oauthApi from '@/api/inboundMcpOAuth';
import * as meApi from '@/api/me';
import { toApiError } from '@/lib/http';
import { ProductState } from '@/product/states';
import { AuthStage } from '@/shell/signin/AuthStage';
import { Button } from '@/shell/ui/button';
import { toast } from '@/shell/ui/sonner';
import { useCurrentOrgSelection, useOrgStore } from '@/stores/useOrgStore';

import {
  AccountRow,
  ConnectionMarks,
  GrantDetails,
  PermissionList,
  redirectOrigin,
  TrustNote,
} from './consentParts';

/**
 * The page an MCP client sends the browser to for access: who is asking, the
 * account and organization the access is for, what it allows, whether to trust
 * the client, and where the browser goes next. It sits on the sign-in stage, as
 * the consent pages of Google and GitHub sit on their sign-in pages.
 */
export function InboundMcpOAuthAuthorize() {
  const { t } = useTranslation('agent');
  const [params] = useSearchParams();
  const queryClient = useQueryClient();
  const query = params.toString();
  const request = React.useMemo(() => authorizationRequest(params), [params]);
  const selection = useCurrentOrgSelection();
  const loadOrgs = useOrgStore((state) => state.loadOrgs);
  const switchOrg = useOrgStore((state) => state.switchOrg);

  React.useEffect(() => {
    void loadOrgs();
  }, [loadOrgs]);

  const inspection = useQuery({
    queryKey: ['inbound-mcp', 'oauth', 'authorization', selection.currentOrgId, query],
    queryFn: () => oauthApi.inspectAuthorization(query),
    enabled: request !== null,
    retry: false,
  });
  const profile = useQuery({ queryKey: ['me', 'profile'], queryFn: meApi.profile, retry: false });

  const switching = useMutation({
    mutationFn: (id: string) => switchOrg(id, { queryClient }),
    onError: (error) => toast.error(toApiError(error).message),
  });
  const decision = useMutation({
    mutationFn: (approve: boolean) => {
      if (!request) throw new Error('Invalid OAuth authorization request');
      return oauthApi.decideAuthorization(request, approve);
    },
    onSuccess: (redirectTo) => window.location.replace(redirectTo),
    onError: (error) => toast.error(toApiError(error).message),
  });

  if (!request) {
    return (
      <ConsentStage>
        <ProductState variant="error" error={new Error(t('oauth.invalid_request'))} />
      </ConsentStage>
    );
  }
  if (inspection.isLoading || switching.isPending) {
    return (
      <ConsentStage>
        <ProductState variant="loading" />
      </ConsentStage>
    );
  }
  if (inspection.isError || !inspection.data) {
    return (
      <ConsentStage>
        <ProductState variant="error" error={inspection.error} />
      </ConsentStage>
    );
  }

  const details = inspection.data;
  const client = details.client.client_name;
  // An organization entered only from the platform cannot grant MCP access.
  const organizations = selection.orgOptions.filter((org) => !org.system && !org.disabled);

  return (
    <ConsentStage client={client}>
      {/* The client's name on a line of its own: a balanced heading would break a Chinese sentence mid-word. */}
      <h1 className="m-0 text-center font-sans" data-oauth-title>
        <span className="block text-xl font-display tracking-tight text-tx-0">{client}</span>
        <span className="mt-1 block text-[13px] font-strong text-tx-2">{t('oauth.title')}</span>
      </h1>

      <AccountRow
        profile={profile.data}
        organization={details.organization}
        organizations={organizations}
        switching={switching.isPending}
        onSwitch={(id) => {
          if (id !== details.organization.id) switching.mutate(id);
        }}
      />

      <PermissionList client={client} scope={details.scope} />
      <TrustNote client={details.client} />

      <div className="mt-6 grid grid-cols-2 gap-2">
        <Button
          variant="outline"
          size="lg"
          disabled={decision.isPending}
          onClick={() => decision.mutate(false)}
        >
          {t('oauth.cancel')}
        </Button>
        <Button size="lg" disabled={decision.isPending} onClick={() => decision.mutate(true)}>
          {t('oauth.allow')}
        </Button>
      </div>

      <p className="mt-3 text-center font-sans text-xs text-tx-3" data-oauth-redirect>
        {t('oauth.redirect', { origin: redirectOrigin(details.redirect_uri) })}
      </p>

      <GrantDetails details={details} />
    </ConsentStage>
  );
}

/**
 * The sign-in stage with the connection marks over one card. A loading or error
 * state brings its own card, so it stands on the stage alone.
 */
function ConsentStage({ client, children }: { client?: string; children: React.ReactNode }) {
  if (!client) {
    return <AuthStage>{children}</AuthStage>;
  }

  return (
    <AuthStage>
      <ConnectionMarks client={client} />
      <section
        className="ms-auth-card mt-7 flex w-full min-w-0 flex-col rounded-2xl border border-bd-0 bg-bg-1 p-6 shadow-login sm:p-8"
        data-testid="oauth-consent"
      >
        {children}
      </section>
    </AuthStage>
  );
}

function authorizationRequest(params: URLSearchParams): oauthApi.OAuthAuthorizationRequest | null {
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
