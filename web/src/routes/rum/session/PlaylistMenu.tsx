import { Check, ListPlus } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { RumRow } from '@/api/rum';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { ChromeButton } from '@/shell/chrome';
import { cn } from '@/shell/lib/cn';
import { Popover, PopoverContent, PopoverTrigger } from '@/shell/ui/popover';

import { usePlaylistChanges, usePlaylists, useSessionPlaylists } from '../playlists/usePlaylists';
import { type ReplayController, replayClockMicros, useReplaySelector } from '../replay/controller';

/**
 * Puts the session in a playlist of its application, or takes it out, from the page of the session:
 * the playlists are listed with a mark on those it is in. A new playlist can be made with it. The
 * session can be opened at the moment the replay is at when it is put in.
 */
export function PlaylistMenu({
  id,
  application,
  controller,
}: {
  id: string;
  session?: RumRow | undefined;
  application: string;
  controller: ReplayController;
}) {
  const { t } = useTranslation('rum');
  const manage = useActionAccess({ permission: 'rum.playlists.manage' });
  const [open, setOpen] = React.useState(false);
  const [atMoment, setAtMoment] = React.useState(false);
  const [name, setName] = React.useState('');
  const known = useReplaySelector(controller, (state) => state.origin !== null);

  const playlists = usePlaylists(application, open);
  const member = useSessionPlaylists(id, open);
  const changes = usePlaylistChanges();
  const inside = new Set((member.data ?? []).map((playlist) => playlist.id));
  const busy = changes.add.isPending || changes.take.isPending || changes.create.isPending;
  const error = changes.add.error ?? changes.take.error ?? changes.create.error;

  /** The session as a playlist is told it: with the moment of the replay when asked. */
  const entry = () => {
    const at = atMoment && known ? replayClockMicros(controller.getState()) : null;

    return { sessionId: id, ...(at !== null && at > 0 ? { atTime: Math.round(at) } : {}) };
  };

  const toggle = (playlist: string) => {
    if (inside.has(playlist)) changes.take.mutate({ id: playlist, sessionId: id });
    else changes.add.mutate({ id: playlist, sessions: [entry()] });
  };

  const create = () => {
    const title = name.trim();

    if (title === '') return;

    changes.create.mutate(
      { application, name: title, sessions: [entry()] },
      { onSuccess: () => setName('') },
    );
  };

  if (manage.disabled) return null;

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <ChromeButton data-rum-playlist-menu="" aria-haspopup="dialog">
          <ListPlus aria-hidden className="h-3.5 w-3.5" />
          {t('playlists.menu.button')}
        </ChromeButton>
      </PopoverTrigger>
      <PopoverContent align="end" className="w-[320px] space-y-3 p-3">
        <div>
          <p className="m-0 font-sans text-sm font-strong text-tx-0">{t('playlists.menu.title')}</p>
          <p className="m-0 mt-0.5 font-sans text-xs text-tx-3">{t('playlists.menu.description')}</p>
        </div>

        <ul role="group" aria-label={t('playlists.menu.title')} className="m-0 max-h-56 list-none space-y-0.5 overflow-y-auto p-0">
          {(playlists.data ?? []).map((playlist) => {
            const on = inside.has(playlist.id);

            return (
              <li key={playlist.id}>
                <button
                  type="button"
                  role="checkbox"
                  aria-checked={on}
                  data-rum-playlist-option={playlist.id}
                  disabled={busy}
                  onClick={() => toggle(playlist.id)}
                  className="flex min-h-9 w-full items-center gap-2 rounded-md border-0 bg-transparent px-2 text-left font-sans text-sm text-tx-1 outline-none hover:bg-bg-3 focus-visible:bg-bg-3 disabled:pointer-events-none disabled:text-[var(--tx-disabled)]"
                >
                  <span
                    aria-hidden
                    className={cn('grid h-4 w-4 shrink-0 place-items-center rounded-[4px]', on ? 'bg-[var(--indigo)] text-white' : 'bg-bg-3')}
                  >
                    {on && <Check className="h-3 w-3" />}
                  </span>
                  <span className="min-w-0 flex-1 truncate">{playlist.name}</span>
                  <span className="shrink-0 tabular-nums text-xs text-tx-3">{playlist.sessionCount}</span>
                </button>
              </li>
            );
          })}
          {playlists.isSuccess && playlists.data.length === 0 && (
            <li className="px-2 py-2 font-sans text-xs text-tx-3">{t('playlists.menu.none')}</li>
          )}
        </ul>

        {known && (
          <label className="flex cursor-pointer items-center gap-2 font-sans text-xs text-tx-2">
            <input
              type="checkbox"
              data-rum-playlist-moment=""
              checked={atMoment}
              onChange={(event) => setAtMoment(event.target.checked)}
              className="h-3.5 w-3.5"
            />
            {t('playlists.menu.at_moment')}
          </label>
        )}

        <form
          className="flex items-center gap-2 border-t border-bd-0 pt-3"
          onSubmit={(event) => {
            event.preventDefault();
            create();
          }}
        >
          <input
            value={name}
            onChange={(event) => setName(event.target.value)}
            placeholder={t('playlists.menu.new_placeholder')}
            aria-label={t('playlists.menu.new_name')}
            maxLength={128}
            autoComplete="off"
            className="h-8 min-w-0 flex-1 rounded-md border-0 bg-[var(--control-surface)] px-2.5 font-sans text-xs text-tx-0 outline-none placeholder:text-tx-3 focus-visible:bg-bg-3"
          />
          <ChromeButton type="submit" data-rum-playlist-new="" disabled={busy || name.trim() === ''}>
            {t('playlists.menu.new')}
          </ChromeButton>
        </form>

        {error && (
          <p role="alert" className="m-0 font-sans text-xs text-red-soft">
            {toApiError(error).message}
          </p>
        )}
      </PopoverContent>
    </Popover>
  );
}
