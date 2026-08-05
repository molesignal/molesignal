import type {
  DashboardElement,
  DashboardTabItem,
  GridPosition,
} from './schema';

export interface LayoutItem {
  id: string;
  gridPos: GridPosition;
}

/**
 * Returns the rendered pixel size of an item spanning CSS grid tracks.
 * A span includes the gaps between its tracks, but never a trailing gap.
 */
export function gridSpanSize(
  span: number,
  trackSize: number,
  gap: number,
): number {
  const trackCount = Math.max(1, Math.round(span));
  return trackCount * trackSize + (trackCount - 1) * gap;
}

export function clampGridPosition(
  position: GridPosition,
  columns: number,
): GridPosition {
  const minW = Math.max(1, position.minW ?? 1);
  const minH = Math.max(1, position.minH ?? 1);
  const maxW = Math.max(minW, Math.min(columns, position.maxW ?? columns));
  const maxH = Math.max(minH, position.maxH ?? Number.MAX_SAFE_INTEGER);
  const w = clamp(Math.round(position.w), minW, maxW);
  const h = clamp(Math.round(position.h), minH, maxH);
  return {
    ...position,
    x: clamp(Math.round(position.x), 0, Math.max(0, columns - w)),
    y: Math.max(0, Math.round(position.y)),
    w,
    h,
  };
}

export function gridPositionsCollide(
  left: GridPosition,
  right: GridPosition,
): boolean {
  return !(
    left.x + left.w <= right.x ||
    right.x + right.w <= left.x ||
    left.y + left.h <= right.y ||
    right.y + right.h <= left.y
  );
}

/**
 * Applies a drag/resize result and pushes only the elements that overlap it.
 * This mirrors the predictable "gravity down" behavior users expect from
 * dashboard editors without rewriting unrelated saved positions.
 */
export function placeLayoutItem<T extends LayoutItem>(
  items: readonly T[],
  id: string,
  nextPosition: GridPosition,
  columns: number,
): T[] {
  const next = items.map((item) =>
    item.id === id
      ? { ...item, gridPos: clampGridPosition(nextPosition, columns) }
      : { ...item, gridPos: clampGridPosition(item.gridPos, columns) },
  );
  const moving = next.find((item) => item.id === id);
  if (!moving) return next;

  const queue: T[] = [moving];
  const visited = new Set<string>([moving.id]);
  while (queue.length > 0) {
    const source = queue.shift();
    if (!source) break;
    for (let index = 0; index < next.length; index += 1) {
      const candidate = next[index];
      if (!candidate || candidate.id === source.id) continue;
      if (!gridPositionsCollide(source.gridPos, candidate.gridPos)) continue;
      const pushed = {
        ...candidate,
        gridPos: clampGridPosition(
          {
            ...candidate.gridPos,
            y: source.gridPos.y + source.gridPos.h,
          },
          columns,
        ),
      };
      next[index] = pushed;
      if (!visited.has(pushed.id)) {
        visited.add(pushed.id);
        queue.push(pushed);
      }
    }
  }
  return next;
}

export function compactLayout<T extends LayoutItem>(
  items: readonly T[],
  columns: number,
): T[] {
  const compacted: T[] = [];
  const sorted = items
    .map((item) => ({
      ...item,
      gridPos: clampGridPosition(item.gridPos, columns),
    }))
    .sort(compareLayoutItems);

  for (const item of sorted) {
    let y = 0;
    while (y < item.gridPos.y) {
      const position = { ...item.gridPos, y };
      if (
        compacted.every(
          (candidate) => !gridPositionsCollide(position, candidate.gridPos),
        )
      ) {
        item.gridPos = position;
        break;
      }
      y += 1;
    }
    compacted.push(item);
  }
  return compacted;
}

export function autoLayout<T extends LayoutItem>(
  items: readonly T[],
  columns: number,
): T[] {
  const laidOut: T[] = [];
  for (const source of items) {
    const item = {
      ...source,
      gridPos: clampGridPosition(source.gridPos, columns),
    };
    let placed = false;
    for (let y = 0; !placed; y += 1) {
      for (let x = 0; x <= columns - item.gridPos.w; x += 1) {
        const position = { ...item.gridPos, x, y };
        if (
          laidOut.every(
            (candidate) => !gridPositionsCollide(position, candidate.gridPos),
          )
        ) {
          item.gridPos = position;
          placed = true;
          break;
        }
      }
    }
    laidOut.push(item);
  }
  return laidOut;
}

export function updateElementInTree(
  elements: readonly DashboardElement[],
  id: string,
  update: (element: DashboardElement) => DashboardElement,
): DashboardElement[] {
  return elements.map((element) => {
    if (element.id === id) return update(element);
    if (element.kind === 'group' || element.kind === 'row') {
      return {
        ...element,
        elements: updateElementInTree(element.elements, id, update),
      };
    }
    if (element.kind === 'tab') {
      return {
        ...element,
        tabs: element.tabs.map((tab) =>
          updateTabElements(tab, id, update),
        ),
      };
    }
    return element;
  });
}

export function removeElementFromTree(
  elements: readonly DashboardElement[],
  id: string,
): DashboardElement[] {
  return elements
    .filter((element) => element.id !== id)
    .map((element) => {
      if (element.kind === 'group' || element.kind === 'row') {
        return {
          ...element,
          elements: removeElementFromTree(element.elements, id),
        };
      }
      if (element.kind === 'tab') {
        return {
          ...element,
          tabs: element.tabs.map((tab) => ({
            ...tab,
            elements: removeElementFromTree(tab.elements, id),
          })),
        };
      }
      return element;
    });
}

export function findElement(
  elements: readonly DashboardElement[],
  id: string,
): DashboardElement | undefined {
  for (const element of elements) {
    if (element.id === id) return element;
    if (element.kind === 'group' || element.kind === 'row') {
      const match = findElement(element.elements, id);
      if (match) return match;
    } else if (element.kind === 'tab') {
      for (const tab of element.tabs) {
        const match = findElement(tab.elements, id);
        if (match) return match;
      }
    }
  }
  return undefined;
}

export function layoutBottom(items: readonly LayoutItem[]): number {
  return items.reduce(
    (bottom, item) => Math.max(bottom, item.gridPos.y + item.gridPos.h),
    0,
  );
}

function updateTabElements(
  tab: DashboardTabItem,
  id: string,
  update: (element: DashboardElement) => DashboardElement,
): DashboardTabItem {
  return {
    ...tab,
    elements: updateElementInTree(tab.elements, id, update),
  };
}

function compareLayoutItems(left: LayoutItem, right: LayoutItem): number {
  return (
    left.gridPos.y - right.gridPos.y ||
    left.gridPos.x - right.gridPos.x ||
    left.id.localeCompare(right.id)
  );
}

function clamp(value: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, value));
}
