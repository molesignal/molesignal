import { useTranslation } from 'react-i18next';

import type { InspectionLevel, InspectionRunStatus } from '@/api/logInspections';
import { Dot, Pill, type PillTone } from '@/shell/chrome';

import type { InspectionStanding } from './model';

type DotTone = 'green' | 'yellow' | 'red' | 'orange' | 'blue' | 'dim';

// The tones alerts and jobs already use for the same meanings: green is well, yellow wants a
// look, red wants action. A report without a verdict is neither well nor known to be wrong.
const LEVEL_TONE: Record<InspectionLevel, { pill: PillTone; dot: DotTone }> = {
  normal: { pill: 'green', dot: 'green' },
  attention: { pill: 'yellow', dot: 'yellow' },
  abnormal: { pill: 'red', dot: 'red' },
  unknown: { pill: 'orange', dot: 'orange' },
};

const STATUS_TONE: Record<InspectionRunStatus, { pill: PillTone; dot: DotTone }> = {
  queued: { pill: 'yellow', dot: 'yellow' },
  running: { pill: 'blue', dot: 'blue' },
  succeeded: { pill: 'green', dot: 'green' },
  failed: { pill: 'red', dot: 'red' },
  timed_out: { pill: 'red', dot: 'red' },
};

/** The verdict of a report. */
export function LevelMark({ level }: { level: InspectionLevel }) {
  const { t } = useTranslation('logs');
  const tone = LEVEL_TONE[level];

  return (
    <Pill tone={tone.pill}>
      <Dot tone={tone.dot} />
      {t(`inspections.levels.${level}`)}
    </Pill>
  );
}

/** How a run is going, or how it ended without a report. */
export function RunStatusMark({ status }: { status: InspectionRunStatus }) {
  const { t } = useTranslation('logs');
  const tone = STATUS_TONE[status];

  return (
    <Pill tone={tone.pill}>
      <Dot tone={tone.dot} />
      {t(`inspections.run_status.${status}`)}
    </Pill>
  );
}

/**
 * How an inspection stands: under way, never run, failed, or the verdict of
 * its last report. A run that succeeded is said by its verdict, not by the
 * word "succeeded": the verdict is what the reader came for.
 */
export function StandingMark({ standing }: { standing: InspectionStanding }) {
  const { t } = useTranslation('logs');

  if (standing.kind === 'active' || standing.kind === 'failed') {
    return <RunStatusMark status={standing.status} />;
  }
  if (standing.kind === 'level') return <LevelMark level={standing.level} />;

  return <Pill tone="dim">{t('inspections.never_run')}</Pill>;
}
