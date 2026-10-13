import { useTranslation } from 'react-i18next';

import type { FunctionResp } from '@/api/functions';
import { writeClipboardText } from '@/lib/clipboard';
import type { ActionAccess } from '@/product/actionAccess';
import { ChromeButton } from '@/shell/chrome';
import { CopyIconButton } from '@/shell/CopyIconButton';
import { FormDrawer } from '@/shell/FormDrawer';
import { toast } from '@/shell/ui/sonner';

import { languageName } from './model';

/**
 * A preset, read: what it does and its source. A preset cannot be changed, so what the drawer
 * offers is the way on from it: a function of the workspace's own that starts as its copy.
 */
export function PresetDrawer({
  preset,
  createAccess,
  onClose,
  onStartFrom,
}: {
  preset: FunctionResp | null;
  createAccess: ActionAccess;
  onClose: () => void;
  onStartFrom: (preset: FunctionResp) => void;
}) {
  const { t } = useTranslation('functions');

  return (
    <FormDrawer
      open={preset !== null}
      onOpenChange={(open) => !open && onClose()}
      title={preset?.name ?? ''}
      subtitle={preset ? t('list.preset_kind', { language: languageName(preset.language) }) : undefined}
      footer={
        <div className="flex items-center justify-end gap-2">
          <CopyIconButton
            label={t('list.preset_copy')}
            onClick={() => {
              if (!preset) return;
              void writeClipboardText(preset.source);
              toast.success(t('list.preset_copied'));
            }}
          />
          <ChromeButton
            variant="primary"
            data-function-start-from=""
            disabled={createAccess.disabled}
            disabledReason={createAccess.reason}
            onClick={() => preset && onStartFrom(preset)}
          >
            {t('list.start_from')}
          </ChromeButton>
        </div>
      }
    >
      <div data-function-preset={preset?.name}>
        {preset?.description ? (
          <p className="m-0 mb-3 font-sans text-sm leading-relaxed text-tx-1">{preset.description}</p>
        ) : null}
        {/* Source is read in the face it is written in, as in the editor. */}
        <pre className="m-0 max-h-[60vh] overflow-auto rounded-md bg-[var(--control-surface)] p-3 font-code text-xs leading-relaxed text-tx-1">
          {preset?.source}
        </pre>
      </div>
    </FormDrawer>
  );
}
