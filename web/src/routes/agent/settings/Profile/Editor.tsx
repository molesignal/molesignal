import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as agentApi from '@/api/agent';
import * as providersApi from '@/api/agent/modelProviders';
import { toApiError } from '@/lib/http';
import { FormDrawer, FormSubmitFooter } from '@/shell/FormDrawer';
import { toast } from '@/shell/ui/sonner';

import {
  ProfileEditorFields,
  ProfileEditorNavigation,
} from './EditorFields';
import {
  createProfileDraft,
  profileEditorPane,
  profileEditorPanes,
  profileInputFromDraft,
  toolsAvailableToAgentProfiles,
  type ProfileDraft,
  type ProfileEditorSection,
  type ProfileEditorTarget,
} from './model';

export type { ProfileEditorTarget } from './model';

export function ProfileEditorDrawer({
  target,
  profiles,
  providers,
  tools,
  onClose,
}: {
  target: ProfileEditorTarget;
  profiles: agentApi.AgentProfile[];
  providers: providersApi.ModelProvider[];
  tools: agentApi.RegisteredTool[];
  onClose: () => void;
}) {
  const { t } = useTranslation('agent');
  const queryClient = useQueryClient();
  const currentProviders = useQuery({
    queryKey: ['agent', 'model-providers'],
    queryFn: providersApi.list,
    enabled: target !== null,
    retry: false,
    refetchOnMount: 'always',
  });
  const profileTools = React.useMemo(
    () => toolsAvailableToAgentProfiles(tools),
    [tools],
  );
  const toolNamesKey = profileTools.map((tool) => tool.name).join('\n');
  const [draft, setDraft] = React.useState<ProfileDraft>(() =>
    createProfileDraft('new', profiles.length, []),
  );
  const [activePane, setActivePane] = React.useState(profileEditorPane('profile'));
  const [formError, setFormError] = React.useState('');

  React.useEffect(() => {
    if (!target) return;
    setDraft(
      createProfileDraft(
        target.profile,
        profiles.length,
        toolNamesKey ? toolNamesKey.split('\n') : [],
      ),
    );
    setActivePane(profileEditorPane(target.section));
    setFormError('');
  }, [profiles.length, target, toolNamesKey]);

  const save = useMutation({
    mutationFn: ({
      id,
      input,
    }: {
      id: string | null;
      input: agentApi.AgentProfileInput;
      section: ProfileEditorSection;
    }) =>
      id ? agentApi.updateProfile(id, input) : agentApi.createProfile(input),
    onSuccess: async (_saved, variables) => {
      await queryClient.invalidateQueries({ queryKey: ['agent', 'profiles'] });
      onClose();
      toast.success(
        t(
          variables.id
            ? sectionUpdatedKey(variables.section)
            : 'settings.profiles.created',
        ),
      );
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const submit = (event: React.FormEvent) => {
    event.preventDefault();
    if (!target) return;
    if (!draft.name.trim()) {
      setActivePane('identity');
      setFormError(t('settings.profiles.name_required'));
      return;
    }
    const input = profileInputFromDraft(
      draft,
      t('settings.profiles.default_description'),
    );
    if (!input) {
      setActivePane('limits');
      setFormError(t('settings.profiles.invalid_limits'));
      return;
    }
    setFormError('');
    save.mutate({
      id: target.profile === 'new' ? null : target.profile.id,
      section: target.section,
      input,
    });
  };

  const section = target?.section ?? 'profile';
  const panes = profileEditorPanes(section);
  const isNew = target?.profile === 'new';
  const titleKey = isNew
    ? 'settings.profiles.create'
    : section === 'profile'
      ? 'settings.profiles.edit'
      : section === 'tools'
        ? 'settings.tools.editor_title'
        : section === 'data'
          ? 'settings.data.editor_title'
          : section === 'network'
            ? 'settings.network.editor_title'
            : 'settings.approval_policy.editor_title';
  const subtitleKey =
    section === 'profile'
      ? 'settings.profiles.editor_description'
      : section === 'tools'
        ? 'settings.tools.editor_description'
        : section === 'data'
          ? 'settings.data.editor_description'
          : section === 'network'
            ? 'settings.network.editor_description'
            : 'settings.approval_policy.editor_description';
  const formId = `agent-profile-${section}-editor`;

  return (
    <FormDrawer
      open={target !== null}
      onOpenChange={(open) => {
        if (!open && !save.isPending) onClose();
      }}
      title={t(titleKey)}
      subtitle={t(subtitleKey)}
      headerDivider={false}
      width={section === 'profile' ? 760 : 640}
      bodyClassName={section === 'profile' ? 'pt-0' : 'pt-5'}
      footer={
        <FormSubmitFooter
          busy={save.isPending}
          onCancel={onClose}
          formId={formId}
          submitLabel={t(isNew ? 'common.create' : 'common.save')}
        />
      }
    >
      <form id={formId} onSubmit={submit}>
        {section === 'profile' ? (
          <>
            <p className="mb-5 border-b border-bd-0 pb-4 text-xs leading-5 text-tx-2">
              {t('settings.profiles.usage_hint')}
            </p>
            <ProfileEditorNavigation
              panes={panes}
              activePane={activePane}
              onChange={(pane) => {
                setActivePane(pane);
                setFormError('');
              }}
            />
          </>
        ) : target?.profile !== 'new' ? (
          <p className="mb-5 border-b border-bd-0 pb-4 text-xs text-tx-2">
            {t('settings.profiles.editing_profile', {
              name: target?.profile.name,
            })}
          </p>
        ) : null}
        {formError && <FormError>{formError}</FormError>}
        <ProfileEditorFields
          pane={activePane}
          draft={draft}
          providers={currentProviders.data ?? providers}
          tools={profileTools}
          onChange={(patch) =>
            setDraft((current) => ({ ...current, ...patch }))
          }
        />
      </form>
    </FormDrawer>
  );
}

function sectionUpdatedKey(section: ProfileEditorSection): string {
  if (section === 'tools') return 'settings.tools.updated';
  if (section === 'data') return 'settings.data.updated';
  if (section === 'network') return 'settings.network.updated';
  if (section === 'approvals') return 'settings.approval_policy.updated';
  return 'settings.profiles.updated';
}

function FormError({ children }: { children: React.ReactNode }) {
  return (
    <div
      role="alert"
      className="mb-5 border-l-2 border-red/60 pl-3 text-sm leading-6 text-red-soft"
    >
      {children}
    </div>
  );
}
