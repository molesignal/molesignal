import type { TraceNode, TraceTree } from './tree';

/** What one side of a comparison holds of a service, or of an operation of one. */
export interface CompareSide {
  spans: number;
  errors: number;
  /** The time its spans spent on their own work. */
  selfNs: number;
}

/** A service, or an operation of one, in both traces. */
export interface CompareRow {
  key: string;
  service: string;
  /** `null` on a service's line. */
  operation: string | null;
  a: CompareSide;
  b: CompareSide;
  /** How much more of its own time the second trace spent on it; less is negative. */
  deltaNs: number;
}

/** A step of the way from a trace's root down to a span: who did what. */
export interface CallStep {
  service: string;
  operation: string;
}

/**
 * Calls one trace made that the other did not: spans at a place of the call
 * tree, the way down to it named by service and operation. Where a call is
 * missing, so is what it called: those are counted under it, not listed.
 */
export interface MissingCall {
  key: string;
  path: CallStep[];
  /** How many more spans the trace has at this place than the other. */
  count: number;
  /** How many spans lie under them. */
  under: number;
}

export interface TraceComparison {
  /** How much longer the second trace took; shorter is negative. */
  durationDeltaNs: number;
  services: CompareRow[];
  operations: CompareRow[];
  /** Calls only the first trace made, the biggest first. */
  onlyInA: MissingCall[];
  /** Calls only the second trace made, the biggest first. */
  onlyInB: MissingCall[];
}

/**
 * Two traces side by side: how much longer the second took, where it spent
 * its time differently, by service and by operation, and the calls one made
 * that the other did not. Spans are matched by the way down to them: the
 * services and operations of every span above them, so the same query made
 * from two places counts as two calls.
 */
export function compareTraces(a: TraceTree, b: TraceTree): TraceComparison {
  return {
    durationDeltaNs: b.totalNs - a.totalNs,
    services: compareGroups(a, b, (node) => [node.service, null]),
    operations: compareGroups(a, b, (node) => [node.service, node.span.operation]),
    ...missingCalls(a, b),
  };
}

function compareGroups(a: TraceTree, b: TraceTree, keyOf: (node: TraceNode) => [string, string | null]): CompareRow[] {
  const rows = new Map<string, CompareRow>();
  const empty = (): CompareSide => ({ spans: 0, errors: 0, selfNs: 0 });

  for (const [side, tree] of [['a', a], ['b', b]] as const) {
    for (const node of tree.nodes) {
      const [service, operation] = keyOf(node);
      const key = operation === null ? service : `${service}\u0000${operation}`;
      let row = rows.get(key);
      if (!row) {
        row = { key, service, operation, a: empty(), b: empty(), deltaNs: 0 };
        rows.set(key, row);
      }
      row[side].spans += 1;
      row[side].selfNs += node.selfNs;
      if (node.failed) row[side].errors += 1;
    }
  }

  for (const row of rows.values()) row.deltaNs = row.b.selfNs - row.a.selfNs;

  // The biggest change first: where the second trace's time went differently.
  return [...rows.values()].sort(
    (left, right) => Math.abs(right.deltaNs) - Math.abs(left.deltaNs) || right.a.selfNs + right.b.selfNs - (left.a.selfNs + left.b.selfNs) || left.key.localeCompare(right.key),
  );
}

/** The places of both trees' calls, each the same number in both, and how many spans each tree has at each. */
interface CallPlaces {
  parent: Array<number | null>;
  step: CallStep[];
  count: { a: number[]; b: number[] };
}

function placesOf(a: TraceTree, b: TraceTree): CallPlaces {
  const ids = new Map<string, number>();
  const places: CallPlaces = { parent: [], step: [], count: { a: [], b: [] } };

  for (const [side, tree] of [['a', a], ['b', b]] as const) {
    const placeOf = new Map<string, number>();

    // Every span comes after its caller: the caller's place is known when the span's is worked out.
    for (const node of tree.nodes) {
      const parent = node.parentId ? placeOf.get(node.parentId) ?? null : null;
      const key = `${parent ?? ''}\u0001${node.service}\u0000${node.span.operation}`;
      let place = ids.get(key);
      if (place === undefined) {
        place = places.step.length;
        ids.set(key, place);
        places.parent.push(parent);
        places.step.push({ service: node.service, operation: node.span.operation });
        places.count.a.push(0);
        places.count.b.push(0);
      }
      placeOf.set(node.id, place);
      places.count[side][place] = (places.count[side][place] ?? 0) + 1;
    }
  }

  return places;
}

function missingCalls(a: TraceTree, b: TraceTree): Pick<TraceComparison, 'onlyInA' | 'onlyInB'> {
  const places = placesOf(a, b);

  const side = (mine: number[], theirs: number[]): MissingCall[] => {
    const extra = mine.map((count, place) => Math.max(0, count - (theirs[place] ?? 0)));
    // Each place's topmost place with extra spans of its own: a place is made before any under it.
    const top: number[] = [];
    const calls = new Map<number, MissingCall>();

    for (let place = 0; place < extra.length; place += 1) {
      const parent = places.parent[place] ?? null;
      top[place] = parent !== null && extra[parent]! > 0 ? top[parent]! : place;
      if (extra[place]! === 0) continue;

      if (top[place] === place) calls.set(place, { key: String(place), path: pathOf(places, place), count: extra[place]!, under: 0 });
      else calls.get(top[place]!)!.under += extra[place]!;
    }

    return [...calls.values()].sort((left, right) => right.count + right.under - (left.count + left.under) || Number(left.key) - Number(right.key));
  };

  return { onlyInA: side(places.count.a, places.count.b), onlyInB: side(places.count.b, places.count.a) };
}

function pathOf(places: CallPlaces, place: number): CallStep[] {
  const path: CallStep[] = [];
  for (let at: number | null = place; at !== null; at = places.parent[at] ?? null) path.unshift(places.step[at]!);
  return path;
}
