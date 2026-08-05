declare module 'wcag-contrast' {
  export function hex(a: string, b: string): number;
  export function rgb(a: readonly number[], b: readonly number[]): number;
  export function luminance(a: number, b: number): number;
  export function score(contrast: number): 'AAA' | 'AA' | 'AA Large' | 'Fail';
}
