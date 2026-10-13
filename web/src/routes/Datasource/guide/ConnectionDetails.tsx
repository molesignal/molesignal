import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { CopyField } from '@/shell/CopyField';

import { TokenField } from '../credentials/TokenField';
import type { IntakeContext } from '../intakeContext';
import type { Signal } from '../sources';

export interface GuideEndpoint {
  /** The signal this endpoint takes, named when the source sends several. */
  signal: Signal | null;
  url: string;
}

/**
 * Where a source sends its data and what it authenticates with: one line per
 * value, each ready to copy. The commands of the next step carry the same values.
 */
export function ConnectionDetails({
  endpoints,
  context,
  application,
}: {
  endpoints: readonly GuideEndpoint[];
  context: IntakeContext;
  /** The application field of a RUM source, between its endpoint and its token. */
  application?: React.ReactNode;
}) {
  const { t } = useTranslation('onboarding');

  return (
    <dl
      data-guide-connection
      className="m-0 grid gap-y-[10px] sm:grid-cols-[minmax(112px,max-content)_minmax(0,1fr)] sm:gap-x-[20px]"
    >
      {endpoints.map((endpoint) => (
        <DetailRow
          key={endpoint.signal ?? 'endpoint'}
          label={
            endpoint.signal
              ? t('datasource_page.endpoint_for', {
                  signal: t(`datasource_page.signals.${endpoint.signal}`),
                })
              : t('datasource.endpoint')
          }
        >
          <CopyField value={endpoint.url} copyLabel={t('datasource.copy_endpoint')} />
        </DetailRow>
      ))}
      {application && (
        <DetailRow label={t('datasource_page.rum_application_id')} htmlFor="rum-application-id">
          {application}
        </DetailRow>
      )}
      <DetailRow
        label={context.isRum ? t('datasource.rum_client_token') : t('datasource.intake_token')}
      >
        <TokenField context={context} />
      </DetailRow>
    </dl>
  );
}

function DetailRow({
  label,
  htmlFor,
  children,
}: {
  label: string;
  htmlFor?: string;
  children: React.ReactNode;
}) {
  return (
    <>
      <dt className="flex items-center font-sans text-sm text-tx-2 sm:h-9">
        {htmlFor ? <label htmlFor={htmlFor}>{label}</label> : label}
      </dt>
      <dd className="m-0 min-w-0">{children}</dd>
    </>
  );
}
