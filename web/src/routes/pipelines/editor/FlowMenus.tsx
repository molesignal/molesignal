import { Plug, Plus, Send, TableProperties } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { ChromeButton } from '@/shell/chrome';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/shell/ui/dropdown-menu';

import type { FlowEditing } from './Flow';
import { OUTSIDE_STEPS, processorGroups } from './StepFields';
import { STEP_ICONS } from './steps';
import type { CountingKind } from '../counting/model';
import { MAX_STREAMS, type AlertStage, type PipelineMode, type StepType } from '../PipelineGraph/model';

/** What a new destination can be: another stream that takes every event, or a connector. */
export function AddDestinationMenu({ editing, streamsFull }: { editing: FlowEditing; streamsFull: boolean }) {
  const { t } = useTranslation('pipelines');

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <ChromeButton
          size="sm"
          variant="ghost"
          className="justify-self-start text-tx-2"
          disabled={editing.disabled}
          disabledReason={editing.disabledReason}
        >
          <Plus aria-hidden className="h-3.5 w-3.5" />
          {t('flow.add_destination')}
        </ChromeButton>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start" className="min-w-[208px]">
        <DropdownMenuItem
          disabled={streamsFull}
          disabledReason={streamsFull ? t('flow.at_most', { max: MAX_STREAMS }) : undefined}
          onSelect={() => editing.onAddDestination('stream')}
        >
          <Send aria-hidden className="h-3.5 w-3.5 text-tx-2" />
          {t('flow.destination_stream')}
        </DropdownMenuItem>
        <DropdownMenuItem onSelect={() => editing.onAddDestination('connector')}>
          <Plug aria-hidden className="h-3.5 w-3.5 text-tx-2" />
          {t('flow.destination_connector')}
        </DropdownMenuItem>
        <DropdownMenuItem onSelect={() => editing.onAddDestination('table')}>
          <TableProperties aria-hidden className="h-3.5 w-3.5 text-tx-2" />
          {t('flow.destination_table')}
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

/**
 * What a new step can be: a ready-made processor, a step that keeps count, a
 * route, or VRL written by hand; for an alert pipeline an AI summary instead
 * of a route, and no route where events stay in the stream they arrive in
 * (`routes`).
 */
export function AddStepMenu({
  editing,
  mode,
  stage = 'open',
  routes = true,
  counting = [],
}: {
  editing: FlowEditing;
  mode: PipelineMode | undefined;
  /** When an alert pipeline runs: a step goes outside only as an incident opens. */
  stage?: AlertStage;
  routes?: boolean;
  /** The steps that keep count this pipeline can take. */
  counting?: readonly CountingKind[];
}) {
  const { t } = useTranslation('pipelines');
  const item = (type: StepType) => {
    const Icon = STEP_ICONS[type];

    return (
      <DropdownMenuItem key={type} onSelect={() => editing.onAddStep(type)}>
        <Icon aria-hidden className="h-3.5 w-3.5 text-tx-2" />
        {t(`steps.types.${type}`)}
      </DropdownMenuItem>
    );
  };

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <ChromeButton
          size="sm"
          variant="ghost"
          className="justify-self-start text-tx-2"
          disabled={editing.disabled}
          disabledReason={editing.disabledReason}
        >
          <Plus aria-hidden className="h-3.5 w-3.5" />
          {t('flow.add_step')}
        </ChromeButton>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start" className="min-w-[208px]">
        {processorGroups(mode).map(({ group, ops }) => (
          <React.Fragment key={group}>
            <DropdownMenuLabel>{t(`steps.groups.${group}`)}</DropdownMenuLabel>
            {ops.map(item)}
          </React.Fragment>
        ))}
        {counting.length > 0 && (
          <>
            <DropdownMenuLabel>{t('steps.groups.counting')}</DropdownMenuLabel>
            {counting.map(item)}
          </>
        )}
        <DropdownMenuSeparator />
        {mode === 'alert' ? stage === 'open' && OUTSIDE_STEPS.map(item) : routes && item('route')}
        {item('vrl')}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
