import { useQuery } from '@tanstack/react-query';
import { CalendarClock } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import * as inspectionsApi from '@/api/logInspections';
import { useProductAccess } from '@/product/access';
import { ChromeButton } from '@/shell/chrome';

import { canManageInspections, canReadInspections } from './access';
import { INSPECTIONS_KEY, InspectionEditor, type InspectionSeed } from './Editor';
import { DEFAULT_LIMITS } from './model';

/**
 * The way from the inspection of one query to the inspections that run on a
 * schedule. It is there only for those who may see them: a way to a page that
 * turns one away is not a way.
 */
export function InspectionsLink({ className }: { className?: string }) {
  const { t } = useTranslation('logs');
  const access = useProductAccess();

  if (!canReadInspections(access)) return null;

  return (
    <Link
      to="/logs/inspections"
      data-log-inspections-entry
      className={
        className ??
        'inline-flex h-8 shrink-0 items-center gap-1.5 rounded-md px-2.5 font-sans text-xs font-strong text-[var(--blue-ink)] hover:bg-bg-2 focus-visible:bg-bg-3 focus-visible:outline-none'
      }
    >
      <CalendarClock className="h-3.5 w-3.5" aria-hidden="true" />
      {t('inspections.entry')}
    </Link>
  );
}

/**
 * Saves the query the reader is looking at as an inspection that runs on a
 * schedule: the form opens with the stream, the query, the requirements and
 * the model already in it. Shown to those who may save one.
 */
export function ScheduleInspectionButton({ seed }: { seed: InspectionSeed }) {
  const { t } = useTranslation('logs');
  const access = useProductAccess();
  const manageable = canManageInspections(access);
  const [open, setOpen] = React.useState(false);
  // What the form opens with is what was on the page when the button was pressed.
  const [opened, setOpened] = React.useState<InspectionSeed>(seed);
  // The limits are asked for when the form is first wanted, and kept. The form is drafted
  // within them, so it waits for the answer; without one it takes what an instance has
  // unless told otherwise, and the backend still has the last word.
  const limits = useQuery({
    queryKey: [...INSPECTIONS_KEY, 'limits'],
    queryFn: () => inspectionsApi.limits(),
    enabled: manageable && open,
    staleTime: 5 * 60_000,
    retry: false,
  });

  if (!manageable) return null;

  return (
    <>
      <ChromeButton
        data-log-inspection-schedule
        onClick={() => {
          setOpened(seed);
          setOpen(true);
        }}
      >
        <CalendarClock className="h-3.5 w-3.5" aria-hidden="true" />
        {t('inspections.save_as')}
      </ChromeButton>
      <InspectionEditor
        open={open && !limits.isPending}
        onOpenChange={setOpen}
        inspection={null}
        seed={opened}
        limits={limits.data ?? DEFAULT_LIMITS}
      />
    </>
  );
}
