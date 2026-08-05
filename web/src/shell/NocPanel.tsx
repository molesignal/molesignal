import type { CSSProperties, ReactNode } from 'react';

import { uiLabelClass, uiLabelStrongClass } from '@/shell/chrome';
import { TimeSeriesSparkline } from '@/viz/timeseries/TimeSeriesChart';

/**
 * NocPanel / NocKpi — the layout primitives the NOC wallboard is built from
 * (brief Component Inventory). Extracted out of `routes/Noc.tsx` so the
 * wallboard's panel grammar lives in one place and can be reused by other
 * wallboard surfaces.
 */

export function NocKpi({
  label,
  value,
  data,
  color,
  state = 'ready',
}: {
  label: string;
  value: string;
  data: number[];
  color: string;
  state?: 'ready' | 'empty' | 'loading' | 'error';
}) {
  const valueTone =
    state === 'error'
      ? 'text-red-soft'
      : state === 'loading'
        ? 'text-blue'
        : state === 'empty'
          ? 'text-tx-2'
          : 'text-tx-0';

  return (
    <div className="relative flex min-h-[140px] flex-col gap-2 overflow-hidden rounded-lg border border-bd-1 bg-bg-1 p-4">
      <div className={uiLabelClass}>{label}</div>
      <div
        className={`font-sans font-display-strong leading-none tracking-tight ${state === 'ready' ? 'text-[56px]' : 'text-[28px]'} ${valueTone}`}
      >
        {value}
      </div>
      <div
        className={`pointer-events-none absolute bottom-0 right-0 h-12 w-2/3 opacity-50 ${state === 'ready' ? '' : 'hidden'}`}
      >
        <TimeSeriesSparkline data={data} color={color} height={48} ariaLabel={label} />
      </div>
    </div>
  );
}

export function NocPanel({
  title,
  className,
  children,
  bodyClassName,
  style,
}: {
  title: string;
  className?: string;
  children: ReactNode;
  bodyClassName?: string;
  style?: CSSProperties;
}) {
  return (
    <div
      className={`flex min-h-0 flex-col overflow-hidden rounded-lg border border-bd-1 bg-bg-1 ${className ?? ''}`}
      style={style}
    >
      <div className={`border-b border-bd-0 px-4 py-2 ${uiLabelStrongClass}`}>
        {title}
      </div>
      <div className={`flex-1 overflow-auto ${bodyClassName ?? 'p-4'}`}>{children}</div>
    </div>
  );
}
