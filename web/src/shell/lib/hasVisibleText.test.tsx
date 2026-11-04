import { Plus } from 'lucide-react';
import { expect, it } from 'vitest';

import { hasVisibleText } from '@/shell/lib/hasVisibleText';

it('finds words wherever they are in the children', () => {
  expect(hasVisibleText('Save')).toBe(true);
  expect(hasVisibleText(3)).toBe(true);
  expect(hasVisibleText([<Plus key="icon" />, 'New rule'])).toBe(true);
  expect(hasVisibleText(<><Plus /> New rule</>)).toBe(true);
  expect(hasVisibleText(<span className="truncate">acme-prod</span>)).toBe(true);
});

it('counts an icon, blanks and words for screen readers only as no words', () => {
  expect(hasVisibleText(<Plus />)).toBe(false);
  expect(hasVisibleText([<Plus key="icon" />, ' ', null, false])).toBe(false);
  expect(hasVisibleText(<><Plus /><span className="sr-only">Refresh</span></>)).toBe(false);
  expect(hasVisibleText(undefined)).toBe(false);
});
