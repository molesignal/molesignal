import * as React from 'react';

/** Controls whose press or key changes a value without an input event: a menu of options, a switch, a day. */
const VALUE_CONTROL =
  '[role="option"],[role="menuitemradio"],[role="menuitemcheckbox"],[role="switch"],[role="checkbox"],[role="radio"],[role="slider"],[role="gridcell"],[aria-pressed],[aria-checked]';

function isValueControl(target: EventTarget, buttonsCount: boolean): boolean {
  if (!(target instanceof Element)) return false;

  const control = target.closest(VALUE_CONTROL);
  // A view tab says which part of the form is showing, not what it holds.
  if (control !== null && !control.hasAttribute('data-tab')) return true;
  if (!buttonsCount) return false;

  const button = target.closest('button');
  return button !== null && !button.hasAttribute('data-tab') && !button.hasAttribute('data-draft-ignore');
}

/** Keys that change what a field holds, whichever editor draws the field. */
function isEditingKey(event: React.KeyboardEvent): boolean {
  if (event.metaKey || event.ctrlKey || event.altKey) return false;
  return event.key.length === 1 || event.key === 'Backspace' || event.key === 'Delete';
}

/**
 * Whether anything in a form was touched, from the events of the element that
 * holds it, so a form of many fields need not compare them. `buttons` counts any
 * button in it too, for a form with cards and presets that fill it in; a button
 * that changes nothing says so with `data-draft-ignore`.
 */
export function useDraftTouched({ buttons = false }: { buttons?: boolean } = {}) {
  const [touched, setTouched] = React.useState(false);
  const markChanged = React.useCallback(() => setTouched(true), []);

  return {
    touched,
    markChanged,
    reset: React.useCallback(() => setTouched(false), []),
    events: {
      onInput: markChanged,
      onChange: markChanged,
      onPointerUpCapture: (event: React.PointerEvent) => {
        if (isValueControl(event.target, buttons)) markChanged();
      },
      onKeyDownCapture: (event: React.KeyboardEvent) => {
        const pressed = (event.key === 'Enter' || event.key === ' ') && isValueControl(event.target, buttons);
        if (pressed || isEditingKey(event)) markChanged();
      },
    },
  };
}
