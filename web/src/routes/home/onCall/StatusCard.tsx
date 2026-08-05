import {
  AlertTriangle,
  ChevronRight,
  Clock3,
  UserRoundCheck,
} from 'lucide-react';
import { useTranslation } from 'react-i18next';

import {
  Card,
  CardBody,
  CardHeader,
  cardTextActionClass,
  ChromeButton,
  Dot,
  Pill,
  uiLabelClass,
} from '@/shell/chrome';
import { cn } from '@/shell/lib/cn';
import type { UserLite } from '@/shell/useUsers';

import type {
  FeaturedOnCall,
  OnCallShiftOverview,
} from './model';
import { OnCallShiftOverviewPanel } from './ShiftOverview';
import {
  formatScheduleDay,
  formatScheduleTime,
  relativeDuration,
  UserAvatar,
} from '../../alerts/schedule/Ui';

type StatusTone = 'dim' | 'green' | 'orange' | 'red';

const CARD_TREATMENT: Record<StatusTone, string> = {
  dim: 'bg-bg-1',
  green: 'bg-bg-1',
  orange: 'border-l-[4px] border-l-orange bg-bg-1',
  red: 'border-l-[4px] border-l-red bg-bg-1',
};

function durationWithoutDirection(
  targetMicros: number,
  nowMicros: number,
  locale: string,
): string {
  const value = relativeDuration(targetMicros, nowMicros, locale);
  return locale.toLowerCase().startsWith('zh')
    ? value.replace(/后$/, '')
    : value.replace(/^in\s+/, '');
}

function zonedDayOrdinal(
  micros: number,
  timeZone: string,
): number {
  try {
    const parts = new Intl.DateTimeFormat('en-US', {
      timeZone,
      year: 'numeric',
      month: 'numeric',
      day: 'numeric',
    }).formatToParts(new Date(micros / 1000));
    const value = (type: Intl.DateTimeFormatPartTypes) =>
      Number(parts.find((part) => part.type === type)?.value);
    return Math.floor(
      Date.UTC(value('year'), value('month') - 1, value('day')) /
        86_400_000,
    );
  } catch {
    return Math.floor(micros / 1000 / 86_400_000);
  }
}

function StatusBadge({
  tone,
  children,
}: {
  tone: StatusTone;
  children: React.ReactNode;
}) {
  return (
    <Pill tone={tone}>
      <Dot tone={tone === 'dim' ? 'dim' : tone} />
      {children}
    </Pill>
  );
}

export interface OnCallStatusCardProps {
  feature: FeaturedOnCall | null;
  teamName: string | undefined;
  usersById: ReadonlyMap<string, UserLite>;
  shiftOverview: OnCallShiftOverview | null;
  nowMicros: number;
  locale: string;
  loading: boolean;
  onViewSchedule: () => void;
  onViewEscalations: () => void;
  onOpenIncidents: () => void;
  onArrange: () => void;
  arrangeDisabled: boolean;
  arrangeDisabledReason?: string | undefined;
  className?: string;
}

