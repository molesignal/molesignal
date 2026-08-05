import type { ScheduledPipeline } from '@/api/pipelines';

export type PipelineHealth = 'healthy' | 'running' | 'error' | 'paused' | 'unknown' | 'never';

export function pipelineHealth(pipeline: ScheduledPipeline): PipelineHealth {
  if (pipeline.enabled === false) return 'paused';
  if (pipeline.last_run_state === 'failed') return 'error';
  if (pipeline.last_run_state === 'running') return 'running';
  if (pipeline.last_run_state === 'succeeded') return 'healthy';
  if (pipeline.last_run_started_at_micros || pipeline.last_run_at_micros) return 'unknown';
  return 'never';
}

export function pipelineSuccessRate(pipeline: ScheduledPipeline): number | null {
  const total = pipeline.runs_24h ?? 0;
  if (total <= 0) return null;
  return ((pipeline.succeeded_runs_24h ?? 0) / total) * 100;
}

export function formatSchedule(cron: string | null | undefined, locale: string): string {
  if (!cron) return '—';
  const match = /^every:(\d+)([smh])$/.exec(cron.trim());
  if (!match) return cron;
  const count = Number(match[1]);
  const zh = locale.toLowerCase().startsWith('zh');
  const unit = match[2];
  if (zh) {
    const label = unit === 's' ? '秒' : unit === 'm' ? '分钟' : '小时';
    return `每 ${count} ${label}`;
  }
  const label = unit === 's' ? 'second' : unit === 'm' ? 'minute' : 'hour';
  return `Every ${count} ${label}${count === 1 ? '' : 's'}`;
}

export function formatLookback(seconds: number | null | undefined, locale: string): string {
  if (seconds == null) return '—';
  const zh = locale.toLowerCase().startsWith('zh');
  if (seconds % 3600 === 0) {
    const value = seconds / 3600;
    return zh ? `${value} 小时` : `${value}h`;
  }
  if (seconds % 60 === 0) {
    const value = seconds / 60;
    return zh ? `${value} 分钟` : `${value}m`;
  }
  return zh ? `${seconds} 秒` : `${seconds}s`;
}

export function formatRelativeMicros(
  micros: number | null | undefined,
  locale: string,
  nowMillis = Date.now(),
): string {
  if (!micros) return '—';
  const deltaSeconds = Math.round(micros / 1000 - nowMillis) / 1000;
  const formatter = new Intl.RelativeTimeFormat(locale, { numeric: 'auto' });
  const absSeconds = Math.abs(deltaSeconds);
  if (absSeconds < 60) return formatter.format(Math.round(deltaSeconds), 'second');
  const deltaMinutes = deltaSeconds / 60;
  if (Math.abs(deltaMinutes) < 60) return formatter.format(Math.round(deltaMinutes), 'minute');
  const deltaHours = deltaMinutes / 60;
  if (Math.abs(deltaHours) < 24) return formatter.format(Math.round(deltaHours), 'hour');
  return formatter.format(Math.round(deltaHours / 24), 'day');
}

export function formatRunDuration(
  startedAtMicros: number,
  finishedAtMicros: number | null | undefined,
): string {
  if (!finishedAtMicros) return '—';
  const millis = Math.max(0, (finishedAtMicros - startedAtMicros) / 1000);
  if (millis < 1000) return `${millis.toFixed(0)} ms`;
  if (millis < 60_000) return `${(millis / 1000).toFixed(1)} s`;
  return `${(millis / 60_000).toFixed(1)} min`;
}
