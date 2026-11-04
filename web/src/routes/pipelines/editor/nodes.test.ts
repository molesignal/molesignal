import { describe, expect, it } from 'vitest';

import { canRemoveNode, removeNode } from './nodes';
import { defaultPipelineGraph } from '../PipelineGraph';

const model = {
  ...defaultPipelineGraph('logs'),
  sources: ['first', 'second'],
  sinks: ['out', 'connector:c1'],
  transforms: [{ name: 'one', script: '.' }, { name: 'two', script: '.' }],
};

describe('node removal', () => {
  it('preserves selection when removing an earlier sibling or another lane', () => {
    const next = removeNode(model, 'transform-0', 'transform-1');
    expect(next.model.transforms.map((step) => step.name)).toEqual(['two']);
    expect(next.selectedId).toBe('transform-0');
    expect(removeNode(model, 'source-0', 'transform-1').selectedId).toBe('transform-1');
    expect(removeNode(model, 'source-0', 'source-1').model.sources).toEqual(['second']);
    expect(removeNode(model, 'sink-1', 'sink-1').selectedId).toBe('sink-0');
  });

  it('selects the source after deleting the final step', () => {
    expect(removeNode({ ...model, transforms: [model.transforms[0]!] }, 'transform-0', 'transform-0').selectedId).toBe('source-0');
  });

  it('protects required endpoints and fixed nodes', () => {
    expect(canRemoveNode({ ...model, sources: ['only'] }, 'source-0')).toBe(false);
    expect(removeNode(model, 'sink-0', 'sink-0').model).toBe(model);
    expect(canRemoveNode({ ...model, mode: 'alert' }, 'source-0')).toBe(false);
    expect(canRemoveNode({ ...model, mode: 'alert' }, 'sink-0')).toBe(false);
    expect(canRemoveNode({ ...model, mode: 'realtime', signalType: 'metrics' }, 'sink-0')).toBe(false);
  });
});
