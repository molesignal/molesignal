import { describe, expect, it } from 'vitest';

import { createValueEchoes } from './valueEchoes';

describe('createValueEchoes', () => {
  it('takes the text of earlier keystrokes for the owner catching up', () => {
    const echoes = createValueEchoes();

    // Three keys are typed before the owner has rendered the first.
    ['l', 'le', 'lev'].forEach(echoes.told);

    expect(echoes.isEcho('l', 'lev')).toBe(true);
    expect(echoes.isEcho('le', 'lev')).toBe(true);
    expect(echoes.isEcho('lev', 'lev')).toBe(true);
  });

  it('takes a text the editor never held for the owner’s own', () => {
    const echoes = createValueEchoes();

    ['l', 'le'].forEach(echoes.told);

    // A field was picked in the list: the owner wrote a clause into the query.
    expect(echoes.isEcho("level = ''", 'le')).toBe(false);
  });

  it('forgets what was told once the owner sets a text of its own', () => {
    const echoes = createValueEchoes();

    ['a', 'ab'].forEach(echoes.told);
    expect(echoes.isEcho('reset', 'ab')).toBe(false);

    // "a" was told before the reset: handed back now, it is the owner going back to it.
    expect(echoes.isEcho('a', 'reset')).toBe(false);
  });

  it('skips renders the owner never made', () => {
    const echoes = createValueEchoes();

    ['a', 'ab', 'abc', 'abcd'].forEach(echoes.told);

    // The owner rendered "ab" and then "abcd", and nothing between.
    expect(echoes.isEcho('ab', 'abcd')).toBe(true);
    expect(echoes.isEcho('abcd', 'abcd')).toBe(true);
    // Caught up: an older text handed back now is the owner's own.
    expect(echoes.isEcho('abc', 'abcd')).toBe(false);
  });

  it('does not wait for ever on an owner that never answers', () => {
    const echoes = createValueEchoes();

    Array.from({ length: 200 }, (_, index) => `text ${index}`).forEach(echoes.told);

    expect(echoes.isEcho('text 0', 'text 199')).toBe(false);
  });

  it('holds for a text typed, deleted and typed again', () => {
    const echoes = createValueEchoes();

    ['a', '', 'a', 'ab'].forEach(echoes.told);

    expect(echoes.isEcho('a', 'ab')).toBe(true);
    expect(echoes.isEcho('', 'ab')).toBe(true);
    expect(echoes.isEcho('a', 'ab')).toBe(true);
    expect(echoes.isEcho('ab', 'ab')).toBe(true);
  });
});
