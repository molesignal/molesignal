import type { StatusPageComponent, StatusPageSubscriber } from '@/api/statusPages';

/**
 * The components a subscriber hears about, by name, in the page's order; an
 * empty list means the whole page. A component deleted since is left out.
 */
export function subscriberComponentNames(
  subscriber: Pick<StatusPageSubscriber, 'component_scope' | 'component_ids'>,
  components: Pick<StatusPageComponent, 'id' | 'name' | 'position'>[],
): string[] {
  if (subscriber.component_scope !== 'selected') return [];
  const chosen = new Set(subscriber.component_ids ?? []);
  return components
    .filter((component) => chosen.has(component.id))
    .sort((left, right) => left.position - right.position || left.name.localeCompare(right.name))
    .map((component) => component.name);
}
