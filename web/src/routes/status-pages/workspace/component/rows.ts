import type {
  ComponentLifecycle,
  ComponentStatus,
  ComponentVisibility,
  StatusPageComponent,
} from '@/api/statusPages';

import { componentTree, groupStatus, isComponentGroup } from '../../model';

/** One row of the components list. */
export interface ComponentListRow {
  component: StatusPageComponent;
  /** 1 for a component listed under its group. */
  depth: 0 | 1;
  /** The components of a group, matching or not. */
  members: StatusPageComponent[];
  /** The status the row shows; a group shows the worst of its components. */
  status: ComponentStatus;
}

export interface ComponentFilters {
  lifecycle: ComponentLifecycle;
  search: string;
  status?: ComponentStatus | undefined;
  visibility?: ComponentVisibility | undefined;
}

export function componentOrder(left: StatusPageComponent, right: StatusPageComponent): number {
  return left.position - right.position || left.name.localeCompare(right.name) || left.id.localeCompare(right.id);
}

/**
 * The rows of the components list: groups with their components under them,
 * as the public page lists them. A filter keeps a component that matches it,
 * and a group while a component in it matches; a group found by its own name
 * keeps all of its components.
 */
export function componentRows(
  components: readonly StatusPageComponent[],
  filters: ComponentFilters,
): ComponentListRow[] {
  const inView = components.filter((component) => component.lifecycle === filters.lifecycle).sort(componentOrder);
  const matches = (component: StatusPageComponent) =>
    (!filters.status || component.status === filters.status)
    && (!filters.visibility || component.visibility === filters.visibility)
    && matchesSearch(component, filters.search);

  return componentTree(inView).flatMap(({ item, members }): ComponentListRow[] => {
    if (!isComponentGroup(item)) {
      return matches(item) ? [{ component: item, depth: 0, members: [], status: item.status }] : [];
    }

    const foundByName = !filters.status && !filters.visibility && matchesSearch(item, filters.search);
    const listed = foundByName ? members : members.filter(matches);
    if (!foundByName && listed.length === 0) return [];

    return [
      { component: item, depth: 0, members, status: groupStatus(members) },
      ...listed.map((member): ComponentListRow => ({ component: member, depth: 1, members: [], status: member.status })),
    ];
  });
}

/** What a component is moved up and down among: the active components of its group, or the top level. */
export function componentSiblings(
  components: readonly StatusPageComponent[],
  component: StatusPageComponent,
): StatusPageComponent[] {
  const level = component.group_id ?? null;
  return components
    .filter((candidate) => candidate.lifecycle === 'active' && (candidate.group_id ?? null) === level)
    .sort(componentOrder);
}

function matchesSearch(component: StatusPageComponent, search: string): boolean {
  const normalized = search.trim().toLocaleLowerCase();
  return !normalized
    || component.name.toLocaleLowerCase().includes(normalized)
    || component.description.toLocaleLowerCase().includes(normalized);
}