export function OnCallStatusCard({
  feature,
  teamName,
  usersById,
  shiftOverview,
  nowMicros,
  locale,
  loading,
  onViewSchedule,
  onViewEscalations,
  onOpenIncidents,
  onArrange,
  arrangeDisabled,
  arrangeDisabledReason,
  className,
}: OnCallStatusCardProps) {
  const { t } = useTranslation('onboarding');
  const pendingCount = shiftOverview?.pendingCount ?? 0;
  const isGap = feature?.status === 'gap';
  const isOverride = Boolean(feature?.activeOverride);
  const currentUser = feature?.current?.userId
    ? usersById.get(feature.current.userId)
    : undefined;
  const currentName =
    currentUser?.name ??
    feature?.current?.userId ??
    t('home.on_call.unknown_member');
  const currentTeamName =
    teamName?.trim() || t('home.on_call.unassigned_team');
  const replacedName = feature?.replacedUserId
    ? usersById.get(feature.replacedUserId)?.name ??
      feature.replacedUserId
    : t('home.on_call.unknown_member');
  const nextName = feature?.nextUserId
    ? usersById.get(feature.nextUserId)?.name ??
      feature.nextUserId
    : t('home.on_call.unknown_member');
  const timeZone = feature?.schedule.timezone ?? 'UTC';
  const tone: StatusTone = isGap
    ? 'red'
    : isOverride || pendingCount > 0
      ? 'orange'
      : feature?.current
        ? 'green'
        : 'dim';

  const formatAt = (micros: number) => {
    const time = formatScheduleTime(micros, locale, timeZone);
    const dayDistance =
      zonedDayOrdinal(micros, timeZone) -
      zonedDayOrdinal(nowMicros, timeZone);
    if (dayDistance === 0) {
      return t('home.on_call.today_at', { time });
    }
    if (dayDistance === 1) {
      return t('home.on_call.tomorrow_at', { time });
    }
    return `${formatScheduleDay(micros, locale, timeZone)} ${time}`;
  };

  const title = isGap
    ? t('home.on_call.gap_title')
    : isOverride
      ? t('home.on_call.override_title')
      : feature?.current
        ? t('home.on_call.active_title')
        : t('home.on_call.title');

  return (
    <Card
      className={cn(
        'relative flex min-h-[250px] flex-1 overflow-hidden',
        CARD_TREATMENT[tone],
        className,
      )}
      bodyClassName="flex min-h-0 flex-1 flex-col"
    >
      <CardHeader
        className={cn(
          'relative z-10 shrink-0',
          isGap && 'text-red-soft',
        )}
        title={
          <span className="flex items-center gap-2">
            {isGap ? (
              <AlertTriangle
                aria-hidden="true"
                className="h-4 w-4 text-red-soft"
              />
            ) : (
              <UserRoundCheck
                aria-hidden="true"
                className={cn(
                  'h-4 w-4',
                  tone === 'orange'
                    ? 'text-orange-soft'
                    : tone === 'green'
                      ? 'text-green-soft'
                      : 'text-tx-3',
                )}
              />
            )}
            {title}
          </span>
        }
        actions={
          isGap ? (
            <StatusBadge tone="red">
              {t('home.on_call.coverage_gap_badge')}
            </StatusBadge>
          ) : feature?.current ? (
            <StatusBadge tone="green">
              {t('home.on_call.active_badge')}
            </StatusBadge>
          ) : undefined
        }
      />

      {loading && !feature ? (
        <div className="relative z-10 grid min-h-[204px] flex-1 place-items-center font-sans text-xs text-tx-2">
          {t('home.loading')}
        </div>
      ) : !feature ? (
        <CardBody className="relative z-10 grid min-h-[204px] flex-1 place-items-center text-center">
          <div>
            <UserRoundCheck
              aria-hidden="true"
              className="mx-auto h-5 w-5 text-tx-3"
            />
            <div className="mt-2 font-sans text-sm font-strong text-tx-1">
              {t('home.on_call.empty_title')}
            </div>
            <p className="mt-1 font-sans text-xs text-tx-2">
              {t('home.on_call.empty_description')}
            </p>
          </div>
        </CardBody>
      ) : isGap ? (
        <CardBody className="relative z-10 flex min-h-[204px] flex-1 flex-col p-4">
          <div className="font-sans text-base font-strong text-tx-0">
            {feature.schedule.name}
          </div>
          <p className="mt-2 max-w-[46ch] font-sans text-xs leading-relaxed text-tx-2">
            {t('home.on_call.gap_description')}
          </p>
          <div className="mt-auto flex flex-wrap items-center gap-2 pt-4">
            <ChromeButton
              size="sm"
              className="h-11 border-red/30 text-red-soft enabled:hover:bg-red-dim sm:h-8"
              disabled={arrangeDisabled}
              disabledReason={arrangeDisabledReason}
              onClick={onArrange}
            >
              {t('home.on_call.arrange')}
              <ChevronRight aria-hidden="true" className="h-3 w-3" />
            </ChromeButton>
            <button
              type="button"
              className={cn(cardTextActionClass, 'h-11 sm:h-8')}
              onClick={onViewSchedule}
            >
              {t('home.on_call.view_schedule')}
              <ChevronRight aria-hidden="true" className="h-3 w-3" />
            </button>
          </div>
        </CardBody>
      ) : !feature.current ? (
        <CardBody className="relative z-10 grid min-h-[204px] flex-1 place-items-center text-center">
          <div>
            <Clock3
              aria-hidden="true"
              className="mx-auto h-5 w-5 text-tx-3"
            />
            <div className="mt-2 font-sans text-sm font-strong text-tx-1">
              {t('home.on_call.no_current')}
            </div>
            <p className="mt-1 font-sans text-xs text-tx-2">
              {feature.schedule.name}
            </p>
          </div>
        </CardBody>
      ) : (
        <CardBody className="relative z-10 flex min-h-[204px] flex-1 flex-col overflow-y-auto px-4 py-2">
          <div className="flex min-w-0 items-center gap-3">
            <UserAvatar user={currentUser} size="lg" online />
            <div className="min-w-0 flex-1">
              <div className="truncate font-sans text-base font-display text-tx-0">
                {currentName}
              </div>
              <div className="mt-1 truncate font-sans text-xs text-tx-2">
                {currentTeamName}
              </div>
              {isOverride && (
                <div className="mt-1 truncate font-sans text-xs font-strong text-orange-soft">
                  {t('home.on_call.override_description', {
                    current: currentName,
                    original: replacedName,
                  })}
                </div>
              )}
            </div>
          </div>

          <div className="mt-2 grid grid-cols-1 gap-3 border-y border-bd-0 py-1.5 min-[430px]:grid-cols-2">
            <div className="min-w-0">
              <div className={uiLabelClass}>
                {isOverride
                  ? t('home.on_call.override_until_label')
                  : t('home.on_call.next_handoff')}
              </div>
              <div className="mt-1 truncate font-sans text-sm font-strong tabular-nums text-tx-0">
                {feature.nextAt
                  ? formatAt(feature.nextAt)
                  : t('home.on_call.no_handoff')}
              </div>
              {feature.nextAt && (
                <div className="mt-1 flex min-w-0 items-center gap-1.5 font-sans text-xs text-tx-2">
                  <span className="shrink-0">
                    {t('home.on_call.next_label')}
                  </span>
                  <Pill tone="blue" className="max-w-28 shrink-0">
                    <span title={nextName} className="truncate">
                      {nextName}
                    </span>
                  </Pill>
                </div>
              )}
            </div>

            <div className="min-w-0 min-[430px]:border-l min-[430px]:border-bd-0 min-[430px]:pl-4">
              <div className={uiLabelClass}>
                {t('home.on_call.remaining_label')}
              </div>
              <div className="mt-1 font-sans text-[20px] font-display-strong leading-tight tabular-nums tracking-[-0.02em] text-tx-0">
                {feature.nextAt
                  ? durationWithoutDirection(
                      feature.nextAt,
                      nowMicros,
                      locale,
                    )
                  : '—'}
              </div>
            </div>
          </div>

          <OnCallShiftOverviewPanel
            overview={shiftOverview}
            scheduleName={feature.schedule.name}
            startedAt={
              feature.currentStartedAt != null
                ? formatAt(feature.currentStartedAt)
                : null
            }
            elapsed={
              feature.currentStartedAt != null
                ? durationWithoutDirection(
                    nowMicros,
                    feature.currentStartedAt,
                    locale,
                  )
                : null
            }
            onViewSchedule={onViewSchedule}
            onViewEscalations={onViewEscalations}
          />

          {pendingCount > 0 && (
            <div
              role="status"
              data-testid="on-call-acknowledgement-status"
              className="mt-2 flex min-h-10 items-center gap-2 rounded-md bg-orange-dim px-3 py-2 font-sans text-xs font-strong text-orange-soft"
            >
              <AlertTriangle
                aria-hidden="true"
                className="h-4 w-4 shrink-0"
              />
              <span>
                {t('home.on_call.pending_summary', {
                  count: pendingCount,
                })}
              </span>
            </div>
          )}

          <div className="mt-auto flex flex-wrap items-center justify-end gap-2 pt-2">
            {pendingCount > 0 && (
              <ChromeButton
                size="sm"
                className="h-11 sm:h-8"
                onClick={onOpenIncidents}
              >
                {t('home.on_call.open_incidents')}
              </ChromeButton>
            )}
            <button
              type="button"
              className={cn(
                cardTextActionClass,
                '-mr-1 h-11 sm:h-8',
              )}
              onClick={onViewSchedule}
            >
              {t('home.on_call.view_schedule')}
              <ChevronRight aria-hidden="true" className="h-3 w-3" />
            </button>
          </div>
        </CardBody>
      )}
    </Card>
  );
}
