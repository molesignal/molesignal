import { isPlatformStream } from '@/product/platformStreams';

const STREAM_NAME = /^[A-Za-z0-9_.-]{1,255}$/;
const STREAM_PREFIX = /^[A-Za-z0-9_.-]{0,254}$/;
/** The product's own stream; no pipeline may read from or write to it. */
const SYSTEM_STREAM = '_molesignal';

/**
 * Whether a pipeline may name a stream this: the backend's rule for stream
 * names (`validate_stream_name`), less the product's own stream.
 */
export function isStreamName(name: string): boolean {
  return STREAM_NAME.test(name) && name !== '.' && name !== '..' && name !== SYSTEM_STREAM;
}

/**
 * The streams RUM intake writes (`RumStream`): a pipeline may read them and
 * change their custom parts as they arrive, never write them from elsewhere.
 */
const RUM_STREAMS = new Set([
  'rum_sessions',
  'rum_views',
  'rum_actions',
  'rum_errors',
  'rum_resources',
  'rum_long_tasks',
  'rum_vitals',
  'rum_logs',
]);

/** The RUM streams that keep every row a pipeline sees: no step may drop one. */
const KEEP_EVERY_ROW = new Set(['rum_views', 'rum_vitals']);

/** The audit trail: a scheduled run reads it with the access of whoever last saved the pipeline. */
export const AUDIT_STREAM = '_audit';

export function isRumStream(name: string): boolean {
  return RUM_STREAMS.has(name);
}

/** Whether a pipeline on this RUM stream keeps every row: its steps may not drop one. */
export function keepsEveryRow(name: string): boolean {
  return KEEP_EVERY_ROW.has(name);
}

/** Whether a realtime pipeline may change a stream as it arrives: not one the platform writes, not the sessions. */
export function isRealtimeSource(name: string): boolean {
  return !isPlatformStream(name) && name !== 'rum_sessions';
}

/**
 * Whether a pipeline may write a stream: a stream name that is neither a RUM
 * stream nor one the platform writes (`writable_stream`).
 */
export function isWritableStream(name: string): boolean {
  return isStreamName(name) && !RUM_STREAMS.has(name) && !isPlatformStream(name);
}

/** What a routing step may put in front of a stream's name: the same characters, or nothing. */
export function isStreamPrefix(prefix: string): boolean {
  return STREAM_PREFIX.test(prefix);
}
