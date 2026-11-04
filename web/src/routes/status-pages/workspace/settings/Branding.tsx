import { useMutation } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as statusPagesApi from '@/api/statusPages';
import { toApiError } from '@/lib/http';
import { ChromeButton } from '@/shell/chrome';
import { FormField, FormInput } from '@/shell/FormDrawer';
import { toast } from '@/shell/ui/sonner';

import { StatusPageLogoField } from '../../StatusPageLogoField';
import { useStatusPageWorkspace } from '../Layout';
import { statusPageInput } from './pageInput';
import { SettingsCard, SettingsFooter } from './SettingsSection';

export function StatusPageBrandingSettings() {
  const { t } = useTranslation('status-pages');
  const { pageId, snapshot, refresh } = useStatusPageWorkspace();
  const page = snapshot.page;
  const editable = page.lifecycle === 'active';
  const [brandColor, setBrandColor] = React.useState(page.brandColor);
  const [logoUrl, setLogoUrl] = React.useState(page.logoUrl ?? '');
  const [logoFile, setLogoFile] = React.useState<File | null>(null);
  React.useEffect(() => {
    setBrandColor(page.brandColor);
    setLogoUrl(page.logoUrl ?? '');
    setLogoFile(null);
  }, [page]);
  const mutation = useMutation({
    mutationFn: async () => {
      let updated = await statusPagesApi.update(pageId, statusPageInput(page, {
        brandColor: brandColor.toUpperCase(),
        logoUrl: logoFile ? page.logoUrl : logoUrl.trim() || null,
      }));
      if (logoFile) updated = await statusPagesApi.uploadLogo(pageId, logoFile);
      return updated;
    },
    onSuccess: async () => {
      toast.success(t('toast.page_updated'));
      await refresh();
    },
    onError: (error: unknown) => toast.error(toApiError(error).message),
  });

  return (
    <SettingsCard title={t('settings.branding.title')} description={t('settings.branding.description')}>
      <StatusPageLogoField
        active={editable}
        page={page}
        logoUrl={logoUrl}
        logoFile={logoFile}
        busy={mutation.isPending}
        onLogoUrlChange={setLogoUrl}
        onLogoFileChange={setLogoFile}
      />
      <FormField label={t('fields.brand_color')} required>
        <div className="flex items-center gap-3">
          <span
            aria-hidden
            className="h-9 w-9 shrink-0 rounded-md border border-bd-1"
            style={{ backgroundColor: /^#[0-9a-fA-F]{6}$/.test(brandColor) ? brandColor : 'transparent' }}
          />
          <FormInput disabled={!editable} value={brandColor} maxLength={7} onChange={(event) => setBrandColor(event.currentTarget.value)} />
        </div>
      </FormField>
      <SettingsFooter>
        <ChromeButton
          variant="primary"
          disabled={!editable || !/^#[0-9a-fA-F]{6}$/.test(brandColor) || mutation.isPending}
          onClick={() => mutation.mutate()}
        >
          {t('actions.save')}
        </ChromeButton>
      </SettingsFooter>
    </SettingsCard>
  );
}
