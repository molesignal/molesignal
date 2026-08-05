import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { useKeyboard } from '@/keyboard/controller';
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/shell/ui/dialog';
import { Kbd } from '@/shell/ui/kbd';
import { useKeyboardScope } from '@/stores/useKeyboardScope';

interface HelpOverlayProps {
  open: boolean;
  onOpenChange: (v: boolean) => void;
}

export function HelpOverlay({ open, onOpenChange }: HelpOverlayProps) {
  const { t } = useTranslation('keyboard');
  const ctrl = useKeyboard();
  const scope = useKeyboardScope((s) => s.peek());

  const byCategory = React.useMemo(() => {
    const bindings = open ? ctrl.listBindings(scope) : [];
    const out: Record<string, typeof bindings> = {};
    for (const b of bindings) {
      const c = b.category ?? 'other';
      if (!out[c]) out[c] = [];
      out[c]!.push(b);
    }
    return out;
  }, [open, ctrl, scope]);

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-2xl">
        <DialogHeader>
          <DialogTitle>{t('help_title')}</DialogTitle>
        </DialogHeader>
        <div className="grid grid-cols-2 gap-x-6 gap-y-1">
          {Object.entries(byCategory).map(([cat, items]) => (
            <div key={cat} className="space-y-1">
              <div className="mb-1 text-xs font-semibold tracking-normal text-muted-foreground">
                {cat}
              </div>
              {items.map((b) => (
                <div key={b.keys} className="flex items-center justify-between text-xs">
                  <span className="text-foreground">{b.description}</span>
                  <span className="flex gap-1">
                    {b.keys.split(' ').map((k) => (
                      <Kbd key={k}>{prettify(k)}</Kbd>
                    ))}
                  </span>
                </div>
              ))}
            </div>
          ))}
        </div>
      </DialogContent>
    </Dialog>
  );
}

function prettify(key: string): string {
  return key
    .replace('mod', '⌘')
    .replace(/\+/g, ' ')
    .replace('esc', 'Esc')
    .replace('enter', 'Enter')
    .replace('shift', '⇧')
    .replace('alt', '⌥')
    .trim();
}
