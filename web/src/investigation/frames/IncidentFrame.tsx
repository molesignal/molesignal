import type { FrameProps } from '@/investigation/frame';

import { FramePlaceholder } from './_placeholder';

export function IncidentFrame(props: FrameProps) {
  return <FramePlaceholder {...props} label="Incident" />;
}
