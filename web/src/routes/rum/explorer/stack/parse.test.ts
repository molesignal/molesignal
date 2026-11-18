import { describe, expect, it } from 'vitest';

import { stackLines, type StackPart } from './parse';

/** A line as `kind:text` pieces. */
const read = (parts: StackPart[]) => parts.map(({ kind, text }) => `${kind}:${text}`);

describe('stackLines', () => {
  it('tells the error, the functions and the places of a V8 stack apart', () => {
    const [header, first, second] = stackLines(
      [
        'PaymentTimeout: Payment authorization exceeded the expected response time',
        '    at submitOrder (https://shop.example.test/assets/checkout.js:184:16)',
        '    at https://shop.example.test/assets/api-client.js:72:9',
      ].join('\n'),
    );

    expect(read(header!)).toEqual([
      'type:PaymentTimeout',
      'mark:: ',
      'message:Payment authorization exceeded the expected response time',
    ]);
    expect(read(first!)).toEqual([
      'text:    ',
      'mark:at ',
      'function:submitOrder',
      'mark: (',
      'path:https://shop.example.test/assets/',
      'file:checkout.js',
      'position::184:16',
      'mark:)',
    ]);
    expect(read(second!)).toEqual([
      'text:    ',
      'mark:at ',
      'path:https://shop.example.test/assets/',
      'file:api-client.js',
      'position::72:9',
    ]);
  });

  it('reads the frames Firefox and Safari write, and those of the JVM', () => {
    const [firefox, anonymous, jvm] = stackLines(
      ['submitOrder@https://shop.example.test/checkout.js:184:16', '@https://shop.example.test/app.js:3:1', '\tat com.shop.Cart.add(Cart.java:42)'].join('\n'),
    );

    expect(read(firefox!).slice(0, 3)).toEqual(['function:submitOrder', 'mark:@', 'path:https://shop.example.test/']);
    expect(read(anonymous!)[0]).toBe('mark:@');
    expect(read(jvm!)).toContain('function:com.shop.Cart.add');
    expect(read(jvm!)).toContain('file:Cart.java');
    expect(read(jvm!)).toContain('position::42');
  });

  it('leaves what it does not know as text, and every line as it came', () => {
    const stack = [
      'Uncaught TypeError: x is undefined',
      '    at new Cart (cart.js:1:2)',
      '    at async Promise.all (index 0)',
      'something else entirely',
      '',
    ].join('\n');
    const lines = stackLines(stack);

    expect(read(lines[0]!).slice(0, 1)).toEqual(['type:Uncaught TypeError']);
    expect(read(lines[3]!)).toEqual(['text:something else entirely']);
    expect(lines.map((parts) => parts.map((piece) => piece.text).join('')).join('\n')).toBe(stack);
  });
});
