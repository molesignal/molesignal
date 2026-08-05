export interface LogRow {
  /** Epoch microseconds (matches backend `_timestamp`). */
  _timestamp: number;
  level?: 'fatal' | 'error' | 'warn' | 'info' | 'debug' | string;
  service?: string;
  message?: string;
  trace_id?: string;
  /** Arbitrary additional fields. */
  [key: string]: unknown;
}
