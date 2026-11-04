/** A span in the units an incident is timed in: seconds, then minutes, then hours and minutes. */
export function courseDuration(micros: number): string {
  const seconds = Math.max(0, Math.round(micros / 1_000_000));

  if (seconds < 60) return `${seconds}s`;
  if (seconds < 3600) return `${Math.round(seconds / 60)}m`;

  const hours = Math.floor(seconds / 3600);
  const minutes = Math.round((seconds % 3600) / 60);

  return minutes > 0 ? `${hours}h ${minutes}m` : `${hours}h`;
}
