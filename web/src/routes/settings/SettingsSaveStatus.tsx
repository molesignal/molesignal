import { AlertCircle, Check, LoaderCircle } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { cn } from '@/shell/lib/cn';

type SaveState = 'saved' | 'dirty' | 'saving' | 'error';

interface SettingsSaveStatusValue {
  beginSave: () => void;
  completeSave: () => void;
  failSave: () => void;
  setDraftDirty: (key: string, dirty: boolean) => void;
}

const SettingsSaveStatusContext = React.createContext<SettingsSaveStatusValue | null>(null);
const SettingsSaveStateContext = React.createContext<SaveState>('saved');

export function SettingsSaveStatusProvider({ children }: { children: React.ReactNode }) {
  const [state, setState] = React.useState<SaveState>('saved');
  const pendingCount = React.useRef(0);
  const dirtyDrafts = React.useRef(new Set<string>());

  const beginSave = React.useCallback(() => {
    pendingCount.current += 1;
    setState('saving');
  }, []);

  const completeSave = React.useCallback(() => {
    pendingCount.current = Math.max(0, pendingCount.current - 1);
    if (pendingCount.current === 0) {
      setState(dirtyDrafts.current.size > 0 ? 'dirty' : 'saved');
    }
  }, []);

  const failSave = React.useCallback(() => {
    pendingCount.current = Math.max(0, pendingCount.current - 1);
    setState('error');
  }, []);

  const setDraftDirty = React.useCallback((key: string, dirty: boolean) => {
    const currentlyDirty = dirtyDrafts.current.has(key);
    if (currentlyDirty === dirty) return;
    if (dirty) dirtyDrafts.current.add(key);
    else dirtyDrafts.current.delete(key);
    if (pendingCount.current === 0) {
      setState(dirtyDrafts.current.size > 0 ? 'dirty' : 'saved');
    }
  }, []);

  const value = React.useMemo(
    () => ({ beginSave, completeSave, failSave, setDraftDirty }),
    [beginSave, completeSave, failSave, setDraftDirty],
  );

  return (
    <SettingsSaveStatusContext.Provider value={value}>
      <SettingsSaveStateContext.Provider value={state}>
        {children}
      </SettingsSaveStateContext.Provider>
    </SettingsSaveStatusContext.Provider>
  );
}

export function useSettingsSaveStatus(): SettingsSaveStatusValue {
  const value = React.useContext(SettingsSaveStatusContext);
  if (!value) {
    throw new Error('useSettingsSaveStatus must be used within SettingsSaveStatusProvider');
  }
  return value;
}

export function SettingsSaveStatusIndicator() {
  const { t } = useTranslation('settings-admin');
  const state = React.useContext(SettingsSaveStateContext);
  const saving = state === 'saving';
  const error = state === 'error';
  const dirty = state === 'dirty';

  return (
    <div
      role="status"
      aria-live="polite"
      className={cn(
        'inline-flex h-8 items-center gap-1.5 rounded-md px-2.5 font-sans text-xs font-strong',
        error
          ? 'bg-red-dim text-red-soft'
          : dirty
            ? 'bg-yellow-dim text-yellow-soft'
            : 'text-tx-2',
      )}
    >
      {saving ? (
        <LoaderCircle className="h-3.5 w-3.5 animate-spin" />
      ) : error ? (
        <AlertCircle className="h-3.5 w-3.5" />
      ) : dirty ? (
        <span className="h-1.5 w-1.5 rounded-full bg-yellow" />
      ) : (
        <Check className="h-3.5 w-3.5 text-green-soft" />
      )}
      <span>{t(`save_status.${state}`)}</span>
    </div>
  );
}
