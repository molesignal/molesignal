import { Check, ClipboardCheck, ExternalLink, RefreshCw, ShieldAlert, Wrench } from 'lucide-react';
import type * as React from 'react';
import { Trans, useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import type * as oauthApi from '@/api/inboundMcpOAuth';
import type { MeProfile } from '@/api/me';
import { LogoMark } from '@/shell/LogoMark';
import { Avatar, AvatarFallback, AvatarImage } from '@/shell/ui/avatar';
import { FittedSelect } from '@/shell/ui/fitted-select';

/** The first letters of a name, for a mark where there is no picture. */
function initials(name: string): string {
  const words = name.trim().split(/\s+/).filter(Boolean);
  const letters = words.length > 1 ? [words[0]![0], words[1]![0]] : [...(words[0] ?? '?')].slice(0, 2);
  return letters.join('').toUpperCase();
}

/** The client and MoleSignal side by side, joined by the connection being asked for. */
export function ConnectionMarks({ client }: { client: string }) {
  return (
    <div className="flex items-center gap-3" aria-hidden="true" data-oauth-marks>
      <span className="grid h-12 w-12 place-items-center rounded-xl bg-bg-1 font-sans text-[15px] font-strong text-tx-1 shadow-login">
        {initials(client)}
      </span>
      <span className="flex items-center gap-1.5">
        <span className="w-6 border-t-2 border-dotted border-bd-1" />
        <span className="grid h-6 w-6 place-items-center rounded-full bg-[var(--green-tint)] text-[var(--green-ink)]">
          <Check className="h-3.5 w-3.5" strokeWidth={3} />
        </span>
        <span className="w-6 border-t-2 border-dotted border-bd-1" />
      </span>
      <span className="grid h-12 w-12 place-items-center rounded-xl bg-bg-1 shadow-login">
        <LogoMark size={30} />
      </span>
    </div>
  );
}

/**
 * Who is granting the access, and in which organization: the grant is bound to
 * both. Someone in several organizations picks the one here.
 */
export function AccountRow({
  profile,
  organization,
  organizations,
  switching,
  onSwitch,
}: {
  profile: MeProfile | undefined;
  organization: oauthApi.OAuthAuthorizationInspection['organization'];
  organizations: { id: string; name: string }[];
  switching: boolean;
  onSwitch: (id: string) => void;
}) {
  const { t } = useTranslation('agent');
  const name = profile?.display_name || profile?.email || '';

  return (
    <div className="mt-5 flex items-center gap-3 rounded-xl bg-bg-2 px-3.5 py-3" data-oauth-account>
      <Avatar className="h-9 w-9">
        {profile?.avatar_url ? <AvatarImage src={profile.avatar_url} alt="" /> : null}
        <AvatarFallback className="bg-indigo-dim font-sans text-xs font-strong text-indigo-soft">
          {initials(name || '?')}
        </AvatarFallback>
      </Avatar>
      <div className="min-w-0 flex-1">
        <div className="truncate font-sans text-[13px] font-strong text-tx-0">{name}</div>
        {profile?.display_name && profile.email ? (
          <div className="truncate font-sans text-xs text-tx-2">{profile.email}</div>
        ) : null}
      </div>
      <div className="flex min-w-0 shrink-0 flex-col items-end gap-0.5">
        <span className="font-sans text-xs text-tx-3">{t('oauth.organization')}</span>
        {organizations.length > 1 ? (
          <FittedSelect
            value={organization.id}
            onValueChange={onSwitch}
            options={organizations.map((org) => ({ value: org.id, label: org.name }))}
            maxWidth={180}
            disabled={switching}
            aria-label={t('oauth.switch_organization')}
            className="h-7 text-xs"
          />
        ) : (
          <span className="max-w-[180px] truncate font-sans text-xs font-strong text-tx-1">
            {organization.name}
          </span>
        )}
      </div>
    </div>
  );
}

function Permission({
  icon,
  title,
  body,
}: {
  icon: React.ReactNode;
  title: string;
  body: string;
}) {
  return (
    <li className="flex gap-3 py-3">
      <span className="mt-0.5 grid h-7 w-7 shrink-0 place-items-center rounded-lg bg-indigo-dim text-indigo-soft">
        {icon}
      </span>
      <div className="min-w-0">
        <div className="font-sans text-[13px] font-strong text-tx-0">{title}</div>
        <p className="mt-0.5 font-sans text-xs leading-relaxed text-tx-2">{body}</p>
      </div>
    </li>
  );
}

/** What the grant lets the client do, said once per thing it can do. */
export function PermissionList({ client, scope }: { client: string; scope: string }) {
  const { t } = useTranslation('agent');
  const scopes = new Set(scope.split(/\s+/).filter(Boolean));

  return (
    <section className="mt-6" data-oauth-permissions>
      <h2 className="font-sans text-xs font-strong text-tx-1">{t('oauth.allows', { client })}</h2>
      <ul className="mt-1 divide-y divide-bd-0">
        <Permission
          icon={<Wrench className="h-3.5 w-3.5" />}
          title={t('oauth.permission.tools_title')}
          body={t('oauth.permission.tools_body')}
        />
        <Permission
          icon={<ClipboardCheck className="h-3.5 w-3.5" />}
          title={t('oauth.permission.changes_title')}
          body={t('oauth.permission.changes_body')}
        />
        {scopes.has('offline_access') ? (
          <Permission
            icon={<RefreshCw className="h-3.5 w-3.5" />}
            title={t('oauth.permission.offline_title')}
            body={t('oauth.permission.offline_body')}
          />
        ) : null}
      </ul>
    </section>
  );
}

/**
 * Whether the client is who it says it is. A client that registered itself is
 * named by itself; one with a metadata document is named by its own domain.
 */
export function TrustNote({ client }: { client: oauthApi.OAuthAuthorizationInspection['client'] }) {
  const { t } = useTranslation('agent');
  const host = client.metadata_document ? hostOf(client.client_id) : null;

  return (
    <section className="mt-4 rounded-xl bg-bg-2 px-4 py-3.5" data-oauth-trust>
      <h2 className="flex items-center gap-1.5 font-sans text-xs font-strong text-tx-1">
        <ShieldAlert className="h-3.5 w-3.5 text-[var(--yellow-ink)]" />
        {t('oauth.trust_title', { client: client.client_name })}
      </h2>
      <p className="mt-1.5 font-sans text-xs leading-relaxed text-tx-2">
        <Trans
          i18nKey={host ? 'agent:oauth.trust_metadata' : 'agent:oauth.trust_registered'}
          values={{ client: client.client_name, host }}
          components={{
            // Not `link`: that is an HTML element without content, and the text inside would be lost.
            revoke: <Link to="/agent/settings/inbound-mcp" className="text-[var(--blue-ink)] hover:underline" />,
          }}
        />
      </p>
      {client.client_uri ? (
        <a
          href={client.client_uri}
          target="_blank"
          rel="noreferrer"
          className="mt-2 inline-flex items-center gap-1 font-sans text-xs text-[var(--blue-ink)] hover:underline"
        >
          {t('oauth.client_website')} <ExternalLink className="h-3 w-3" />
        </a>
      ) : null}
    </section>
  );
}

function hostOf(url: string): string | null {
  try {
    return new URL(url).host || null;
  } catch {
    return null;
  }
}

/** Where the browser goes after a decision: the origin, as the reader can judge it. */
export function redirectOrigin(redirectUri: string): string {
  try {
    const url = new URL(redirectUri);
    return url.protocol === 'http:' || url.protocol === 'https:' ? url.origin : `${url.protocol}//${url.host}`;
  } catch {
    return redirectUri;
  }
}

/** The identifiers the grant is bound to, for the reader who wants to check them. */
export function GrantDetails({ details }: { details: oauthApi.OAuthAuthorizationInspection }) {
  const { t } = useTranslation('agent');
  const rows: [string, string][] = [
    [t('oauth.client_id'), details.client.client_id],
    [t('oauth.resource'), details.resource],
    [t('oauth.scopes'), details.scope],
    [t('oauth.redirect_uri'), details.redirect_uri],
  ];

  return (
    <details className="group mt-4 font-sans text-xs" data-oauth-details>
      <summary className="cursor-pointer list-none text-center text-tx-3 hover:text-tx-1 [&::-webkit-details-marker]:hidden">
        {t('oauth.details')}
      </summary>
      <dl className="mt-3 space-y-2 rounded-xl bg-bg-2 px-4 py-3">
        {rows.map(([label, value]) => (
          <div key={label}>
            <dt className="text-xs text-tx-3">{label}</dt>
            <dd className="mt-0.5 break-all text-tx-1">{value}</dd>
          </div>
        ))}
        <p className="pt-1 text-xs leading-relaxed text-tx-3">{t('oauth.binding')}</p>
      </dl>
    </details>
  );
}
