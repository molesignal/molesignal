import { useTranslation } from 'react-i18next';

import type { ProbeAgent } from '@/api/synthetics';
import { Readout } from '@/shell/readout/Readout';
import { SpectrumBar } from '@/shell/readout/SpectrumBar';
import { STATEMENT_CELL, Statement } from '@/shell/readout/Statement';

import { summarizeAgents, type AgentStatus } from './model';

// Worst first, as the bar is read; a revoked agent is no longer part of the fleet.
const FLEET_STATES: ReadonlyArray<[status: AgentStatus, color: string]> = [
  ['offline', 'var(--red)'],
  ['degraded', 'var(--yellow)'],
  ['draining', 'var(--blue)'],
  ['registered', 'var(--indigo)'],
  ['online', 'var(--green)'],
];

/**
 * What the Agents tab opens with: can the agents run what is asked of them.
 *
 * The verdict is whether every agent is online. Beside it is the room they
 * have left, for checks and for browser journeys, since an agent that is
 * online and full runs nothing new either.
 */
export function AgentsStanding({ agents }: { agents: ProbeAgent[] }) {
  const { t } = useTranslation('synthetics');
  const summary = summarizeAgents(agents);
  const fleet = agents.filter((agent) => agent.status !== 'revoked');
  const count = (status: AgentStatus) => fleet.filter((agent) => agent.status === status).length;
  const down = count('offline');
  const tone =
    summary.active === 0
      ? { color: 'var(--tx-3)', text: 'text-tx-0' }
      : down > 0
        ? { color: 'var(--red)', text: 'text-red' }
        : summary.attention > 0
          ? { color: 'var(--yellow)', text: 'text-yellow' }
          : { color: 'var(--green)', text: 'text-green' };
  const free = (capacity: { available: number; maximum: number }) =>
    capacity.maximum > 0
      ? t('standing.of_total', { count: capacity.available, total: capacity.maximum })
      : '—';

  return (
    <Statement
      label={t('agents.standing_label')}
      tone={tone.color}
      eyebrow={t('agents.standing_eyebrow')}
      title={
        summary.active === 0
          ? t('agents.none_title')
          : summary.attention > 0
            ? t('agents.need_attention', { count: summary.attention })
            : t('agents.all_online', { count: summary.active })
      }
      titleClassName={tone.text}
      readoutColumns={2}
      dataAttributes={{
        'data-agents-standing':
          summary.active === 0 ? 'none' : summary.attention > 0 ? 'attention' : 'online',
      }}
      readouts={
        <>
          <Readout
            className={STATEMENT_CELL}
            riseIndex={1}
            label={t('agents.concurrent_capacity')}
            value={free(summary.capacity)}
            tone={
              summary.capacity.maximum > 0 && summary.capacity.available === 0 ? 'danger' : undefined
            }
            detail={t('agents.slots_free')}
          />
          <Readout
            className={STATEMENT_CELL}
            riseIndex={2}
            label={t('agents.browser_capacity')}
            value={free(summary.browserCapacity)}
            tone={
              summary.browserCapacity.maximum > 0 && summary.browserCapacity.available === 0
                ? 'danger'
                : undefined
            }
            detail={t('agents.browser_slots_free')}
          />
        </>
      }
    >
      {summary.active > 0 && (
        <SpectrumBar
          className="mt-[14px]"
          label={t('agents.states')}
          segments={FLEET_STATES.map(([status, color]) => ({
            key: status,
            label: t(`states.${status}`),
            value: count(status),
            color,
          })).filter((segment) => segment.value > 0 || segment.key === 'online')}
        />
      )}
    </Statement>
  );
}
