import type { ServiceAddress } from './useService';
import { ErrorList } from '../errors/Errors';

/** The service's error groups; one opens in the page's error drawer. */
export function ServiceErrors({ address }: { address: ServiceAddress }) {
  return (
    <ErrorList
      service={{
        namespace: address.namespace,
        name: address.name,
        version: address.scope.version,
      }}
    />
  );
}
