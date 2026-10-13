import { useTranslation } from 'react-i18next';

import type { InspectionNotifyRoute } from '@/api/logInspections';
import type { NotifyTargetType } from '@/api/notify';
import { FieldArray, FormInput, FormSelect } from '@/shell/FormDrawer';

import { MAX_ROUTES, type ChannelConnector } from './model';

interface RoutesFieldProps {
  routes: InspectionNotifyRoute[];
  /**
   * The connectors a channel can be on: those enabled, and those a channel of
   * this inspection is already on (see `channelConnectors`).
   */
  connectors: readonly ChannelConnector[];
  onChange: (routes: InspectionNotifyRoute[]) => void;
}

/** A group where the connector can post to one, an address otherwise. */
function targetTypeFor(connector: ChannelConnector | undefined): NotifyTargetType {
  return connector?.group === false ? 'fixed_address' : 'fixed_group';
}

/**
 * Where a notification goes: a connector, and the group or address on it. A
 * person is not a channel here: what reaches people goes through the notify
 * policies and their own endpoints.
 */
export function RoutesField({ routes, connectors, onChange }: RoutesFieldProps) {
  const { t } = useTranslation('logs');
  const { t: tn } = useTranslation('notify');
  const first = connectors[0];

  if (!first) return null;

  return (
    <FieldArray<InspectionNotifyRoute>
      items={routes}
      onChange={(next) => onChange(next.slice(0, MAX_ROUTES))}
      addLabel={t('inspections.editor.add_channel')}
      removeLabel={t('inspections.editor.remove_channel')}
      emptyHint={t('inspections.editor.no_channel')}
      newItem={() => ({
        connector_id: first.id,
        target_type: targetTypeFor(first),
        target: '',
      })}
      renderItem={(route, index, setItem) => (
        <div data-inspection-route={index} className="grid grid-cols-[minmax(0,1fr)_132px_minmax(0,1fr)] gap-2">
          <FormSelect
            ariaLabel={t('inspections.editor.channel_connector', { number: index + 1 })}
            value={route.connector_id}
            onChange={(id) =>
              setItem({
                ...route,
                connector_id: id,
                target_type: targetTypeFor(connectors.find((connector) => connector.id === id)),
              })
            }
            options={connectors.map((connector) => ({
              value: connector.id,
              // A connector that no longer delivers says so where it is chosen.
              label:
                connector.standing === 'on'
                  ? connector.name
                  : t(`inspections.editor.connector_${connector.standing}`, { name: connector.name }),
            }))}
          />
          <FormSelect
            ariaLabel={t('inspections.editor.channel_kind', { number: index + 1 })}
            value={route.target_type}
            onChange={(type) => setItem({ ...route, target_type: type as NotifyTargetType })}
            options={(['fixed_group', 'fixed_address'] as const).map((type) => ({
              value: type,
              label: tn(`target_types.${type}`),
            }))}
          />
          <FormInput
            aria-label={t('inspections.editor.channel_target', { number: index + 1 })}
            placeholder={t(`inspections.editor.target_placeholder.${route.target_type === 'fixed_address' ? 'address' : 'group'}`)}
            value={route.target}
            onChange={(event) => setItem({ ...route, target: event.target.value })}
          />
        </div>
      )}
    />
  );
}
