import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';

import { LevelChip, LevelMark } from './LevelMark';

describe('level words', () => {
  it('writes a level in a table cell in the face and size of the row, and the head of a record in the monospace face', () => {
    render(
      <>
        <LevelChip level="ERROR" />
        <LevelMark level="WARN" />
      </>,
    );

    expect(screen.getByText('ERROR').className).not.toContain('font-mono');
    expect(screen.getByText('ERROR').className).toContain('text-xs');
    expect(screen.getByText('ERROR').className).toContain('font-strong');
    expect(screen.getByText('WARN').className).toContain('font-mono');
  });
});
