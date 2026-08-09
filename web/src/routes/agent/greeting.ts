export type GreetingPeriod = 'morning' | 'afternoon' | 'evening' | 'late';

export function greetingPeriodForHour(hour: number): GreetingPeriod {
  if (hour >= 5 && hour < 12) return 'morning';
  if (hour >= 12 && hour < 18) return 'afternoon';
  if (hour >= 18 && hour < 23) return 'evening';
  return 'late';
}
