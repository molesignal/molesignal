import type { FrameProps } from '@/investigation/frame';

import { FramePlaceholder } from './_placeholder';

export function SqlFrame(props: FrameProps) {
  return <FramePlaceholder {...props} label="SQL" />;
}
