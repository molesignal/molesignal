import { useMutation, useQueryClient } from '@tanstack/react-query';
import { Pencil, Trash2 } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as rumApi from '@/api/rum';
import type { RumComment } from '@/api/rum';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { ChromeButton, IconButton } from '@/shell/chrome';
import { Textarea } from '@/shell/ui/textarea';

import { HEAD, ListScroller, Moment, NoRows, rowClass } from './parts';
import { useKeepInSight, useLitRow, useSeekable } from './useFollow';
import { formatClock } from '../../replay/clock';
import { type ReplayController, replayClockMicros, useReplaySelector } from '../../replay/controller';

const TRACKS = '64px minmax(0,1fr) minmax(64px,120px) 56px';

/** What a comment is at, as the rows of a list order it. */
const atOf = (comment: RumComment) => ({ ...comment, at: comment.atTime });

/**
 * What people said about moments of the session. A comment goes at the moment the replay is at, or at
 * the start of a session that has no replay. Whoever wrote one may change it; a row moves the replay
 * to its moment and the replay lights the row it has reached.
 */
export function CommentList({
  session,
  application,
  comments,
  controller,
  zero,
  queryKey,
}: {
  session: string;
  application: string;
  comments: readonly RumComment[];
  controller: ReplayController;
  /** Where the ruler starts, in microseconds: the moment of a session with no replay. */
  zero: number;
  /** What the page read the comments under, to read them again after a change. */
  queryKey: readonly unknown[];
}) {
  const { t } = useTranslation('rum');
  const client = useQueryClient();
  const list = React.useRef<HTMLDivElement>(null);
  const rows = React.useMemo(() => comments.map(atOf), [comments]);
  const lit = useLitRow(controller, rows);
  const seekable = useSeekable(controller);
  const clock = useReplaySelector(controller, (state) => replayClockMicros(state));
  const access = useActionAccess({ permission: 'rum.replay.comments.write' });
  const [draft, setDraft] = React.useState('');
  const [editing, setEditing] = React.useState<string | null>(null);
  const [edited, setEdited] = React.useState('');
  const at = Math.round(clock ?? zero);

  useKeepInSight(list, lit);

  const reread = () => client.invalidateQueries({ queryKey: [...queryKey] });

  const add = useMutation({
    mutationFn: () => rumApi.addComment(session, { application, atTime: at, body: draft.trim() }),
    onSuccess: () => {
      setDraft('');
      void reread();
    },
  });

  const change = useMutation({
    mutationFn: ({ id, body }: { id: string; body: string }) => rumApi.changeComment(session, id, body),
    onSuccess: () => {
      setEditing(null);
      void reread();
    },
  });

  const remove = useMutation({
    mutationFn: (id: string) => rumApi.removeComment(session, id),
    onSuccess: () => void reread(),
  });

  const failure = add.error ?? change.error ?? remove.error;

  return (
    <div data-rum-comments="">
      {!access.disabled && (
        <form
          className="flex items-start gap-2 border-b border-[var(--row-rule)] px-3 py-2"
          onSubmit={(event) => {
            event.preventDefault();
            if (draft.trim()) add.mutate();
          }}
        >
          <Textarea
            aria-label={t('session_detail.tools.comments.write')}
            data-rum-comment-draft=""
            className="min-h-9 flex-1 py-2 text-xs"
            rows={1}
            value={draft}
            maxLength={2_000}
            placeholder={t('session_detail.tools.comments.placeholder', { time: formatClock((at - zero) / 1_000) })}
            onChange={(event) => setDraft(event.target.value)}
          />
          <ChromeButton type="submit" data-rum-comment-add="" disabled={draft.trim() === '' || add.isPending}>
            {t('session_detail.tools.comments.add')}
          </ChromeButton>
        </form>
      )}
      {failure != null && (
        <p role="alert" className="m-0 bg-red-dim px-3 py-2 font-sans text-xs text-red-soft">
          {toApiError(failure).message}
        </p>
      )}
      {rows.length === 0 ? (
        <NoRows>{t('session_detail.tools.comments.empty')}</NoRows>
      ) : (
        <ListScroller ref={list} label={t('session_detail.tools.comments.aria')} minWidth={480}>
          <div role="row" data-list-head="" className={HEAD} style={{ gridTemplateColumns: TRACKS }}>
            <span role="columnheader">{t('session_detail.tools.comments.time')}</span>
            <span role="columnheader">{t('session_detail.tools.comments.body')}</span>
            <span role="columnheader">{t('session_detail.tools.comments.author')}</span>
            <span role="columnheader" />
          </div>
          {rows.map((row, index) => (
            <div
              key={row.id}
              role="row"
              data-rum-comment={row.id}
              data-current={lit === index ? 'true' : undefined}
              className={rowClass(lit === index, false)}
              style={{ gridTemplateColumns: TRACKS }}
            >
              <Moment
                at={row.at}
                zero={zero}
                seekable={seekable}
                label={t('session_detail.tools.seek', { time: formatClock((row.at - zero) / 1_000, true) })}
                onSeek={() => controller.seekToMicros(row.at)}
              />
              {editing === row.id ? (
                <span role="cell" className="relative z-[1] flex min-w-0 items-start gap-2">
                  <Textarea
                    aria-label={t('session_detail.tools.comments.edit')}
                    className="min-h-9 flex-1 py-2 text-xs"
                    rows={1}
                    value={edited}
                    maxLength={2_000}
                    onChange={(event) => setEdited(event.target.value)}
                  />
                  <ChromeButton disabled={edited.trim() === '' || change.isPending} onClick={() => change.mutate({ id: row.id, body: edited.trim() })}>
                    {t('session_detail.tools.comments.save')}
                  </ChromeButton>
                  <ChromeButton variant="ghost" onClick={() => setEditing(null)}>
                    {t('session_detail.tools.comments.cancel')}
                  </ChromeButton>
                </span>
              ) : (
                <span role="cell" className="min-w-0 whitespace-pre-wrap break-words text-tx-0">
                  {row.body}
                </span>
              )}
              <span role="cell" className="min-w-0 truncate text-tx-3" title={row.authorName ?? undefined}>
                {row.authorName ?? t('session_detail.tools.comments.nobody')}
              </span>
              <span role="cell" className="relative z-[1] flex items-center justify-end gap-1">
                {row.editable && editing !== row.id && (
                  <>
                    <IconButton
                      aria-label={t('session_detail.tools.comments.edit')}
                      onClick={() => {
                        setEditing(row.id);
                        setEdited(row.body);
                      }}
                    >
                      <Pencil aria-hidden className="h-3.5 w-3.5" />
                    </IconButton>
                    <IconButton
                      aria-label={t('session_detail.tools.comments.delete')}
                      disabled={remove.isPending}
                      onClick={() => remove.mutate(row.id)}
                    >
                      <Trash2 aria-hidden className="h-3.5 w-3.5" />
                    </IconButton>
                  </>
                )}
              </span>
            </div>
          ))}
        </ListScroller>
      )}
    </div>
  );
}
