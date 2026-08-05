import type { CorrelationContext, Filter } from '@/api/web';
import type { FrameKind } from '@/stores/useInvestigationStack';
import type { TimeWindow } from '@/stores/useTimeStore';
import { halo } from '@/time/halo';

export type SignalKind = 'metric' | 'trace' | 'log' | 'host' | 'service';

export interface SourceCtx {
  kind: SignalKind;
  t?: string; // ISO datetime of the click/hover
  globalWindow: TimeWindow;
  fields: Record<string, string>; // trace_id, service.name, host, severity, …
}

export interface LinkProvider {
  from: SignalKind;
  to: SignalKind;
  label: string;
  /** Map this provider into the target FrameKind that lands on the stack. */
  targetFrameKind: FrameKind;
  derive: (ctx: SourceCtx) => CorrelationContext;
}

function filtersFromFields(fields: Record<string, string>, keep: string[]): Filter[] {
  const out: Filter[] = [];
  for (const k of keep) {
    const v = fields[k];
    if (v !== undefined) out.push({ field: k, op: '=', value: v });
  }
  return out;
}

const m2t: LinkProvider = {
  from: 'metric',
  to: 'trace',
  label: 'View traces',
  targetFrameKind: 'trace',
  derive: (ctx) => {
    const at = ctx.t ?? new Date().toISOString();
    return {
      time_range: rangeToObject(halo('metric_sample', at, ctx.globalWindow)),
      filters: filtersFromFields(ctx.fields, ['service.name']),
      prefill: {
        sql: `SELECT trace_id, duration_ms FROM traces WHERE service.name = '${ctx.fields['service.name'] ?? ''}' ORDER BY duration_ms DESC LIMIT 100`,
      },
    };
  },
};

const m2l: LinkProvider = {
  from: 'metric',
  to: 'log',
  label: 'View logs',
  targetFrameKind: 'log',
  derive: (ctx) => {
    const at = ctx.t ?? new Date().toISOString();
    return {
      time_range: rangeToObject(halo('metric_sample', at, ctx.globalWindow)),
      filters: filtersFromFields(ctx.fields, ['service.name', 'host']),
    };
  },
};

const t2l: LinkProvider = {
  from: 'trace',
  to: 'log',
  label: 'View logs',
  targetFrameKind: 'log',
  derive: (ctx) => {
    const at = ctx.t ?? new Date().toISOString();
    return {
      time_range: rangeToObject(halo('trace_span', at, ctx.globalWindow)),
      filters: filtersFromFields(ctx.fields, ['trace_id', 'service.name', 'host']),
    };
  },
};

const t2h: LinkProvider = {
  from: 'trace',
  to: 'host',
  label: 'View host',
  targetFrameKind: 'host',
  derive: (ctx) => {
    const at = ctx.t ?? new Date().toISOString();
    return {
      time_range: rangeToObject(halo('trace_span', at, ctx.globalWindow)),
      filters: filtersFromFields(ctx.fields, ['host']),
    };
  },
};

const l2t: LinkProvider = {
  from: 'log',
  to: 'trace',
  label: 'View trace',
  targetFrameKind: 'trace',
  derive: (ctx) => {
    const at = ctx.t ?? new Date().toISOString();
    return {
      time_range: rangeToObject(halo('log_row', at, ctx.globalWindow)),
      filters: filtersFromFields(ctx.fields, ['trace_id']),
    };
  },
};

const l2h: LinkProvider = {
  from: 'log',
  to: 'host',
  label: 'View host',
  targetFrameKind: 'host',
  derive: (ctx) => {
    const at = ctx.t ?? new Date().toISOString();
    return {
      time_range: rangeToObject(halo('log_row', at, ctx.globalWindow)),
      filters: filtersFromFields(ctx.fields, ['host']),
    };
  },
};

const h2m: LinkProvider = {
  from: 'host',
  to: 'metric',
  label: 'View metrics',
  targetFrameKind: 'metric',
  derive: (ctx) => {
    const at = ctx.t ?? new Date().toISOString();
    return {
      time_range: rangeToObject(halo('metric_sample', at, ctx.globalWindow)),
      filters: filtersFromFields(ctx.fields, ['host']),
    };
  },
};

const s2t: LinkProvider = {
  from: 'service',
  to: 'trace',
  label: 'View traces',
  targetFrameKind: 'trace',
  derive: (ctx) => {
    const at = ctx.t ?? new Date().toISOString();
    return {
      time_range: rangeToObject(halo('trace_span', at, ctx.globalWindow)),
      filters: filtersFromFields(ctx.fields, ['service.name']),
      prefill: {
        sql: `SELECT trace_id, duration_ms FROM traces WHERE service.name = '${ctx.fields['service.name'] ?? ''}' ORDER BY duration_ms DESC LIMIT 100`,
      },
    };
  },
};

export const PROVIDERS: LinkProvider[] = [m2t, m2l, t2l, t2h, l2t, l2h, h2m, s2t];

export function providersFor(from: SignalKind): LinkProvider[] {
  return PROVIDERS.filter((p) => p.from === from);
}

function rangeToObject(w: TimeWindow): { from: string; to: string } {
  return { from: w.from, to: w.to };
}
