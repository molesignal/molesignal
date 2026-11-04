import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import type * as ssoApi from '@/api/sso';
import { ChromeButton } from '@/shell/chrome';
import { StatusWord } from '@/shell/StatusWord';

import { STANDING_TONE, standingOf, testResultPath } from './model';

/**
 * Where a provider stands with a test sign-in, in a word. The word opens the
 * result for those who may read it: it has a person's email and groups.
 */
export function TestStatusWord({
  provider,
  waiting,
  linked,
}: {
  provider: Pick<ssoApi.SsoProvider, 'test'>;
  waiting: boolean;
  linked: boolean;
}) {
  const { t } = useTranslation('settings-admin');

  if (waiting) {
    return <StatusWord tone="blue">{t('sso_providers.test.standing.waiting')}</StatusWord>;
  }

  const standing = standingOf(provider);
  const word = (
    <StatusWord tone={STANDING_TONE[standing.status]}>
      {t(`sso_providers.test.standing.${standing.status}`)}
    </StatusWord>
  );

  if (!linked || !standing.testId) return word;

  return (
    <Link
      to={testResultPath(standing.testId)}
      className="underline-offset-2 hover:underline focus-visible:underline"
    >
      {word}
    </Link>
  );
}

/** The test row of a saved provider's drawer: a test runs on the saved settings, not the form. */
export function ProviderTestRow({
  provider,
  dirty,
  waiting,
  starting,
  disabledReason,
  onTest,
}: {
  provider: ssoApi.SsoProvider;
  dirty: boolean;
  waiting: boolean;
  starting: boolean;
  disabledReason?: string | undefined;
  onTest: () => void;
}) {
  const { t } = useTranslation('settings-admin');
  const { t: tc } = useTranslation('common');
  const reason =
    disabledReason ??
    (dirty
      ? t('sso_providers.test.save_first')
      : starting
        ? tc('access.operation_pending')
        : undefined);

  return (
    <div className="mb-6 flex items-center gap-3" data-testid="sso-provider-test">
      <span className="min-w-0 flex-1">
        <span className="flex items-baseline gap-2 font-sans text-sm">
          <span className="font-strong text-tx-0">{t('sso_providers.test.title')}</span>
          <TestStatusWord provider={provider} waiting={waiting} linked />
        </span>
        <span className="mt-0.5 block font-sans text-xs leading-relaxed text-tx-3">
          {dirty ? t('sso_providers.test.save_first') : t('sso_providers.test.hint')}
        </span>
      </span>
      <ChromeButton
        type="button"
        size="sm"
        disabled={Boolean(reason)}
        disabledReason={reason}
        onClick={onTest}
      >
        {t('sso_providers.actions.test')}
      </ChromeButton>
    </div>
  );
}
