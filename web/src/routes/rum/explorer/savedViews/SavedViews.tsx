import { Bookmark, Trash2 } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { ChromeButton } from '@/shell/chrome';
import { cn } from '@/shell/lib/cn';
import { QueryIconAction } from '@/shell/query/IconAction';
import { Popover, PopoverContent, PopoverTrigger } from '@/shell/ui/popover';
import { useUiPreference } from '@/shell/uiPreference/useUiPreference';

import {
  canSaveView,
  MAX_VIEW_NAME,
  parseSavedViews,
  type RumSavedView,
  SAVED_VIEWS_KEY,
  SAVED_VIEWS_SCOPE,
  withoutSavedView,
  withSavedView,
} from './model';

const NO_VIEWS: readonly RumSavedView[] = [];

/**
 * The views of the explorer one keeps to come back to: a name for what is on screen, which opens it
 * again. They are one's own, and the address of a view is what is sent to someone else.
 */
export function RumSavedViews({ search, onOpen }: { search: string; onOpen: (search: string) => void }) {
  const { t } = useTranslation('rum');
  const preference = useUiPreference(SAVED_VIEWS_SCOPE, SAVED_VIEWS_KEY, parseSavedViews);
  const views = preference.value?.views ?? NO_VIEWS;
  const [open, setOpen] = React.useState(false);
  const [name, setName] = React.useState('');
  const ready = canSaveView(views, name, search);

  const save = () => {
    if (!ready) return;

    preference.save({ views: withSavedView(views, name, search) });
    setName('');
  };

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <QueryIconAction icon={Bookmark} label={t('explorer.saved_views.label')} />
      </PopoverTrigger>
      <PopoverContent align="end" className="w-[320px] space-y-2 p-2" data-rum-saved-views="">
        {views.length === 0 ? (
          <p className="m-0 px-2 py-1 font-sans text-xs text-tx-3">{t('explorer.saved_views.none')}</p>
        ) : (
          <ul className="m-0 max-h-[280px] list-none space-y-0.5 overflow-auto p-0">
            {views.map((view) => (
              <li key={view.id} className="group flex items-center gap-1 rounded-md hover:bg-bg-3">
                <button
                  type="button"
                  data-rum-saved-view={view.id}
                  className={cn(
                    'min-w-0 flex-1 truncate rounded-md px-2 py-1.5 text-left font-sans text-xs outline-none focus-visible:bg-bg-3',
                    view.search === search ? 'font-strong text-[var(--blue-ink)]' : 'text-tx-1',
                  )}
                  onClick={() => {
                    onOpen(view.search);
                    setOpen(false);
                  }}
                >
                  {view.name}
                </button>
                <button
                  type="button"
                  aria-label={t('explorer.saved_views.delete', { name: view.name })}
                  className="grid h-6 w-6 shrink-0 place-items-center rounded-md text-tx-3 outline-none hover:text-tx-0 focus-visible:bg-bg-3 focus-visible:text-tx-0"
                  onClick={() => preference.save({ views: withoutSavedView(views, view.id) })}
                >
                  <Trash2 aria-hidden="true" className="h-3 w-3" />
                </button>
              </li>
            ))}
          </ul>
        )}
        <form
          className="flex items-center gap-1.5"
          onSubmit={(event) => {
            event.preventDefault();
            save();
          }}
        >
          <input
            value={name}
            maxLength={MAX_VIEW_NAME}
            aria-label={t('explorer.saved_views.name')}
            placeholder={t('explorer.saved_views.name')}
            onChange={(event) => setName(event.target.value)}
            className="h-8 min-w-0 flex-1 rounded-md border-0 bg-[var(--control-surface)] px-2 font-sans text-xs text-tx-0 outline-none placeholder:text-tx-3 focus-visible:bg-bg-3"
          />
          <ChromeButton type="submit" disabled={!ready}>
            {t('explorer.saved_views.save')}
          </ChromeButton>
        </form>
      </PopoverContent>
    </Popover>
  );
}
