import { describe, expect, it } from 'vitest';

import { blockProblem, MAX_BLOCK_DEPTH, resolveBlocks } from './blocks';

// The same cases as the server's `engine/blocks.rs`, so the preview renders what is sent.
describe('template conditions', () => {
  it('keeps the branch that holds and leaves placeholders for later', () => {
    const body = '{{#if incident.status == "resolved"}}已恢复{{else}}请处理 {{summary}}{{/if}}';

    expect(resolveBlocks(body, { 'incident.status': 'resolved' })).toBe('已恢复');
    expect(resolveBlocks(body, { 'incident.status': 'open' })).toBe('请处理 {{summary}}');
  });

  it('tests presence and difference, and nests', () => {
    const body = "A{{#if runbook}}[{{runbook}}]{{#if severity != 'info'}}!{{/if}}{{/if}}B";

    expect(resolveBlocks(body, {})).toBe('AB');
    expect(resolveBlocks(body, { runbook: 'https://r', severity: 'info' })).toBe('A[{{runbook}}]B');
    expect(resolveBlocks(body, { runbook: 'https://r', severity: 'critical' })).toBe('A[{{runbook}}]!B');
    expect(resolveBlocks(body, { runbook: '  ' })).toBe('AB');
  });

  it('finds the mistakes the server refuses', () => {
    expect(blockProblem('{{#if a}}x')).not.toBeNull();
    expect(blockProblem('x{{/if}}')).not.toBeNull();
    expect(blockProblem('{{else}}')).not.toBeNull();
    expect(blockProblem('{{#if a}}1{{else}}2{{else}}3{{/if}}')).not.toBeNull();
    expect(blockProblem('{{#if a == b}}x{{/if}}')).not.toBeNull();
    expect(blockProblem('{{#each a}}x{{/if}}')).not.toBeNull();
    expect(blockProblem('{{#if a b}}x{{/if}}')).not.toBeNull();
    expect(blockProblem('{{#if a}}'.repeat(MAX_BLOCK_DEPTH + 1))).not.toBeNull();

    const deepest = `${'{{#if a}}'.repeat(MAX_BLOCK_DEPTH)}x${'{{/if}}'.repeat(MAX_BLOCK_DEPTH)}`;
    expect(blockProblem(deepest)).toBeNull();
    expect(blockProblem('plain {{summary}}')).toBeNull();
  });
});
