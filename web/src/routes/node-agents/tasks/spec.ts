import type { NodeTaskKind } from '@/api/nodeAgents';

/**
 * The spec of a new task, filled in as a form and written as the JSON the node reads.
 *
 * `tool` follows `node_observe::tools::ToolSpec` exactly: the keys of each tool and no other.
 * `autotracing_run` names one of the AutoTracing scenarios. `profile` takes perf's sampling
 * options (window, target, frequency and mode).
 */

export const TOOLS = ['dropwatch', 'tcpshark', 'iotracing', 'perf'] as const;
export type ToolName = (typeof TOOLS)[number];

export const RULES = ['cpusys', 'cpuidle', 'dload', 'memburst', 'iotracing'] as const;
export type RuleName = (typeof RULES)[number];

export const PROFILE_MODES = ['oncpu', 'offcpu', 'virtual_alloc', 'physical_alloc', 'physical_usage'] as const;
export type ProfileMode = (typeof PROFILE_MODES)[number];

export const TARGETS = ['host', 'pid', 'container'] as const;
export type Target = (typeof TARGETS)[number];

export const DEVICE_SCOPES = ['all', 'only', 'except'] as const;
export type DeviceScope = (typeof DEVICE_SCOPES)[number];

/** Every field of the form, as typed. Numbers left empty take the node's default. */
export interface SpecDraft {
  tool: ToolName;
  durationSecs: string;
  filter: string;
  maxEventsPerSecond: string;
  deviceScope: DeviceScope;
  devices: string;
  enableTlp: boolean;
  maxStack: string;
  maxProcess: string;
  maxFilesPerProcess: string;
  scheduleThresholdMs: string;
  target: Target;
  pid: string;
  containerId: string;
  frequencyHz: string;
  mode: ProfileMode;
  rule: RuleName;
}

export type SpecField = keyof SpecDraft;

export type SpecProblem = 'duration' | 'number' | 'pid' | 'container' | 'devices' | 'block_devices';

export function defaultDraft(): SpecDraft {
  return {
    tool: 'dropwatch',
    durationSecs: '60',
    filter: '',
    maxEventsPerSecond: '100',
    deviceScope: 'all',
    devices: '',
    enableTlp: false,
    maxStack: '',
    maxProcess: '',
    maxFilesPerProcess: '',
    scheduleThresholdMs: '',
    target: 'host',
    pid: '',
    containerId: '',
    frequencyHz: '99',
    mode: 'oncpu',
    rule: 'cpusys',
  };
}

/** The window a tool starts with: a snapshot tool looks briefly, a stream for a minute. */
export function defaultDuration(tool: ToolName): string {
  return tool === 'iotracing' || tool === 'perf' ? '10' : '60';
}

type Built = { spec: Record<string, unknown> } | { problem: SpecProblem; field: SpecField };

class Invalid {
  constructor(
    readonly problem: SpecProblem,
    readonly field: SpecField,
  ) {}
}

function list(text: string): string[] {
  return text
    .split(/[\s,]+/)
    .map((item) => item.trim())
    .filter(Boolean);
}

/** A whole number of at least `min`, or nothing when the field is left empty and may be. */
function whole(draft: SpecDraft, field: SpecField, min = 0): number | undefined {
  const text = String(draft[field]).trim();
  if (!text) return undefined;

  const value = Number(text);
  if (!Number.isSafeInteger(value) || value < min) throw new Invalid('number', field);
  return value;
}

function duration(draft: SpecDraft): number {
  const value = Number(draft.durationSecs.trim());
  if (!Number.isInteger(value) || value < 1 || value > 3600) throw new Invalid('duration', 'durationSecs');
  return value;
}

function put(spec: Record<string, unknown>, key: string, value: unknown) {
  if (value !== undefined && value !== '') spec[key] = value;
}

/** perf's and profile's target: the whole host, one process, or one container. */
function target(draft: SpecDraft, spec: Record<string, unknown>) {
  if (draft.target === 'pid') {
    const pid = Number(draft.pid.trim());
    if (!Number.isInteger(pid) || pid < 1) throw new Invalid('pid', 'pid');
    spec.pid = pid;
  }

  if (draft.target === 'container') {
    const container = draft.containerId.trim();
    if (!container) throw new Invalid('container', 'containerId');
    spec.container_id = container;
  }
}

function toolSpec(draft: SpecDraft): Record<string, unknown> {
  const spec: Record<string, unknown> = { tool: draft.tool, duration_secs: duration(draft) };

  switch (draft.tool) {
    case 'dropwatch': {
      put(spec, 'filter', draft.filter.trim());
      put(spec, 'max_events_per_second', whole(draft, 'maxEventsPerSecond'));

      if (draft.deviceScope !== 'all') {
        const devices = list(draft.devices);
        if (devices.length === 0) throw new Invalid('devices', 'devices');
        spec[draft.deviceScope === 'only' ? 'devices' : 'devices_excluded'] = devices;
      }

      return spec;
    }
    case 'tcpshark': {
      spec.mode = 'retransmit';
      spec.enable_tlp = draft.enableTlp;
      put(spec, 'filter', draft.filter.trim());
      put(spec, 'max_events_per_second', whole(draft, 'maxEventsPerSecond'));
      return spec;
    }
    case 'iotracing': {
      const devices = list(draft.devices);
      if (devices.some((device) => !/^\d+:\d+$/.test(device))) throw new Invalid('block_devices', 'devices');
      if (devices.length > 0) spec.devices = devices;
      put(spec, 'max_stack', whole(draft, 'maxStack', 1));
      put(spec, 'max_process', whole(draft, 'maxProcess', 1));
      put(spec, 'max_files_per_process', whole(draft, 'maxFilesPerProcess', 1));
      put(spec, 'schedule_threshold_ms', whole(draft, 'scheduleThresholdMs'));
      return spec;
    }
    case 'perf': {
      target(draft, spec);
      put(spec, 'frequency_hz', whole(draft, 'frequencyHz', 1));
      spec.mode = draft.mode;
      return spec;
    }
  }
}

/** The spec the node is sent, or the first field that is not filled in right. */
export function buildSpec(kind: NodeTaskKind, draft: SpecDraft): Built {
  try {
    if (kind === 'tool') return { spec: toolSpec(draft) };
    if (kind === 'autotracing_run') return { spec: { rule: draft.rule } };

    const spec: Record<string, unknown> = { duration_secs: duration(draft) };
    target(draft, spec);
    put(spec, 'frequency_hz', whole(draft, 'frequencyHz', 1));
    spec.mode = draft.mode;
    return { spec };
  } catch (error) {
    if (error instanceof Invalid) return { problem: error.problem, field: error.field };
    throw error;
  }
}
