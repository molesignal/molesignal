import type { FrameProps } from '@/investigation/frame';

import { FramePlaceholder } from './_placeholder';

export function HostFrame(props: FrameProps) {
  return <FramePlaceholder {...props} label="Host" />;
}
