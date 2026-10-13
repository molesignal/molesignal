import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as intakeApi from '@/api/intake';
import * as rumIntakeApi from '@/api/rumIntake';
import { toApiError } from '@/lib/http';
import { ChromeButton } from '@/shell/chrome';

import type { IntakeContext } from '../intakeContext';
import type { Signal, Source } from '../sources';

const TEST_STREAM = 'default';

type SendState =
  | { kind: 'idle' }
  | { kind: 'pending' }
  | { kind: 'ok'; accepted: number; rejected: number }
  | { kind: 'fail'; message: string };

/**
 * Sends one event of each signal the source carries from this browser, to tell
 * a receiver that takes data apart from a collector that is not sending any.
 */
export function TestEventButton({
  source,
  context,
}: {
  source: Source;
  context: IntakeContext;
}) {
  const { t } = useTranslation('onboarding');
  const [state, setState] = React.useState<SendState>({ kind: 'idle' });

  // A profile cannot be made up in the browser.
  if (source.signals.every((signal) => signal === 'profiles')) return null;

  const send = async () => {
    setState({ kind: 'pending' });

    try {
      const result = await sendTestEvent(source, context);
      setState({ kind: 'ok', accepted: result.accepted, rejected: result.rejected });
    } catch (error) {
      setState({ kind: 'fail', message: toApiError(error).message });
    }
  };

  return (
    <>
      <ChromeButton
        onClick={send}
        disabled={
          state.kind === 'pending' ||
          (context.isRum && (!context.applicationValid || !context.token))
        }
      >
        {state.kind === 'pending'
          ? t('datasource.test_event_sending')
          : t('datasource.test_event')}
      </ChromeButton>
      {state.kind === 'ok' && (
        <span role="status" className="font-sans text-xs font-semibold text-[var(--green-ink)]">
          {t('datasource.test_event_sent', {
            accepted: state.accepted,
            rejected: state.rejected,
          })}
        </span>
      )}
      {state.kind === 'fail' && (
        <span role="alert" className="font-sans text-xs font-semibold text-red-soft">
          {state.message}
        </span>
      )}
    </>
  );
}

async function sendTestEvent(
  source: Source,
  context: IntakeContext,
): Promise<intakeApi.IntakeResult> {
  if (source.rumPlatform) {
    return rumIntakeApi.sendTestError({
      token: context.token,
      applicationId: context.applicationId,
      platform: source.rumPlatform,
      service: source.id,
    });
  }

  const signals = source.signals.filter(
    (signal): signal is Exclude<Signal, 'profiles'> => signal !== 'profiles',
  );
  const results = await Promise.all(
    signals.map((signal) => {
      if (signal === 'traces') {
        return intakeApi.intakeTraces(TEST_STREAM, [testTraceEvent(source)]);
      }
      if (signal === 'metrics') {
        return intakeApi.intakeMetrics(TEST_STREAM, [testMetricEvent(source)]);
      }
      return intakeApi.intakeLogs(TEST_STREAM, [testLogEvent(source)]);
    }),
  );

  return results.reduce<intakeApi.IntakeResult>(
    (total, result) => ({
      accepted: total.accepted + result.accepted,
      rejected: total.rejected + result.rejected,
      errors: [...(total.errors ?? []), ...(result.errors ?? [])],
    }),
    { accepted: 0, rejected: 0, errors: [] },
  );
}

function testTraceEvent(source: Source): Record<string, unknown> {
  const startNs = Date.now() * 1_000_000;
  const endNs = startNs + 1_000_000;

  return {
    _timestamp: Math.floor(startNs / 1000),
    trace_id: crypto.randomUUID().replace(/-/g, ''),
    span_id: crypto.randomUUID().replace(/-/g, '').slice(0, 16),
    'service.name': source.id,
    name: 'molesignal.datasource.test',
    kind: 1,
    start_time_unix_nano: startNs,
    end_time_unix_nano: endNs,
    duration_ns: endNs - startNs,
    status_code: 'OK',
    generated_by: 'molesignal-web',
  };
}

function testMetricEvent(source: Source): Record<string, unknown> {
  return {
    name: 'molesignal_datasource_test_total',
    value: 1,
    timestamp: new Date().toISOString(),
    tags: { source: source.id, generated_by: 'molesignal-web' },
  };
}

function testLogEvent(source: Source): Record<string, unknown> {
  return {
    timestamp: new Date().toISOString(),
    level: 'info',
    // The source's id, not its name: the event reads the same in every language.
    message: `molesignal datasource test event from ${source.id}`,
    source: source.id,
    generated_by: 'molesignal-web',
  };
}
