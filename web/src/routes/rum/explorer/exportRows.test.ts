import { describe, expect, it } from 'vitest';

import { rumRowsToCsv } from './exportRows';

describe('the file of RUM rows', () => {
  it('has the time of each row and the columns on screen', () => {
    const csv = rumRowsToCsv(
      [
        { timestamp: 1_760_000_000_000_000, 'view.url_path': '/a,b', 'view.error.count': 2, context: { tier: 'x' } },
        { timestamp: 1_760_000_001_000_000 },
      ],
      ['view.url_path', 'view.error.count', 'context'],
    );

    const lines = csv.replace('﻿', '').trim().split('\r\n');

    expect(lines[0]).toBe('time,view.url_path,view.error.count,context');
    expect(lines[1]).toBe('2025-10-09T08:53:20.000Z,"/a,b",2,"{""tier"":""x""}"');
    expect(lines[2]).toBe('2025-10-09T08:53:21.000Z,,,');
  });
});
