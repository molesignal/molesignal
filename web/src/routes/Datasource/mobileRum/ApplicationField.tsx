import { useQuery } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import * as rumApi from '@/api/rum';
import { FormSelect } from '@/shell/FormDrawer';

/**
 * The RUM application a token is bound to, chosen among the applications of the organization: a
 * token is issued per application, and an event for any other one is refused. The one a link
 * names is taken, and where there is one there is nothing to choose and it is taken; where there
 * is none, the way to make one is shown.
 */
export function ApplicationField({
  value,
  preferred,
  onChange,
}: {
  /** The application chosen: always one the organization has, or nothing yet. */
  value: string;
  /** The application a link to the guide names. */
  preferred: string;
  onChange: (applicationId: string) => void;
}) {
  const { t } = useTranslation('onboarding');
  const query = useQuery({
    queryKey: ['rum', 'applications'],
    queryFn: () => rumApi.listApplications(),
  });
  const applications = React.useMemo(
    () => [...(query.data ?? [])].sort((a, b) => a.name.localeCompare(b.name)),
    [query.data],
  );
  const known = applications.some((application) => application.id === value);

  React.useEffect(() => {
    if (!query.isSuccess) return;

    // What was chosen can be deleted from another page while this one is open.
    if (value !== '' && !known) {
      onChange('');
      return;
    }

    if (value !== '') return;

    const [only] = applications;
    const chosen =
      applications.find((application) => application.id === preferred) ??
      (applications.length === 1 ? only : undefined);
    if (chosen) onChange(chosen.id);
  }, [applications, known, onChange, preferred, query.isSuccess, value]);

  if (query.isError) {
    return (
      <p role="alert" className="m-0 font-sans text-xs leading-relaxed text-red-soft">
        {t('datasource_page.rum_application_load_failed')}
      </p>
    );
  }

  if (query.isSuccess && applications.length === 0) {
    return (
      <p className="m-0 font-sans text-xs leading-relaxed text-tx-3">
        {t('datasource_page.rum_application_none')}{' '}
        <Link
          to="/rum/applications?create=1"
          className="rounded font-strong text-[var(--blue-ink)] hover:underline focus-visible:underline"
        >
          {t('datasource_page.rum_application_create')}
        </Link>
      </p>
    );
  }

  return (
    <>
      {/* The select is drawn once its choices are known: its value and its options arrive together. */}
      {query.isSuccess ? (
        <FormSelect
          value={value}
          onChange={onChange}
          ariaLabel={t('datasource_page.rum_application_id')}
          placeholder={t('datasource_page.rum_application_placeholder')}
          options={applications.map((application) => ({
            value: application.id,
            label: `${application.name} · ${application.id}`,
          }))}
        />
      ) : (
        <FormSelect
          value=""
          onChange={onChange}
          ariaLabel={t('datasource_page.rum_application_id')}
          placeholder={t('datasource_page.rum_application_loading')}
          options={[]}
          disabled
        />
      )}
      <p className="m-0 mt-[6px] font-sans text-xs leading-relaxed text-tx-3">
        {t('datasource_page.rum_application_hint')}
      </p>
    </>
  );
}
