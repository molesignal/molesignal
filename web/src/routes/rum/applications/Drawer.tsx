import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Eye, EyeOff } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import { ConfirmDialog } from '@/admin';
import * as apiTokensApi from '@/api/apiTokens';
import * as rumApi from '@/api/rum';
import { SCRUB_KINDS } from '@/api/rum';
import type { RumApplication, RumApplicationSettings } from '@/api/rum';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { ChromeButton, IconButton } from '@/shell/chrome';
import { CopyField } from '@/shell/CopyField';
import { FormDrawer, FormField, FormInput, FormSection, FormSelect } from '@/shell/FormDrawer';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/shell/ui/tooltip';

import { guideSourceOf, MAX_ISSUES_PER_HOUR, sameSettings, validSettings, withScrub } from './model';
import { RetentionSection } from './RetentionSection';
import { SdkSection } from './SdkSection';
import { SegmentsSection } from './SegmentsSection';
import { SettingSwitch } from './SettingSwitch';

/** The mask a token wears until it is asked for. */
function maskedToken(token: string): string {
  if (!token) return '';
  if (token.length <= 12) return `${token.slice(0, 4)}••••`;
  return `${token.slice(0, 8)}${'•'.repeat(14)}${token.slice(-6)}`;
}

/**
 * One application: the client token its SDKs are given, how its data is collected, and the way to
 * set an SDK up. The token is write-only and bound to this application.
 */
