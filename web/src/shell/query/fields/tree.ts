/** A field as a list shows it: under its own name, or under what is left of it inside a group. */
export interface FieldTreeLeaf<T> {
  field: T;
  /** The name it is listed under: its own, or the part after the group's. */
  label: string;
}

/** Fields that share the first part of their dotted names, as one entry that opens on them. */
export interface FieldTreeGroup<T> {
  kind: 'group';
  /** The shared first part: `http` of `http.method` and `http.route`. */
  name: string;
  children: FieldTreeLeaf<T>[];
}

export type FieldTreeNode<T> = ({ kind: 'field' } & FieldTreeLeaf<T>) | FieldTreeGroup<T>;

/** The part of a dotted name before its first dot, and the rest; nothing for a name that has no two parts. */
function splitHead(name: string): { head: string; rest: string } | null {
  const dot = name.indexOf('.');
  if (dot <= 0 || dot === name.length - 1) return null;

  return { head: name.slice(0, dot), rest: name.slice(dot + 1) };
}

/**
 * Fields as a tree of two levels.
 *
 * A dotted name is a path: `http.method` and `http.route` are `method` and
 * `route` of `http`. Fields that share the first part of their names are
 * listed once under it, each by the rest of its name, so a long list reads as
 * a handful of things with their parts instead of as forty names. A field
 * whose whole name is that first part (a document beside the fields read out
 * of it) goes in the group too, first and under its own name.
 *
 * A first part only one field has says nothing: that field stays where it is,
 * under its whole name. The order is the order given; a group stands where
 * its first field stood.
 */
export function fieldTree<T extends { name: string }>(fields: readonly T[]): FieldTreeNode<T>[] {
  const parts = new Map<string, number>();
  const wholes = new Set<string>();

  for (const field of fields) {
    const split = splitHead(field.name);
    if (split) parts.set(split.head, (parts.get(split.head) ?? 0) + 1);
    else wholes.add(field.name);
  }

  const grouped = (head: string) => {
    const count = parts.get(head) ?? 0;

    return count >= 2 || (count === 1 && wholes.has(head));
  };
  const groups = new Map<string, FieldTreeGroup<T>>();
  const tree: FieldTreeNode<T>[] = [];
  const groupOf = (name: string) => {
    let group = groups.get(name);

    if (!group) {
      group = { kind: 'group', name, children: [] };
      groups.set(name, group);
      tree.push(group);
    }

    return group;
  };

  for (const field of fields) {
    const split = splitHead(field.name);

    if (split && grouped(split.head)) {
      groupOf(split.head).children.push({ field, label: split.rest });
    } else if (!split && grouped(field.name)) {
      // The document itself leads the fields read out of it, wherever it stood among them.
      groupOf(field.name).children.unshift({ field, label: field.name });
    } else {
      tree.push({ kind: 'field', field, label: field.name });
    }
  }

  return tree;
}

/** How many fields a tree lists, groups opened. */
export function fieldTreeSize<T>(tree: readonly FieldTreeNode<T>[]): number {
  return tree.reduce((count, node) => count + (node.kind === 'group' ? node.children.length : 1), 0);
}
