import { useTranslation } from 'react-i18next';

import type { ProbeStepEvidence } from '@/api/synthetics';

import { lossRatio, tracerouteHops } from './traceroute';

/**
 * The hops of a traceroute step, one row each: the address that answered at that distance, the
 * share of probes it left unanswered, and how long its answers took.
 */
export function TracerouteHops({ step }: { step: ProbeStepEvidence }) {
  const { t } = useTranslation('synthetics');
  const hops = tracerouteHops(step.metadata.hops);

  if (hops.length === 0) return null;

  const millis = (value: number | null) => (value === null ? '—' : value.toFixed(1));
  const numeric = 'py-[4px] text-right tabular-nums';

  return (
    <div className="mt-[6px]">
      <table data-traceroute-hops={hops.length} className="w-full table-fixed font-sans text-xs">
        <thead className="text-tx-2">
          <tr>
            <th scope="col" className="w-[36px] py-[4px] text-left font-normal">
              {t('results.traceroute.hop')}
            </th>
            <th scope="col" className="py-[4px] text-left font-normal">
              {t('results.traceroute.address')}
            </th>
            <th scope="col" className={`w-[52px] font-normal ${numeric}`}>
              {t('results.traceroute.loss')}
            </th>
            <th scope="col" className={`w-[64px] font-normal ${numeric}`}>
              {t('results.traceroute.best')}
            </th>
            <th scope="col" className={`w-[64px] font-normal ${numeric}`}>
              {t('results.traceroute.average')}
            </th>
            <th scope="col" className={`w-[64px] font-normal ${numeric}`}>
              {t('results.traceroute.worst')}
            </th>
          </tr>
        </thead>
        <tbody className="text-tx-1">
          {hops.map((hop) => (
            <tr key={hop.ttl} className="border-t border-bd-0">
              <td className="py-[4px] tabular-nums">{hop.ttl}</td>
              <td className="truncate py-[4px] font-code" title={hop.address ?? undefined}>
                {hop.address ?? '*'}
                {hop.addresses > 1 && <span className="text-tx-2"> +{hop.addresses - 1}</span>}
              </td>
              <td className={numeric}>{Math.round(lossRatio(hop) * 100)}%</td>
              <td className={numeric}>{millis(hop.bestMs)}</td>
              <td className={numeric}>{millis(hop.averageMs)}</td>
              <td className={numeric}>{millis(hop.worstMs)}</td>
            </tr>
          ))}
        </tbody>
      </table>
      {step.metadata.reached !== 'true' && (
        <p className="mt-[6px] font-sans text-xs text-tx-2">{t('results.traceroute.not_reached')}</p>
      )}
    </div>
  );
}
