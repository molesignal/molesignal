/**
 * A moment of a session as the player's ruler reads it: minutes and seconds from the beginning, `0:38`,
 * and with tenths where the rows of a list are told apart by them, `0:41.5`. Past the hour the hours come
 * first: `1:02:03`.
 */
export function formatClock(milliseconds: number, tenths = false): string {
  const total = Math.max(0, Number.isFinite(milliseconds) ? milliseconds : 0);
  // Counted in tenths first, so that 59.96 s carries into the minute instead of reading 0:59.10.
  const wholeTenths = Math.floor(total / 100);
  const seconds = Math.floor(wholeTenths / 10);
  const fraction = wholeTenths % 10;
  const hours = Math.floor(seconds / 3_600);
  const minutes = Math.floor((seconds % 3_600) / 60);
  const rest = String(seconds % 60).padStart(2, '0');
  const head = hours > 0 ? `${hours}:${String(minutes).padStart(2, '0')}` : String(minutes);

  return `${head}:${rest}${tenths ? `.${fraction}` : ''}`;
}
