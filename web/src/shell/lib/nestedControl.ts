import type * as React from 'react';

/** What a person presses to act: a reference, a link, a fold mark, a field. */
const CONTROL = [
  'a[href]',
  'button',
  'input',
  'select',
  'textarea',
  'summary',
  '[role="button"]',
  '[role="link"]',
  '[role="checkbox"]',
  '[role="switch"]',
  '[role="menuitem"]',
  '[role="tab"]',
].join(', ');

/**
 * Whether a click a container hears was made on a control inside it, or in a menu one of them
 * opened. Such a click is the control's alone: a row that selects on a click leaves it be.
 *
 * The control does not stop its click on the way up instead. An open popover closes on a press
 * whose click reaches the page, and keeps open on one stopped short of it, as one kept from it on
 * purpose: a reference that stopped its click would leave the menu of another open beside its own.
 */
export function isNestedControlClick(event: React.MouseEvent<Element>): boolean {
  const { target, currentTarget } = event;
  if (!(target instanceof Element)) return false;

  // React passes a click up through the portal of a menu or popover to the components that opened
  // it; it was made outside the container's own elements, and it is never the container's.
  if (!currentTarget.contains(target)) return true;

  const control = target.closest(CONTROL);
  return control !== null && control !== currentTarget && currentTarget.contains(control);
}
