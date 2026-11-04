import type { StatusPageComponent, StatusPageSubscriber } from '@/api/statusPages';

/**
 * The components a subscriber hears about, by name, in the page's order; an
 * empty list means the whole page. A component deleted since is left out.
 */
export function subscriberComponentNames(
  subscriber: Pick<StatusPageSubscriber, 'componentScope' | 'componentIds'>,
  components: Pick<StatusPageComponent, 'id' | 'name' | 'position'>[],
): string[] {
  if (subscriber.componentScope !== 'selected') return [];
  const chosen = new Set(subscriber.componentIds ?? []);
  return components
    .filter((component) => chosen.has(component.id))
    .sort((left, right) => left.position - right.position || left.name.localeCompare(right.name))
    .map((component) => component.name);
}
