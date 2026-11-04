import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { RumApplication, RumPlaylist } from '@/api/rum';
import { toApiError } from '@/lib/http';
import { FormDrawer, FormField, FormInput, FormSection, FormSelect, FormSubmitFooter } from '@/shell/FormDrawer';

import { usePlaylistChanges } from './usePlaylists';

const MAX_NAME = 128;
const MAX_DESCRIPTION = 1_000;

/**
 * The form of a playlist: a new one has a name, a description and the application it is of; a
 * playlist that exists has its name and description changed, and the application stays.
 */
export function PlaylistDrawer({
  open,
  onOpenChange,
  playlist,
  applications,
  application,
  onSaved,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** The playlist to change; none makes a new one. */
  playlist?: RumPlaylist | undefined;
  applications: readonly RumApplication[];
  /** The application a new playlist is of unless another is chosen. */
  application?: string | undefined;
  /** The id of the playlist that was made or changed. */
  onSaved?: ((id: string) => void) | undefined;
}) {
  const { t } = useTranslation('rum');
  const { create, update } = usePlaylistChanges();
  const formId = React.useId();
  const [name, setName] = React.useState('');
  const [description, setDescription] = React.useState('');
  const [chosen, setChosen] = React.useState('');

  // A form opened again starts from what the playlist says, or from nothing.
  React.useEffect(() => {
    if (!open) return;

    setName(playlist?.name ?? '');
    setDescription(playlist?.description ?? '');
    setChosen(playlist?.application || application || applications[0]?.id || '');
    create.reset();
    update.reset();
    // eslint-disable-next-line react-hooks/exhaustive-deps -- the mutations are reset once, when the form opens
  }, [open, playlist, application, applications]);

  const busy = create.isPending || update.isPending;
  const invalid = name.trim() === '' || (!playlist && chosen === '');
  const error = create.error ?? update.error;

  const save = async () => {
    if (invalid || busy) return;

    const saved = playlist
      ? await update.mutateAsync({ id: playlist.id, name: name.trim(), description })
      : await create.mutateAsync({ application: chosen, name: name.trim(), description });

    onOpenChange(false);
    onSaved?.(saved.id);
  };

  return (
    <FormDrawer
      open={open}
      onOpenChange={onOpenChange}
      width={520}
      title={playlist ? t('playlists.form.edit_title') : t('playlists.form.create_title')}
      subtitle={t('playlists.form.subtitle')}
      footer={
        <FormSubmitFooter
          formId={formId}
          busy={busy}
          invalid={invalid}
          onCancel={() => onOpenChange(false)}
          submitLabel={playlist ? t('playlists.form.save') : t('playlists.form.create')}
        />
      }
    >
      <form
        id={formId}
        onSubmit={(event) => {
          event.preventDefault();
          void save().catch(() => undefined);
        }}
      >
        <FormSection>
          <FormField label={t('playlists.form.name')} required>
            <FormInput
              value={name}
              onChange={(event) => setName(event.target.value)}
              placeholder={t('playlists.form.name_placeholder')}
              maxLength={MAX_NAME}
              autoComplete="off"
              autoFocus
            />
          </FormField>
          <FormField label={t('playlists.form.description')} hint={t('playlists.form.description_hint')}>
            <FormInput
              value={description}
              onChange={(event) => setDescription(event.target.value)}
              maxLength={MAX_DESCRIPTION}
              autoComplete="off"
            />
          </FormField>
          <FormField label={t('playlists.form.application')} {...(playlist ? { hint: t('playlists.form.application_fixed') } : {})}>
            <FormSelect
              value={chosen}
              onChange={setChosen}
              ariaLabel={t('playlists.form.application')}
              disabled={Boolean(playlist)}
              options={applications.map((entry) => ({ value: entry.id, label: entry.name }))}
            />
          </FormField>
        </FormSection>
        {error && (
          <p role="alert" className="m-0 rounded-md bg-red-dim px-3 py-2 font-sans text-sm text-red-soft">
            {toApiError(error).message}
          </p>
        )}
      </form>
    </FormDrawer>
  );
}
