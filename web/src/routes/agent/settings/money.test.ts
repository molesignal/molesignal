import { describe, expect, it } from 'vitest';

import { currencyLabel, currencySymbol, formatCosts, formatMoney, formatPrice } from './money';

describe('money', () => {
  it('writes an amount in its currency, and says when it is below the smallest step', () => {
    expect(formatMoney(1.2, 'USD')).toContain('1.20');
    expect(formatMoney(0.004, 'USD').startsWith('< ')).toBe(true);
    expect(formatMoney(0.004, 'USD')).toContain('0.01');
    expect(formatMoney(0, 'USD').startsWith('<')).toBe(false);
    // Yen and won have no smaller unit: below one of them is below one.
    expect(formatMoney(0.4, 'JPY').startsWith('< ')).toBe(true);
    expect(formatMoney(0.4, 'JPY')).toContain('1');
  });

  it('keeps the decimals a unit price was given with', () => {
    expect(formatPrice(0.075, 'USD')).toContain('0.075');
    expect(formatPrice(2, 'CNY')).not.toContain('.00');
  });

  it('writes costs one currency at a time, and a floor with ≥', () => {
    const line = formatCosts({ USD: 1.2, CNY: 8.5 });
    expect(line).toContain(' · ');
    expect(line.indexOf('8.50')).toBeLessThan(line.indexOf('1.20'));
    expect(formatCosts({ USD: 0.15 }, true).startsWith('≥ ')).toBe(true);
    expect(formatCosts({})).toBe('—');
    expect(formatCosts({}, true)).toBe('—');
  });

  it('names a currency by its code and its name in the reader’s language', () => {
    expect(currencyLabel('CNY', 'zh-CN')).toBe('CNY · 人民币');
    expect(currencyLabel('EUR', 'en-US')).toBe('EUR · Euro');
    expect(currencyLabel('CNY', 'zh-CN', '元')).toBe('CNY · 元');
    expect(currencySymbol('EUR')).toBe('€');
  });

  it('writes amounts with the symbol the organization set', () => {
    const symbolOf = (code: string) => (code === 'CNY' ? 'RMB ' : null);
    expect(formatMoney(8.5, 'CNY', symbolOf)).toBe('RMB 8.50');
    expect(formatPrice(0.5, 'CNY', symbolOf)).toBe('RMB 0.5');
    expect(currencySymbol('CNY', symbolOf)).toBe('RMB ');
    expect(formatMoney(1.2, 'USD', symbolOf)).toContain('1.20');
  });
});
