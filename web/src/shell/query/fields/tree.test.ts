import { describe, expect, it } from 'vitest';

import { fieldTree, fieldTreeSize } from './tree';

const fields = (...names: string[]) => names.map((name) => ({ name }));
/** A tree as it reads: a field by its label, a group as its name over its children's labels. */
const shape = (names: string[]) =>
  fieldTree(fields(...names)).map((node) =>
    node.kind === 'group' ? { [node.name]: node.children.map((child) => child.label) } : node.label,
  );

describe('fieldTree', () => {
  it('lists fields that share the first part of their names once, under it', () => {
    expect(shape(['level', 'http.method', 'message', 'http.route', 'http.status_code'])).toEqual([
      'level',
      { http: ['method', 'route', 'status_code'] },
      'message',
    ]);
  });

  it('keeps the rest of a longer name whole: the tree has two levels', () => {
    expect(shape(['k8s.pod.name', 'k8s.namespace.name', 'k8s.pod.uid'])).toEqual([
      { k8s: ['pod.name', 'namespace.name', 'pod.uid'] },
    ]);
  });

  it('leaves a field alone whose first part no other field has', () => {
    expect(shape(['service.name', 'trace_id', 'url.path', 'server.address'])).toEqual([
      'service.name',
      'trace_id',
      'url.path',
      'server.address',
    ]);
  });

  it('puts a document first among the fields read out of it, under its own name', () => {
    expect(shape(['error.type', 'duration_ms', 'error', 'error.code'])).toEqual([
      { error: ['error', 'type', 'code'] },
      'duration_ms',
    ]);
    // One field read out of it is enough: the two belong together.
    expect(shape(['labels', 'labels.ip'])).toEqual([{ labels: ['labels', 'ip'] }]);
  });

  it('keeps the fields themselves, each under the group or the name it is listed by', () => {
    const [group, plain] = fieldTree([
      { name: 'http.method', type: 'string' },
      { name: 'http.route', type: 'string' },
      { name: 'level', type: 'string' },
    ]);

    expect(group).toMatchObject({ kind: 'group', name: 'http' });
    expect(group?.kind === 'group' && group.children[0]?.field).toEqual({ name: 'http.method', type: 'string' });
    expect(plain).toEqual({ kind: 'field', field: { name: 'level', type: 'string' }, label: 'level' });
  });

  it('takes a name for what it is when it only looks like a path', () => {
    expect(shape(['.hidden', 'trailing.', '_timestamp', '.other'])).toEqual([
      '.hidden',
      'trailing.',
      '_timestamp',
      '.other',
    ]);
    expect(fieldTree([])).toEqual([]);
  });

  it('counts the fields it lists, not the entries', () => {
    const tree = fieldTree(fields('level', 'http.method', 'http.route', 'message'));

    expect(tree).toHaveLength(3);
    expect(fieldTreeSize(tree)).toBe(4);
  });
});
