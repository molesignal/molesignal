import { Trans, useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import { cn } from '@/shell/lib/cn';
import { surfacePanelClass } from '@/shell/SurfaceWorkbench';

import { formatShare } from '../_helpers';
import { formatCount } from './format';
import {
  crashFreeSessions,
  crashFreeUsers,
  errorFreeSessions,
  formatVital,
  type Grade,
  markerOf,
  type OverviewScope,
  performancePath,
  type Pulse,
  scaleOf,
  type VitalReading,
} from './model';

const GRADE_TEXT: Record<Grade, string> = {
  good: 'text-[var(--green-ink)]',
  needs_improvement: 'text-[var(--yellow-ink)]',
  poor: 'text-[var(--red-ink)]',
};

const FIGURE = <span className="font-display text-tx-0" />;

/** A count with its noun: "1 session", "9 sessions", the figure set in the strong step. */
function Counted({ unit, value, language }: { unit: 'sessions' | 'people'; value: number; language: string }) {
  return (
    <Trans
      i18nKey={`overview.pulse.${unit}`}
      ns="rum"
      count={value}
      values={{ formatted: formatCount(value, language) }}
      components={{ n: FIGURE }}
    />
  );
}

/**
 * The opening statement of the overview: how many sessions, of how many people, and how many met
 * no error (or, for an app, no crash); under it the Core Web Vitals of the application, each on the
 * scale it is graded by, the mark where the 75th percentile of the views stands.
 */
export function OverviewPulse({
  pulse,
  vitals,
  scope,
  language,
}: {
  pulse: Pulse;
  /** Every vital of a browser application; none for an app, which sends no web vitals. */
  vitals: readonly VitalReading[] | null;
  /** What the vitals are of: each one opens the performance page on it. */
  scope: OverviewScope;
  language: string;
}) {
  const { t } = useTranslation('rum');

  const share = (value: number | null) => (value === null ? '—' : formatShare(value));
  // The two counts the sentence opens with: each agrees with its own number.
  const counts = {
    sessions: <Counted unit="sessions" value={pulse.sessions} language={language} />,
    people: <Counted unit="people" value={pulse.users} language={language} />,
  };

  return (
    <section
      aria-label={t('overview.pulse.aria')}
      data-rum-overview-pulse=""
      className={cn(surfacePanelClass, 'space-y-4 px-5 py-4')}
    >
      <p data-rum-pulse-statement="" className="m-0 type-page-title font-sans font-strong text-tx-1">
        {vitals ? (
          <Trans
            i18nKey="overview.pulse.browser"
            ns="rum"
            values={{ share: share(errorFreeSessions(pulse)) }}
            components={{ n: FIGURE, ...counts }}
          />
        ) : (
          <Trans
            i18nKey="overview.pulse.app"
            ns="rum"
            values={{
              sessionShare: share(crashFreeSessions(pulse)),
              userShare: share(crashFreeUsers(pulse)),
            }}
            components={{ n: FIGURE, ...counts }}
          />
        )}
      </p>
      {vitals && (
        <div className="grid gap-x-8 gap-y-4 sm:grid-cols-3">
          {vitals.map((reading) => (
            <VitalScale key={reading.vital.key} reading={reading} to={performancePath(scope, reading.vital.key)} />
          ))}
        </div>
      )}
    </section>
  );
}

function VitalScale({ reading, to }: { reading: VitalReading; to: string }) {
  const { t } = useTranslation('rum');
  const { vital, value, grade } = reading;
  const name = t(`overview.vitals.names.${vital.key}`);
  const [good, middle, poor] = scaleOf(vital);

  return (
    <div data-rum-vital={vital.key} data-rum-grade={grade ?? 'none'} className="relative min-w-0">
      <div className="flex items-baseline justify-between gap-2">
        <Link
          to={to}
          title={t('overview.vitals.open', { name })}
          className="type-caption font-strong text-tx-2 outline-none after:absolute after:inset-0 after:content-[''] hover:text-tx-0 focus-visible:text-tx-0"
        >
          {t('overview.vitals.label', { name })}
        </Link>
        {grade && <span className={cn('type-caption font-strong', GRADE_TEXT[grade])}>{t(`overview.vitals.grade.${grade}`)}</span>}
      </div>
      <div className="mt-1 type-page-title font-sans font-display tabular-nums text-tx-0">
        {value === null ? <span className="text-tx-4">—</span> : formatVital(vital, value)}
      </div>
      <div
        role="img"
        aria-label={
          value === null || !grade
            ? t('overview.vitals.none_aria', { name })
            : t('overview.vitals.aria', { name, value: formatVital(vital, value), grade: t(`overview.vitals.grade.${grade}`) })
        }
        className="relative mt-2 flex h-[6px] gap-[2px]"
      >
        <span className="rounded-l-[3px] bg-[var(--green)] opacity-80" style={{ flex: good }} />
        <span className="bg-[var(--yellow)] opacity-80" style={{ flex: middle }} />
        <span className="rounded-r-[3px] bg-[var(--red)] opacity-80" style={{ flex: poor }} />
        {value !== null && (
          <span
            aria-hidden
            data-rum-vital-marker=""
            className="absolute -top-[3px] h-[12px] w-[2px] rounded-[1px] bg-tx-0"
            style={{ left: `${markerOf(vital, value) * 100}%` }}
          />
        )}
      </div>
      <div className="mt-1 flex items-baseline justify-between type-caption text-tx-3">
        <span>{t('overview.vitals.good_at', { value: formatVital(vital, vital.good) })}</span>
        <span>{t('overview.vitals.poor_at', { value: formatVital(vital, vital.poor) })}</span>
      </div>
    </div>
  );
}
