import type { FrameProps } from '@/investigation/frame';

import { FramePlaceholder } from './_placeholder';

export function DashboardPanelFrame(props: FrameProps) {
  return <FramePlaceholder {...props} label="Dashboard panel" />;
}
