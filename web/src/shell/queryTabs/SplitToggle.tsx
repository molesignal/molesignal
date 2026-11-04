import { Columns2 } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { ModuleToggle } from '@/shell/query/ModuleToggle';

import { QuerySplitContext } from './scope';

/**
 * Splits the page in two halves, each as wide as the other and each with its
 * own tabs in its query box, or gives the page back to one.
 *
 * A page puts it in the head of its results, first of the signs there, as
 * large as they are. It is drawn in the half at the right only, outside a
 * query tab not at all, and a window too narrow for two halves has none.
 */
export function QuerySplitToggle({ size = 'md' }: { size?: 'md' | 'sm' }) {
  const { t } = useTranslation('common');
  const control = React.useContext(QuerySplitContext);
  const ref = React.useRef<HTMLButtonElement>(null);
  // The focus the switch takes when it is put on the page is handed over, not arrived at.
  const claiming = React.useRef(false);

  React.useLayoutEffect(() => {
    if (!ref.current || !control?.claimFocus()) return;
    claiming.current = true;
    ref.current.focus();
    claiming.current = false;
    // Only the switch just put on the page asks.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  if (!control || (!control.room && !control.split)) return null;

  const { split, onChange } = control;

  return (
    <ModuleToggle
      ref={ref}
      size={size}
      open={split}
      icon={Columns2}
      actionLabel={t(split ? 'query_tabs.unsplit' : 'query_tabs.split')}
      data-query-split={split ? 'on' : 'off'}
      // A tip is for someone who came to the switch; this one was brought to it.
      onFocus={(event) => {
        if (claiming.current) event.preventDefault();
      }}
      onOpenChange={onChange}
    />
  );
}
