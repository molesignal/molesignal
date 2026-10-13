import { useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';

import * as instanceApi from '@/api/instance';
import { resolvePublicBaseUrl } from '@/lib/publicUrl';
import { uiLabelClass } from '@/shell/chrome';
import { CopyField } from '@/shell/CopyField';

import { specOf, type ConnectorKind } from './kinds';

/** Where a push connector of this kind listens, as the other service has to be given it. */
export function usePushEndpoint(kind: ConnectorKind): string {
  const instance = useQuery({ queryKey: ['instance'], queryFn: instanceApi.get, staleTime: 300_000 });
  const base = resolvePublicBaseUrl(instance.data?.external_url, window.location.origin);

  return `${base}${specOf(kind).pushPath ?? ''}`;
}

/**
 * What to give the other service so that it pushes here: the endpoint, the
 * token, and for services configured by one line, that line with both in it.
 * Without `token` only the endpoint is shown: a saved token cannot be read back.
 */
export function PushSetup({ kind, token }: { kind: ConnectorKind; token?: string | undefined }) {
  const { t } = useTranslation('pipelines');
  const endpoint = usePushEndpoint(kind);
  // Each service carries the token its own way; the receiver takes all three.
  const line =
    !token
      ? null
      : kind === 'cloudflare_logpush'
        ? { label: 'destination', value: `${endpoint}?header_X-Connector-Token=${token}` }
        : kind === 'heroku_drain'
          ? { label: 'command', value: `heroku drains:add "${endpoint}?token=${token}"` }
          : null;

  return (
    <div data-connector-push-setup className="grid gap-[12px]">
      {token && (
        <p className="m-0 font-sans text-sm leading-relaxed text-tx-1">{t(`connectors.push.steps.${kind}`)}</p>
      )}
      <Row label={t('connectors.push.endpoint')}>
        <CopyField value={endpoint} copyLabel={t('connectors.push.copy_endpoint')} />
      </Row>
      {token && (
        <Row label={t('connectors.push.token')}>
          <CopyField value={token} copyLabel={t('connectors.push.copy_token')} />
        </Row>
      )}
      {line && (
        <Row label={t(`connectors.push.${line.label}`)}>
          <CopyField value={line.value} copyLabel={t(`connectors.push.copy_${line.label}`)} />
        </Row>
      )}
      {token && (
        <p className="m-0 font-sans text-xs leading-relaxed text-yellow">{t('connectors.push.once')}</p>
      )}
    </div>
  );
}

function Row({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="grid min-w-0 gap-1.5">
      <span className={uiLabelClass}>{label}</span>
      {children}
    </div>
  );
}
