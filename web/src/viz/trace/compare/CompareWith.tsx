import { GitCompare } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';

import { Button } from '@/shell/ui/button';
import { Input } from '@/shell/ui/input';
import { Popover, PopoverContent, PopoverTrigger } from '@/shell/ui/popover';

import { traceCompareHref } from './link';
import type { TraceTree } from '../model/tree';
import { traceWindowAround } from '../pageLink';
import { TraceHeadButton } from '../view/HeadButton';

/**
 * The trace compared with another one, named by its id: the trace's page
 * opens on the two side by side.
 */
export function CompareWith({ tree }: { tree: TraceTree }) {
  const { t } = useTranslation('traces');
  const navigate = useNavigate();
  const [open, setOpen] = React.useState(false);
  const [other, setOther] = React.useState('');
  const inputId = React.useId();
  const wanted = other.trim();
  const same = wanted === tree.traceId;

  const compare = (event: React.FormEvent) => {
    event.preventDefault();
    if (!wanted || same) return;
    setOpen(false);
    navigate(traceCompareHref({ traceId: tree.traceId, window: traceWindowAround(tree.startNs, tree.endNs) }, { traceId: wanted }));
  };

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <TraceHeadButton icon={GitCompare} label={t('compare.with.label')} data-testid="trace-compare-with" />
      </PopoverTrigger>
      <PopoverContent align="end" className="w-[320px] max-w-[calc(100vw-32px)]">
        <form onSubmit={compare} className="flex flex-col gap-2">
          <label htmlFor={inputId} className="font-sans text-xs font-strong text-tx-1">
            {t('compare.with.field')}
          </label>
          <Input
            id={inputId}
            value={other}
            onChange={(event) => setOther(event.target.value)}
            placeholder={t('compare.with.placeholder')}
            autoComplete="off"
            spellCheck={false}
            className="h-8 font-sans text-xs tabular-nums"
            data-testid="trace-compare-with-input"
          />
          {same && <p className="font-sans text-xs text-tx-3">{t('compare.with.same')}</p>}
          <div className="flex justify-end">
            <Button type="submit" size="sm" disabled={!wanted || same} data-testid="trace-compare-with-submit">
              {t('compare.with.submit')}
            </Button>
          </div>
        </form>
      </PopoverContent>
    </Popover>
  );
}
