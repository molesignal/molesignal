import { ImageIcon, Trash2, Upload } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as statusPagesApi from '@/api/statusPages';
import type { StatusPage } from '@/api/statusPages';
import { ChromeButton, uiLabelClass } from '@/shell/chrome';
import { FormField, FormInput } from '@/shell/FormDrawer';

const MAX_LOGO_BYTES = 2 * 1024 * 1024;
const LOGO_TYPES = new Set(['image/png', 'image/jpeg', 'image/webp']);

function isManagedLogoUrl(url: string | null | undefined): boolean {
  return Boolean(
    url?.match(/^\/api\/v1\/public\/status-pages\/[a-z0-9]+(?:-[a-z0-9]+)*\/logo\/[A-Za-z0-9_-]+\.(?:png|jpe?g|webp)$/),
  );
}

function useObjectUrl(value: Blob | null): string | null {
  const [url, setUrl] = React.useState<string | null>(null);

  React.useEffect(() => {
    if (!value) {
      setUrl(null);
      return;
    }
    const next = URL.createObjectURL(value);
    setUrl(next);
    return () => URL.revokeObjectURL(next);
  }, [value]);

  return url;
}

export function StatusPageLogoField({
  active,
  page,
  logoUrl,
  logoFile,
  busy,
  onLogoUrlChange,
  onLogoFileChange,
}: {
  active: boolean;
  page: StatusPage | null;
  logoUrl: string;
  logoFile: File | null;
  busy: boolean;
  onLogoUrlChange: (url: string) => void;
  onLogoFileChange: (file: File | null) => void;
}) {
  const { t } = useTranslation('status-pages');
  const fileInputRef = React.useRef<HTMLInputElement>(null);
  const [storedLogo, setStoredLogo] = React.useState<Blob | null>(null);
  const [storedLogoFailed, setStoredLogoFailed] = React.useState(false);
  const [previewFailed, setPreviewFailed] = React.useState(false);
  const [fileError, setFileError] = React.useState<string | null>(null);
  const managedPageId = isManagedLogoUrl(page?.logo_url) ? page?.id : undefined;
  const managedPageLogoUrl = managedPageId ? page?.logo_url : undefined;

  React.useEffect(() => {
    setStoredLogo(null);
    setStoredLogoFailed(false);
    if (!active || !managedPageId || !managedPageLogoUrl) return;
    let current = true;
    void statusPagesApi
      .getLogoBlob(managedPageId)
      .then((blob) => {
        if (current) setStoredLogo(blob);
      })
      .catch(() => {
        if (current) setStoredLogoFailed(true);
      });
    return () => {
      current = false;
    };
  }, [active, managedPageId, managedPageLogoUrl]);

  const selectedLogoUrl = useObjectUrl(logoFile);
  const storedLogoUrl = useObjectUrl(storedLogo);
  const usesStoredPreview = Boolean(
    logoUrl && managedPageLogoUrl && logoUrl === managedPageLogoUrl,
  );
  const previewUrl = selectedLogoUrl ?? (usesStoredPreview ? storedLogoUrl : logoUrl || null);

  React.useEffect(() => setPreviewFailed(false), [previewUrl]);

  const selectFile = (event: React.ChangeEvent<HTMLInputElement>) => {
    const file = event.currentTarget.files?.[0];
    event.currentTarget.value = '';
    if (!file) return;
    if (!LOGO_TYPES.has(file.type)) {
      setFileError(t('forms.page.logo_type_error'));
      return;
    }
    if (file.size === 0) {
      setFileError(t('forms.page.logo_empty_error'));
      return;
    }
    if (file.size > MAX_LOGO_BYTES) {
      setFileError(t('forms.page.logo_size_error'));
      return;
    }
    setFileError(null);
    onLogoUrlChange('');
    onLogoFileChange(file);
  };

  const hasLogo = Boolean(logoFile || logoUrl);
  const previewUnavailable = previewFailed || (usesStoredPreview && storedLogoFailed);

  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-col gap-1.5">
        <span className={uiLabelClass}>{t('fields.logo')}</span>
        <div className="flex flex-col gap-3 rounded-lg border border-bd-0 bg-bg-2 p-3 sm:flex-row sm:items-center">
          <div
            role="img"
            aria-label={t('forms.page.logo_preview_label')}
            aria-busy={usesStoredPreview && !storedLogoUrl && !storedLogoFailed}
            className="grid h-20 w-20 shrink-0 place-items-center overflow-hidden rounded-lg border border-bd-1 bg-bg-1 text-tx-3"
          >
            {previewUrl && !previewFailed ? (
              <img
                src={previewUrl}
                alt=""
                className="h-full w-full object-contain p-2"
                onError={() => setPreviewFailed(true)}
              />
            ) : (
              <ImageIcon aria-hidden className="h-6 w-6" />
            )}
          </div>
          <div className="min-w-0 flex-1 space-y-2">
            <input
              ref={fileInputRef}
              type="file"
              accept="image/png,image/jpeg,image/webp"
              disabled={busy}
              aria-label={t('actions.upload_logo')}
              className="hidden"
              onChange={selectFile}
            />
            <div className="flex flex-wrap gap-2">
              <ChromeButton
                type="button"
                disabled={busy}
                className="h-11 sm:h-9"
                onClick={() => fileInputRef.current?.click()}
              >
                <Upload aria-hidden className="h-3.5 w-3.5" />
                {t(hasLogo ? 'actions.replace_logo' : 'actions.upload_logo')}
              </ChromeButton>
              {hasLogo && (
                <ChromeButton
                  type="button"
                  disabled={busy}
                  className="h-11 sm:h-9"
                  onClick={() => {
                    setFileError(null);
                    onLogoFileChange(null);
                    onLogoUrlChange('');
                  }}
                >
                  <Trash2 aria-hidden className="h-3.5 w-3.5" />
                  {t('actions.remove_logo')}
                </ChromeButton>
              )}
            </div>
            <p className="text-xs leading-relaxed text-tx-3">
              {t('forms.page.logo_upload_hint')}
            </p>
            {logoFile && (
              <p className="truncate text-xs text-tx-2">
                {t('forms.page.logo_selected', { name: logoFile.name })}
              </p>
            )}
            {(fileError || previewUnavailable) && (
              <p role="alert" className="text-xs leading-relaxed text-red-soft">
                {fileError ?? t('forms.page.logo_preview_error')}
              </p>
            )}
          </div>
        </div>
      </div>

      <FormField label={t('fields.logo_url')} hint={t('forms.page.logo_url_hint')}>
        <FormInput
          value={usesStoredPreview ? '' : logoUrl}
          disabled={busy}
          onChange={(event) => {
            setFileError(null);
            onLogoFileChange(null);
            onLogoUrlChange(event.currentTarget.value);
          }}
          placeholder="https://cdn.example.com/logo.svg"
        />
      </FormField>
    </div>
  );
}
