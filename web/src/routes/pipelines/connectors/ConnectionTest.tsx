import { useMutation } from '@tanstack/react-query';
import { FlaskConical } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as connectorsApi from '@/api/connectors';
import type { Connector } from '@/api/connectors';
import { toApiError } from '@/lib/http';
import { ChromeButton } from '@/shell/chrome';
import { cn } from '@/shell/lib/cn';

import { configOf, problemText, settingsProblem, type ConnectorDraft } from './draft';

/**
 * Tries the settings as they stand, before they are saved: a secret left
 * empty on a saved connector is the one it holds. A result belongs to the
 * settings it was given, so it goes away as soon as they change.
 */
export function ConnectionTest({ draft, saved }: { draft: ConnectorDraft; saved?: Connector | undefined }) {
  const { t } = useTranslation('pipelines');
  const problem = settingsProblem(draft, saved);
  const config = configOf(draft, saved);
  const tried = JSON.stringify([draft.kind, config]);
  const [testedFor, setTestedFor] = React.useState<string | null>(null);

  const test = useMutation({
    mutationFn: () =>
      connectorsApi.testConnection({
        kind: draft.kind,
        configJson: config,
        ...(saved ? { id: saved.id } : {}),
      }),
  });

  const current = testedFor === tried;
  const result = current ? test.data : undefined;
  const failure = current && test.error ? toApiError(test.error).message : null;

  const status = test.isPending
    ? { tone: 'text-tx-2', text: t('connectors.test.running') }
    : result?.ok
      ? { tone: 'text-green', text: t('connectors.test.ok', { ms: result.elapsedMs }) }
      : result
        ? { tone: 'text-red', text: t(`connectors.test.codes.${result.code}`) }
        : failure
          ? { tone: 'text-red', text: failure }
          : null;

  return (
    <section data-connector-test className="grid gap-[8px]">
      <div className="flex min-w-0 flex-wrap items-center gap-x-[12px] gap-y-[6px]">
        <ChromeButton
          size="sm"
          disabled={problem !== null || test.isPending}
          {...(problem ? { disabledReason: problemText(problem, t) } : {})}
          onClick={() => {
            setTestedFor(tried);
            test.mutate();
          }}
        >
          <FlaskConical aria-hidden className="h-3.5 w-3.5" />
          {t('connectors.test.button')}
        </ChromeButton>
        <span
          role="status"
          aria-live="polite"
          data-connector-test-status={result ? result.code : undefined}
          className={cn('min-w-0 font-sans text-xs', status?.tone)}
        >
          {status?.text}
        </span>
      </div>
      {result && !result.ok && result.detail && (
        <p data-connector-test-detail className="m-0 break-words font-sans text-xs leading-relaxed text-tx-2">
          {result.detail}
        </p>
      )}
      <p className="m-0 font-sans text-xs leading-relaxed text-tx-3">{t(`connectors.test.hints.${draft.kind}`)}</p>
    </section>
  );
}
