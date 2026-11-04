import * as React from 'react';

/**
 * Whether children say something in words rather than hold only an icon: a string or a number
 * anywhere in them, other than inside a span for screen readers only. Words a component makes
 * by itself cannot be seen from here, so children made only of such components count as an icon.
 */
export function hasVisibleText(children: React.ReactNode): boolean {
  let found = false;
  React.Children.forEach(children, (child) => {
    if (found) return;
    if (typeof child === 'string') {
      found = child.trim() !== '';
    } else if (typeof child === 'number') {
      found = true;
    } else if (React.isValidElement<{ children?: React.ReactNode; className?: unknown }>(child)) {
      const { className, children: inner } = child.props;
      if (typeof className === 'string' && /\bsr-only\b/.test(className)) return;
      found = hasVisibleText(inner);
    }
  });
  return found;
}
