import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { writeClipboardText } from '@/lib/clipboard';
import { CopyIconButton } from '@/shell/CopyIconButton';

/**
 * A value to copy out, shown the way a field is: an endpoint, a token, a
 * command. `value` is what gets copied; `children` is what is shown when that
 * differs (a masked token), and `actions` sit before the copy button.
 */
export function CopyField({
  value,
  copyLabel,
  disabled = false,
  actions,
  children,
}: {
  value: string;
  copyLabel: string;
  disabled?: boolean;
  actions?: React.ReactNode;
  children?: React.ReactNode;
}) {
  const { t } = useTranslation('common');
  const [copied, setCopied] = React.useState(false);
  const resetTimer = React.useRef<number | undefined>(undefined);

  React.useEffect(() => () => window.clearTimeout(resetTimer.current), []);

  const copy = async () => {
    if (!value) return;

    try {
      await writeClipboardText(value);
      setCopied(true);
      window.clearTimeout(resetTimer.current);
      resetTimer.current = window.setTimeout(() => setCopied(false), 1500);
    } catch {
      // Clipboard access can be denied by the browser.
    }
  };

  return (
    // The actions sit inside the field, so fields with and without a second one end together.
    <div
      data-ui="read-only-control"
      className="flex h-9 min-w-0 items-center rounded-md border-0 bg-[var(--control-surface)] pl-3 pr-[2px]"
    >
      <code className="min-w-0 flex-1 truncate font-mono text-xs text-tx-1">
        {children ?? value}
      </code>
      {actions}
      <CopyIconButton
        onClick={copy}
        disabled={disabled || !value}
        label={copyLabel}
        copied={copied}
        copiedLabel={t('actions.copied')}
      />
    </div>
  );
}
