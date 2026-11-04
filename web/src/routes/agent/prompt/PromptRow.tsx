import { Check, CopyPlus, FileText, Pencil, RotateCcw, Star, Trash2 } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import type * as promptsApi from '@/api/agent/prompts';
import { formatMicrosActive } from '@/lib/time';
import { cn } from '@/shell/lib/cn';
import { Badge } from '@/shell/ui/badge';
import { Button } from '@/shell/ui/button';

export type PromptRowAction =
  | { kind: 'set_default'; prompt: promptsApi.AgentPrompt }
  | { kind: 'restore'; prompt: promptsApi.AgentPrompt }
  | { kind: 'delete'; prompt: promptsApi.AgentPrompt };

/** One prompt in the list: what it is, whether it is in use, and what can be done with it. */
export function PromptRow({
  prompt,
  effective,
  canManage,
  busy,
  onView,
  onEdit,
  onOverride,
  onAction,
}: {
  prompt: promptsApi.AgentPrompt;
  effective: boolean;
  canManage: boolean;
  busy: boolean;
  onView: () => void;
  onEdit: () => void;
  onOverride: () => void;
  onAction: (action: PromptRowAction) => void;
}) {
  const { t } = useTranslation('agent');
  const builtin = prompt.scope === 'builtin';
  return (
    <article className="flex flex-col gap-3 px-4 py-3 lg:flex-row lg:items-center">
      <div
        className={cn(
          'grid h-9 w-9 shrink-0 place-items-center rounded-md border',
          effective
            ? 'border-indigo/30 bg-indigo/10 text-indigo'
            : 'border-bd-0 bg-bg-2 text-tx-3',
        )}
      >
        {effective ? (
          <Check className="h-4 w-4" />
        ) : (
          <FileText className="h-4 w-4" />
        )}
      </div>
      <div className="min-w-0 flex-1">
        <div className="flex flex-wrap items-center gap-2">
          <h4 className="font-strong text-tx-0">{prompt.name}</h4>
          <Badge variant="outline">
            {t(`settings.prompts.scopes.${prompt.scope}`)}
          </Badge>
          {effective && (
            <Badge variant="accent">
              {t('settings.prompts.effective')}
            </Badge>
          )}
          {!prompt.enabled && (
            <Badge variant="secondary">{t('status.disabled')}</Badge>
          )}
        </div>
        <p className="mt-1 line-clamp-1 font-mono text-xs text-tx-3">
          {prompt.body}
        </p>
        <div className="mt-1.5 flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-tx-3">
          <span>{t('settings.prompts.version', { version: prompt.version })}</span>
          {/* A built-in prompt was not saved by anyone: it has no time, and a time of zero is not 1970. */}
          {prompt.updatedAt > 0 && <span>{formatMicrosActive(prompt.updatedAt)}</span>}
          {prompt.builtinKey && (
            <span className="font-mono">{prompt.builtinKey}</span>
          )}
        </div>
      </div>
      <div className="flex shrink-0 flex-wrap items-center gap-1.5 lg:justify-end">
        <Button size="sm" variant="ghost" onClick={onView}>
          {t('settings.prompts.view')}
        </Button>
        {builtin ? (
          <Button size="sm" variant="outline" disabled={!canManage} onClick={onOverride}>
            <CopyPlus /> {t('settings.prompts.override')}
          </Button>
        ) : (
          <>
            <Button size="sm" variant="outline" disabled={!canManage} onClick={onEdit}>
              <Pencil /> {t('common.edit')}
            </Button>
            {!prompt.isDefault && (
              <Button
                size="sm"
                variant="ghost"
                disabled={busy || !canManage}
                onClick={() => onAction({ kind: 'set_default', prompt })}
              >
                <Star /> {t('settings.prompts.set_default')}
              </Button>
            )}
            {prompt.builtinKey && (
              <Button
                size="sm"
                variant="ghost"
                disabled={busy || !canManage}
                onClick={() => onAction({ kind: 'restore', prompt })}
              >
                <RotateCcw /> {t('settings.prompts.restore')}
              </Button>
            )}
            <Button
              size="sm"
              variant="ghost"
              disabled={busy || !canManage}
              onClick={() => onAction({ kind: 'delete', prompt })}
              className="text-red hover:text-red"
            >
              <Trash2 /> {t('settings.prompts.delete')}
            </Button>
          </>
        )}
      </div>
    </article>
  );
}
