import { expect, it } from 'vitest';
import { cn } from '@/shell/lib/cn';

it('knows the product weights and sizes', () => {
  expect(cn('text-sm font-medium', 'font-body')).toBe('text-sm font-body');
  expect(cn('font-display', 'font-sans')).toBe('font-display font-sans');
  expect(cn('font-strong font-mono')).toBe('font-strong font-mono');
  expect(cn('font-strong', 'font-display')).toBe('font-display');
  expect(cn('text-kpi text-tx-0')).toBe('text-kpi text-tx-0');
  expect(cn('text-xs', 'text-sm')).toBe('text-sm');
});
