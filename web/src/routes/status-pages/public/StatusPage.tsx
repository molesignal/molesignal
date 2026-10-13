import type { ReactNode } from 'react';

import type { PublicStatusPageIncident } from '@/api/statusPages';

import { PublicComponentList } from './ComponentList';
import { type PublicStatusContext, PublicStatusFrame } from './Frame';
import { ActiveNotice, UpcomingMaintenance } from './Notices';
import { PastIncidents } from './PastIncidents';
import {
  type PublicStatusSource,
  ongoingPublicIncidents,
  publicIncidentPath,
  upcomingMaintenance,
} from './routing';
import { SectionHeading } from './Section';
import { PublicStatusVerdict } from './Verdict';

/**
 * A public status page: the answer to "is it working", read from the top.
 *
 * First the verdict, and with it whatever is going on now, because that is
 * its reason. Then what is planned, then every component with how it has
 * held up, and last what happened lately and is over.
 */
export function PublicStatusPage({
  source = 'slug',
  unmatchedDomain,
}: {
  source?: PublicStatusSource;
  unmatchedDomain?: ReactNode;
}) {
  return (
    <PublicStatusFrame source={source} view="status" unmatchedDomain={unmatchedDomain}>
      {(context) => <StatusView {...context} />}
    </PublicStatusFrame>
  );
}

function StatusView({ snapshot, page, source, slug, language, copy, paths, languageQuery }: PublicStatusContext) {
  const hrefOf = (incident: PublicStatusPageIncident) =>
    `${publicIncidentPath(source, slug, incident.id)}${languageQuery}`;
  const ongoing = ongoingPublicIncidents(snapshot);

  return (
    <>
      <PublicStatusVerdict
        status={snapshot.overall_status}
        pageName={page.name}
        updatedAt={snapshot.updated_at ?? snapshot.generated_at}
        timezone={page.timezone}
        language={language}
        copy={copy}
      >
        {ongoing.map((incident) => (
          <ActiveNotice
            key={incident.id}
            incident={incident}
            snapshot={snapshot}
            href={hrefOf(incident)}
            timezone={page.timezone}
            language={language}
            copy={copy}
          />
        ))}
      </PublicStatusVerdict>

      <UpcomingMaintenance
        items={upcomingMaintenance(snapshot)}
        snapshot={snapshot}
        hrefOf={hrefOf}
        timezone={page.timezone}
        language={language}
        copy={copy}
      />

      <section className="mt-12 sm:mt-14" aria-labelledby="system-status-heading">
        <SectionHeading
          id="system-status-heading"
          title={copy('public.system_status')}
          meta={copy('public.uptime.period')}
        />
        <PublicComponentList snapshot={snapshot} timezone={page.timezone} language={language} copy={copy} />
      </section>

      <PastIncidents
        snapshot={snapshot}
        hrefOf={hrefOf}
        historyHref={paths.history}
        timezone={page.timezone}
        language={language}
        copy={copy}
      />
    </>
  );
}
