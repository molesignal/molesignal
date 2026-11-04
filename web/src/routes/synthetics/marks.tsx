import { useTranslation } from 'react-i18next';

import type {
  LocationHealth,
  MonitorKind,
  MonitorLifecycle,
  MonitorState,
  ProbeOutcome,
  SyntheticResult,
} from '@/api/synthetics';
import { cn } from '@/shell/lib/cn';

import { STATE_COLOR, STATE_TEXT, runStrip } from './summary';

const MARK = 'inline-flex min-w-0 items-center gap-[6px] font-sans text-xs font-strong';

function Mark({ color, text, children }: { color: string; text: string; children: string }) {
  return (
    <span className={cn(MARK, text)}>
      <span
        aria-hidden="true"
        className="h-[7px] w-[7px] shrink-0 rounded-full"
        style={{ background: color }}
      />
      <span className="truncate">{children}</span>
    </span>
  );
}

/**
 * Where a check stands, in a word and its colour. A check that is not on
 * schedule says so instead, and so does one that gets no results: its last
 * state is not what it is doing now.
 */
export function CheckStateMark({
  state,
  lifecycle = 'active',
  noData = false,
}: {
  state: MonitorState;
  lifecycle?: MonitorLifecycle | undefined;
  /** Enough of its locations stopped getting results (`MissingRuns.no_data`). */
  noData?: boolean | undefined;
}) {
  const { t } = useTranslation('synthetics');
  if (lifecycle === 'paused' || lifecycle === 'draft') {
    return (
      <Mark color="var(--tx-3)" text="text-tx-2">
        {t(`check_states.${lifecycle}`)}
      </Mark>
    );
  }

  if (noData) {
    return (
      <Mark color="var(--tx-3)" text="text-tx-2">
        {t('check_states.no_data')}
      </Mark>
    );
  }

  return (
    <Mark color={STATE_COLOR[state]} text={STATE_TEXT[state]}>
      {t(`check_states.${state}`)}
    </Mark>
  );
}

/** How one run went. */
export function OutcomeMark({ outcome }: { outcome: ProbeOutcome }) {
  const { t } = useTranslation('synthetics');

  return (
    <Mark color={STATE_COLOR[outcome]} text={STATE_TEXT[outcome]}>
      {t(`states.${outcome}`)}
    </Mark>
  );
}

const HEALTH_STATE: Record<LocationHealth, MonitorState> = {
  online: 'healthy',
  degraded: 'degraded',
  offline: 'failing',
  unknown: 'unknown',
};

/** Whether a location can run checks: a location is online or not, it does not "pass". */
export function LocationHealthMark({ health }: { health: LocationHealth }) {
  const { t } = useTranslation('synthetics');
  const state = HEALTH_STATE[health];

  return (
    <Mark color={STATE_COLOR[state]} text={STATE_TEXT[state]}>
      {t(`location_health.${health}`)}
    </Mark>
  );
}

/**
 * Each kind of check in an ink, as a log's level is, so the kinds an agent can
 * run read at a glance. Kinds that look at the same thing share one: reaching a
 * host (TCP, ping, DNS) and a secure handshake (TLS, SSH). A web request and a
 * remote call take the inks APM gives an HTTP and an RPC endpoint. A heartbeat
 * reaches nothing, so it is plain.
 */
const KIND_INK: Record<MonitorKind, string> = {
  http: 'text-[var(--blue-ink)]',
  grpc: 'text-[var(--purple-ink)]',
  browser: 'text-[var(--orange-ink)]',
  tcp: 'text-[var(--green-ink)]',
  icmp: 'text-[var(--green-ink)]',
  dns: 'text-[var(--green-ink)]',
  tls: 'text-[var(--yellow-ink)]',
  ssh: 'text-[var(--yellow-ink)]',
  heartbeat: 'text-tx-1',
};

/** A kind of check as a tag the size of a stream's type, the word in its kind's ink. */
export function KindTag({ kind }: { kind: MonitorKind }) {
  return (
    <span
      data-check-kind={kind}
      className={cn(
        'inline-flex h-[22px] items-center whitespace-nowrap rounded-md border border-bd-1 px-2 font-sans text-xs font-medium',
        KIND_INK[kind],
      )}
    >
      {kind}
    </span>
  );
}

/** Kinds of check as words in their inks for a table cell: the first few, then how many more. */
export function KindWords({ kinds, limit = 3 }: { kinds: readonly MonitorKind[]; limit?: number }) {
  return (
    <span className="font-sans text-xs">
      {kinds.slice(0, limit).map((kind, index) => (
        <span key={kind} data-check-kind={kind}>
          {index > 0 && <span className="text-tx-4"> · </span>}
          <span className={KIND_INK[kind]}>{kind}</span>
        </span>
      ))}
      {kinds.length > limit && <span className="text-tx-3"> +{kinds.length - limit}</span>}
    </span>
  );
}

/**
 * The latest runs as a strip, the oldest on the left and one tick a run, each
 * in the colour of how it went. Twenty passes and one failure read at a glance
 * where a percentage would say "95.2%".
 */
export function RunStrip({
  results,
  length = 20,
  className,
}: {
  results: readonly SyntheticResult[];
  length?: number;
  className?: string;
}) {
  const { t } = useTranslation('synthetics');
  const runs = runStrip(results, length);
  if (runs.length === 0) {
    return <span className={cn('font-sans text-xs text-tx-3', className)}>{t('runs.none')}</span>;
  }

  const passed = runs.filter((run) => run.outcome === 'healthy').length;
  // Runs fill the strip from the right, so the latest is always in the same place.
  const empty = Math.max(0, length - runs.length);

  return (
    <span
      role="img"
      data-run-strip
      aria-label={t('runs.summary', { count: runs.length, passed })}
      title={t('runs.summary', { count: runs.length, passed })}
      className={cn('inline-flex h-[18px] items-stretch gap-[2px]', className)}
    >
      {Array.from({ length: empty }, (_, index) => (
        <span key={`empty-${index}`} className="w-[4px] rounded-[1px] bg-bd-0" />
      ))}
      {runs.map((run) => (
        <span
          key={run.id}
          data-run={run.outcome}
          className="w-[4px] rounded-[1px]"
          style={{
            background: STATE_COLOR[run.outcome],
            // Passes recede so a failure among them stands out.
            opacity: run.outcome === 'healthy' ? 0.55 : 1,
          }}
        />
      ))}
    </span>
  );
}
