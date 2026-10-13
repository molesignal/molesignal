import { describe, expect, it } from 'vitest';

import { vitalGrade, vitalScale } from './vitals';

describe('vitalGrade', () => {
  it('grades a value against the published thresholds', () => {
    expect(vitalGrade('lcp', 2_500)).toBe('good');
    expect(vitalGrade('lcp', 2_501)).toBe('needs_improvement');
    expect(vitalGrade('lcp', 4_001)).toBe('poor');
    expect(vitalGrade('inp', 200)).toBe('good');
    expect(vitalGrade('inp', 500.5)).toBe('poor');
    expect(vitalGrade('cls', 0.1)).toBe('good');
    expect(vitalGrade('cls', 0.2)).toBe('needs_improvement');
  });

  it('has nothing to say without a measurement', () => {
    expect(vitalGrade('lcp', 0)).toBe('unknown');
    expect(vitalGrade('cls', Number.NaN)).toBe('unknown');
  });
});

describe('vitalScale', () => {
  it('divides the scale at the thresholds', () => {
    const { bands } = vitalScale('lcp', 2_300);

    expect(bands.good).toBeCloseTo(41.67, 2);
    expect(bands.needsImprovement).toBeCloseTo(25, 2);
    expect(bands.poor).toBeCloseTo(33.33, 2);
    expect(bands.good + bands.needsImprovement + bands.poor).toBeCloseTo(100, 6);
  });

  it('places the value along it, in the band its grade names', () => {
    const good = vitalScale('inp', 150);
    expect(good.position).toBeCloseTo(20, 6);
    expect(good.position!).toBeLessThan(good.bands.good);

    const needs = vitalScale('inp', 350);
    expect(needs.position!).toBeGreaterThan(needs.bands.good);
    expect(needs.position!).toBeLessThan(needs.bands.good + needs.bands.needsImprovement);
  });

  it('pins a value beyond the scale to its end, and leaves no mark without a value', () => {
    expect(vitalScale('cls', 4).position).toBe(100);
    expect(vitalScale('cls', 0).position).toBeNull();
  });
});
