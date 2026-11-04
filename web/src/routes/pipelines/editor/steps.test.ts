import { describe, expect, it } from 'vitest';

import { stepType, type TransformStep } from '../PipelineGraph/model';
import { moveStep, newStep, retypeStep, STARTER_VRL } from './steps';

describe('pipeline steps', () => {
  it('starts a step of each kind ready to be filled in', () => {
    // A note on what to write: it runs, and changes nothing.
    expect(newStep('vrl', [])).toEqual({ name: 'transform', kind: 'vrl', script: STARTER_VRL });
    expect(STARTER_VRL.split('\n').every((line) => line.startsWith('#'))).toBe(true);
    // Routing by a field, which is for its author to name.
    expect(newStep('route', [])).toMatchObject({
      name: 'route',
      kind: 'builtin',
      operation: 'route',
      target: '',
      routing: { kind: 'field', field: '', fallback: 'default' },
    });

    const mask = newStep('mask', []);
    expect(mask).toMatchObject({ name: 'mask', kind: 'vrl', processor: { op: 'mask', field: '' } });
    expect(stepType(mask)).toBe('mask');
  });

  it('gives a new step a name no other step has', () => {
    expect(newStep('rename', ['rename']).name).toBe('rename-2');
    expect(newStep('rename', ['rename', 'rename-2']).name).toBe('rename-3');
    expect(newStep('parse_json', ['rename']).name).toBe('parse-json');
  });

  it('lets a name nobody chose follow the kind, and keeps one that was typed', () => {
    const taken = ['rename', 'tidy up'];

    expect(retypeStep(newStep('rename', []), 'mask', taken).name).toBe('mask');
    expect(retypeStep({ ...newStep('rename', []), name: 'rename-2' }, 'mask', taken).name).toBe('mask');
    expect(retypeStep({ ...newStep('rename', []), name: 'tidy up' }, 'mask', taken).name).toBe('tidy up');
    expect(retypeStep({ ...newStep('rename', []), name: '' }, 'route', taken).name).toBe('route');
  });

  it('turns a processor into the VRL it wrote, to be taken further by hand', () => {
    const rename: TransformStep = {
      ...newStep('rename', []),
      processor: { op: 'rename', from: 'msg', to: 'message' },
      script: 'if exists(.msg) {\n  .message = del(.msg)\n}',
    };

    const detached = retypeStep(rename, 'vrl', ['rename']);

    expect(detached).toEqual({
      name: 'transform',
      kind: 'vrl',
      script: 'if exists(.msg) {\n  .message = del(.msg)\n}',
    });
    expect(stepType(detached)).toBe('vrl');
  });

  it('does not carry a routing step into VRL, which has no script of its own', () => {
    expect(retypeStep(newStep('route', []), 'vrl', []).script).toBe(STARTER_VRL);
  });

  it('moves a step one place and leaves the list alone at either end', () => {
    const steps = ['a', 'b', 'c'].map((name) => ({ name, script: '. = .' }));
    const names = (list: TransformStep[]) => list.map((step) => step.name);

    expect(names(moveStep(steps, 0, 1))).toEqual(['b', 'a', 'c']);
    expect(names(moveStep(steps, 2, -1))).toEqual(['a', 'c', 'b']);
    expect(names(moveStep(steps, 0, -1))).toEqual(['a', 'b', 'c']);
    expect(names(moveStep(steps, 2, 1))).toEqual(['a', 'b', 'c']);
  });

  it('goes through labels when an alert pipeline renames or keeps by the set', () => {
    expect(newStep('rename_regex', [], 'alert').processor).toMatchObject({ op: 'rename_regex', within: 'labels' });
    expect(newStep('keep', [], 'alert').processor).toMatchObject({ op: 'keep', within: 'labels' });
    expect(newStep('rename_regex', [], 'scheduled').processor).toMatchObject({ within: '' });
    expect(retypeStep(newStep('vrl', []), 'keep', [], 'alert').processor).toMatchObject({ within: 'labels' });
  });

  it('starts the steps that go outside with what they still have to be told', () => {
    expect(newStep('http_enrich', [])).toEqual({ name: 'call-api', kind: 'http_enrich', script: '', connectorId: '' });

    const query = newStep('query_enrich', ['query']);
    expect(query.name).toBe('query-2');
    expect(query.query).toMatchObject({ language: 'sql', stream: '', windowSecs: 300, maxRows: 5 });
    expect(stepType(query)).toBe('query_enrich');
  });
});