export function ApplicationDrawer({
  application,
  onClose,
}: {
  application: RumApplication | null;
  onClose: () => void;
}) {
  const { t } = useTranslation('rum');
  const client = useQueryClient();
  const manage = useActionAccess({ permission: 'rum.applications.manage' });
  const [revealed, setRevealed] = React.useState(false);
  const [settings, setSettings] = React.useState<RumApplicationSettings | null>(null);
  const [confirmingDelete, setConfirmingDelete] = React.useState(false);
  const id = application?.id ?? '';

  // Another application starts from its own settings, with its token hidden.
  React.useEffect(() => {
    setSettings(application?.settings ?? null);
    setRevealed(false);
    setConfirmingDelete(false);
  }, [application]);

  const token = useQuery({
    queryKey: ['rum', 'client-token', id],
    queryFn: () => apiTokensApi.getRumClient(id),
    enabled: id !== '' && manage.allowed,
    staleTime: 60_000,
    retry: false,
  });

  const save = useMutation({
    mutationFn: (next: RumApplicationSettings) => rumApi.updateApplication(id, { settings: next }),
    onSuccess: async () => {
      await client.invalidateQueries({ queryKey: ['rum', 'applications'] });
    },
  });
  const remove = useMutation({
    mutationFn: () => rumApi.deleteApplication(id),
    onSuccess: async () => {
      await client.invalidateQueries({ queryKey: ['rum', 'applications'] });
      setConfirmingDelete(false);
      onClose();
    },
  });

  const dirty = settings !== null && application !== null && !sameSettings(settings, application.settings);
  const valid = settings !== null && validSettings(settings);
  const guide = application ? guideSourceOf(application.platform) : null;
  const tokenText = token.data?.token ?? '';
  const revealLabel = revealed ? t('applications.drawer.hide_token') : t('applications.drawer.show_token');

  return (
    <>
      <FormDrawer
        open={application !== null}
        onOpenChange={(open) => !open && onClose()}
        width={520}
        title={application?.name ?? ''}
        subtitle={application ? `${application.id} · ${t(`applications.platforms.${application.platform}`)}` : undefined}
        footer={
          <>
            <ChromeButton type="button" onClick={onClose}>
              {t('applications.drawer.close')}
            </ChromeButton>
            <ChromeButton
              type="button"
              variant="primary"
              disabled={!dirty || !valid || save.isPending || manage.disabled}
              disabledReason={manage.reason}
              onClick={() => settings && save.mutate(settings)}
            >
              {save.isPending ? t('applications.drawer.saving') : t('applications.drawer.save')}
            </ChromeButton>
          </>
        }
      >
        {application && settings && (
          <>
            <FormSection
              title={t('applications.drawer.token_title')}
              description={t('applications.drawer.token_description')}
            >
              {!manage.allowed ? (
                <p className="m-0 font-sans text-sm text-tx-3">{manage.reason}</p>
              ) : token.isError ? (
                <p role="alert" className="m-0 rounded-md bg-red-dim px-3 py-2 font-sans text-sm text-red-soft">
                  {toApiError(token.error).message}
                </p>
              ) : (
                <CopyField
                  value={tokenText}
                  copyLabel={t('applications.drawer.copy_token')}
                  disabled={token.isLoading || !tokenText}
                  actions={
                    <Tooltip>
                      <TooltipTrigger asChild>
                        <span className="inline-flex shrink-0">
                          <IconButton
                            aria-label={revealLabel}
                            aria-pressed={revealed}
                            disabled={token.isLoading || !tokenText}
                            onClick={() => setRevealed((current) => !current)}
                          >
                            {revealed ? <EyeOff aria-hidden className="h-3.5 w-3.5" /> : <Eye aria-hidden className="h-3.5 w-3.5" />}
                          </IconButton>
                        </span>
                      </TooltipTrigger>
                      <TooltipContent side="top">{revealLabel}</TooltipContent>
                    </Tooltip>
                  }
                >
                  {revealed ? tokenText : maskedToken(tokenText)}
                </CopyField>
              )}
            </FormSection>

            <FormSection
              title={t('applications.drawer.collection_title')}
              description={t('applications.drawer.collection_description')}
            >
              <SettingSwitch
                label={t('applications.drawer.collect_ip')}
                hint={t('applications.drawer.collect_ip_hint')}
                checked={settings.collectIp}
                disabled={manage.disabled}
                onChange={(collect_ip) => setSettings({ ...settings, collectIp: collect_ip })}
              />
              <SettingSwitch
                label={t('applications.drawer.collect_geo')}
                hint={t('applications.drawer.collect_geo_hint')}
                checked={settings.collectGeo}
                disabled={manage.disabled}
                onChange={(collect_geo) => setSettings({ ...settings, collectGeo: collect_geo })}
              />
              <FormField label={t('applications.drawer.privacy')} hint={t('applications.drawer.privacy_hint')}>
                <FormSelect
                  value={settings.defaultPrivacy}
                  onChange={(value) =>
                    setSettings({ ...settings, defaultPrivacy: value === 'allow' || value === 'mask-user-input' ? value : 'mask' })
                  }
                  ariaLabel={t('applications.drawer.privacy')}
                  disabled={manage.disabled}
                  options={[
                    { value: 'mask', label: t('applications.drawer.privacy_mask') },
                    { value: 'mask-user-input', label: t('applications.drawer.privacy_mask_user_input') },
                    { value: 'allow', label: t('applications.drawer.privacy_allow') },
                  ]}
                />
              </FormField>
              {save.isError && (
                <p role="alert" className="m-0 rounded-md bg-red-dim px-3 py-2 font-sans text-sm text-red-soft">
                  {toApiError(save.error).message}
                </p>
              )}
            </FormSection>

            <FormSection
              title={t('applications.drawer.scrub_title')}
              description={t('applications.drawer.scrub_description')}
            >
              {SCRUB_KINDS.map((kind) => (
                <SettingSwitch
                  key={kind}
                  label={t(`applications.drawer.scrub.${kind}`)}
                  hint={t(`applications.drawer.scrub.${kind}_hint`)}
                  checked={settings.scrub.includes(kind)}
                  disabled={manage.disabled}
                  onChange={(on) => setSettings({ ...settings, scrub: withScrub(settings.scrub, kind, on) })}
                />
              ))}
            </FormSection>

            <RetentionSection settings={settings} onChange={setSettings} disabled={manage.disabled} />

            <SdkSection settings={settings} onChange={setSettings} disabled={manage.disabled} />

            <SegmentsSection settings={settings} onChange={setSettings} disabled={manage.disabled} />

            <FormSection
              title={t('applications.drawer.issues_title')}
              description={t('applications.drawer.issues_description')}
            >
              <FormField
                label={t('applications.drawer.issues_per_hour')}
                hint={t('applications.drawer.issues_per_hour_hint')}
              >
                <FormInput
                  type="number"
                  inputMode="numeric"
                  min={1}
                  max={MAX_ISSUES_PER_HOUR}
                  step={1}
                  value={Number.isNaN(settings.issuesPerHour) ? '' : String(settings.issuesPerHour)}
                  onChange={(event) =>
                    setSettings({ ...settings, issuesPerHour: event.target.value === '' ? Number.NaN : Number(event.target.value) })
                  }
                  aria-label={t('applications.drawer.issues_per_hour')}
                  aria-invalid={!valid || undefined}
                  disabled={manage.disabled}
                  disabledReason={manage.reason}
                  className="w-28 tabular-nums"
                />
              </FormField>
            </FormSection>

            <FormSection
              title={t('applications.drawer.setup_title')}
              description={t('applications.drawer.setup_description')}
            >
              {guide ? (
                <Link
                  to={`/datasource/recommended/${guide}?app=${encodeURIComponent(application.id)}`}
                  className="w-fit font-sans text-sm font-strong text-[var(--blue-ink)] hover:underline focus-visible:underline"
                >
                  {t('applications.drawer.open_guide')}
                </Link>
              ) : (
                <p className="m-0 font-sans text-sm text-tx-3">{t('applications.drawer.no_guide')}</p>
              )}
            </FormSection>

            <FormSection
              title={t('applications.drawer.delete_title')}
              description={t('applications.drawer.delete_description')}
            >
              <div>
                <ChromeButton
                  type="button"
                  disabled={manage.disabled}
                  disabledReason={manage.reason}
                  onClick={() => setConfirmingDelete(true)}
                >
                  {t('applications.drawer.delete')}
                </ChromeButton>
              </div>
            </FormSection>
          </>
        )}
      </FormDrawer>
      <ConfirmDialog
        open={confirmingDelete}
        onOpenChange={setConfirmingDelete}
        title={t('applications.drawer.delete_confirm_title', { name: application?.name ?? '' })}
        description={t('applications.drawer.delete_confirm_description')}
        confirmLabel={t('applications.drawer.delete')}
        cancelLabel={t('applications.drawer.cancel')}
        destructive
        busy={remove.isPending}
        onConfirm={() => remove.mutate()}
      />
    </>
  );
}
