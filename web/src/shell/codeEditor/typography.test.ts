import { describe, expect, it } from 'vitest';

import {
  CODE_EDITOR_FONT_FAMILY,
  CODE_EDITOR_FONT_SIZE,
  CODE_EDITOR_FONT_WEIGHT,
  CODE_EDITOR_LINE_HEIGHT,
} from './typography';

describe('code editor typography', () => {
  it('keeps the shared font stack and metrics aligned with the editor contract', () => {
    expect(CODE_EDITOR_FONT_FAMILY).toMatch(
      /^"JetBrains Mono", "SFMono-Regular"/,
    );
    expect(CODE_EDITOR_FONT_SIZE).toBe(12);
    expect(CODE_EDITOR_FONT_WEIGHT).toBe(600);
    expect(CODE_EDITOR_LINE_HEIGHT).toBe(20);
  });
});
