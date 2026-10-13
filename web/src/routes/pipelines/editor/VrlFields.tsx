import { useQuery } from '@tanstack/react-query';
import { Plus } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as functionsApi from '@/api/functions';
import { ChromeButton } from '@/shell/chrome';
import { CodeEditor } from '@/shell/codeEditor';
import { FormField, FormSelect } from '@/shell/FormDrawer';

/**
 * A step written in VRL. The script is the step: a saved function is a place
 * to start from, copied in when picked, and the editor is always on view.
 */
export function VrlFields({
  name,
  script,
  readOnly = false,
  onChange,
}: {
  name: string;
  script: string;
  readOnly?: boolean;
  onChange: (script: string) => void;
}) {
  const { t } = useTranslation('pipelines');
  const functionsQuery = useQuery({ queryKey: ['functions'], queryFn: () => functionsApi.list() });
  const functions = React.useMemo(
    () => (functionsQuery.data ?? []).filter((item) => item.language === 'vrl'),
    [functionsQuery.data],
  );
  // The list holds the built-in presets too. One whose source is this script is shown as picked.
  const picked = functions.find((item) => item.source === script);

  return (
    <div className="grid gap-[12px]">
      <div className="flex min-w-0 flex-wrap items-end gap-[8px]">
        <FormField label={t('steps.start_from')} className="min-w-[240px] max-w-[420px] flex-1">
          <FormSelect
            value={picked?.id ?? ''}
            onChange={(id) => {
              const chosen = functions.find((item) => item.id === id);
              if (chosen) onChange(chosen.source);
            }}
            options={functions.map((item) => ({ value: item.id, label: item.name }))}
            placeholder={t('graph.vrl_function_placeholder')}
            {...(readOnly ? { disabled: true } : {})}
          />
        </FormField>
        <ChromeButton
          // A function is written on its own page; this pipeline stays open here.
          onClick={() => window.open('/functions/new', '_blank', 'noopener')}
        >
          <Plus aria-hidden className="h-3.5 w-3.5" />
          {t('graph.new_vrl_function')}
        </ChromeButton>
        {picked?.is_builtin && picked.description && (
          <p className="m-0 min-w-0 flex-[1_1_100%] font-sans text-xs leading-relaxed text-tx-3">
            {picked.description}
          </p>
        )}
      </div>
      <CodeEditor
        value={script}
        onChange={onChange}
        readOnly={readOnly}
        language="vrl"
        label={`${t('workspace.code_editor')} · ${name || t('graph.transform')}`}
        ariaLabel={t('graph.vrl_script')}
        minHeight={220}
        maxHeight={460}
        resizable
      />
    </div>
  );
}
