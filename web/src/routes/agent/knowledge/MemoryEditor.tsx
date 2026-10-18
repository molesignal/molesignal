import { useMutation, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { MEMORY_KINDS, type Memory, type MemoryKind, updateMemory } from '@/api/agent/memory';
import { toApiError } from '@/lib/http';
import { FormDrawer, FormField, FormSelect, FormSubmitFooter } from '@/shell/FormDrawer';
import { Input } from '@/shell/ui/input';
import { toast } from '@/shell/ui/sonner';

import { MarkdownEditor } from '../markdownEditor/MarkdownEditor';

/** The longest a memory says, as the backend holds it. */
const MAX_BODY_CHARS = 4000;

/** Change one memory: what kind it is, its name, when it applies, and what it says. */
export function MemoryEditor({ memory, onClose }: { memory: Memory; onClose: () => void }) {
  const { t } = useTranslation('agent');
  const queryClient = useQueryClient();
  const [kind, setKind] = React.useState<MemoryKind>(memory.kind);
  const [name, setName] = React.useState(memory.name);
  const [description, setDescription] = React.useState(memory.description);
  const [body, setBody] = React.useState(memory.body);
  const formId = React.useId();

  const save = useMutation({
    mutationFn: () => updateMemory(memory.id, { kind, name, description, body }),
    onSuccess: async () => {
      toast.success(t('memory.saved'));
      await queryClient.invalidateQueries({ queryKey: ['agent', 'memories'] });
      onClose();
    },
    onError: (error: unknown) => toast.error(toApiError(error).message),
  });
  const invalid =
    !name.trim() || !description.trim() || !body.trim() || body.length > MAX_BODY_CHARS;

  return (
    <FormDrawer
      open
      onOpenChange={(open) => !open && onClose()}
      title={t('memory.edit_title')}
      width={560}
      footer={
        <FormSubmitFooter
          formId={formId}
          busy={save.isPending}
          invalid={invalid}
          onCancel={onClose}
          submitLabel={t('memory.save')}
        />
      }
    >
      <form
        id={formId}
        className="flex flex-col gap-4"
        onSubmit={(event) => {
          event.preventDefault();
          if (!invalid) save.mutate();
        }}
      >
        <FormField label={t('memory.kind')}>
          <FormSelect
            value={kind}
            onChange={(value) => setKind(value as MemoryKind)}
            options={MEMORY_KINDS.map((value) => ({ value, label: t(`memory.kinds.${value}`) }))}
          />
        </FormField>
        <FormField label={t('memory.name')}>
          <Input value={name} maxLength={120} onChange={(event) => setName(event.target.value)} />
        </FormField>
        <FormField label={t('memory.applies_when')}>
          <Input
            value={description}
            maxLength={300}
            onChange={(event) => setDescription(event.target.value)}
          />
        </FormField>
        <FormField label={t('memory.body')}>
          {/* Written as it reads: "- " turns into a bullet. */}
          <MarkdownEditor
            value={body}
            onChange={setBody}
            ariaLabel={t('memory.body')}
            className="min-h-[180px] rounded-md bg-[var(--control-surface)] px-3 py-2 transition-colors hover:bg-bg-3 focus-within:bg-bg-3"
          />
        </FormField>
      </form>
    </FormDrawer>
  );
}
