import type { useTranslation } from 'react-i18next';

type TFn = ReturnType<typeof useTranslation<'alerts'>>['t'];

/** A length in seconds as people say it: 45 s, 3 min 20 s, 2 h 5 min. */
export function formatDuration(secs: number, t: TFn): string {
  const rounded = Math.max(0, Math.round(secs));
  if (rounded < 60) return t('insights.duration.seconds', { count: rounded });
  if (rounded < 3_600) {
    const minutes = Math.floor(rounded / 60);
    const seconds = rounded % 60;
    return seconds
      ? t('insights.duration.minutes_seconds', { minutes, seconds })
      : t('insights.duration.minutes', { count: minutes });
  }
  const hours = Math.floor(rounded / 3_600);
  const minutes = Math.floor((rounded % 3_600) / 60);
  return minutes
    ? t('insights.duration.hours_minutes', { hours, minutes })
    : t('insights.duration.hours', { count: hours });
}

/** A share 0..1 as a whole percent, with one decimal below 1%. */
export function formatPercent(value: number): string {
  return `${(value * 100).toFixed(value > 0 && value < 0.01 ? 1 : 0)}%`;
}
