import * as React from 'react';

interface AutoSaveOptions {
  fingerprint: string;
  enabled: boolean;
  busy: boolean;
  delay?: number;
  save: () => Promise<unknown>;
}

/**
 * Saves the latest draft once per distinct value. A failed value is not
 * retried in a loop; it waits for either an explicit retry or another edit.
 */
export function useAutoSave({
  fingerprint,
  enabled,
  busy,
  delay = 700,
  save,
}: AutoSaveOptions) {
  const saveRef = React.useRef(save);
  const attemptedFingerprint = React.useRef<string | null>(null);

  React.useEffect(() => {
    saveRef.current = save;
  }, [save]);

  React.useEffect(() => {
    if (!enabled) attemptedFingerprint.current = null;
  }, [enabled]);

  React.useEffect(() => {
    if (
      !enabled ||
      busy ||
      attemptedFingerprint.current === fingerprint
    ) {
      return;
    }
    const timer = window.setTimeout(() => {
      attemptedFingerprint.current = fingerprint;
      void saveRef.current().catch(() => undefined);
    }, delay);
    return () => window.clearTimeout(timer);
  }, [busy, delay, enabled, fingerprint]);

  return React.useCallback(() => {
    if (!enabled || busy) return;
    attemptedFingerprint.current = fingerprint;
    void saveRef.current().catch(() => undefined);
  }, [busy, enabled, fingerprint]);
}
