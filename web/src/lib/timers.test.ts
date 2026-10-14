import { describe, expect, it } from 'vitest';
import { clock, extend, ringing, type Timer } from './timers';

const t = (endsAt: number): Timer => ({ id: 'a', label: 'x', path: 'p', endsAt });

describe('timers', () => {
  it('formats the clock', () => {
    expect(clock(245_000)).toBe('4:05');
    expect(clock(244_100)).toBe('4:05'); // rounds up while counting down
    expect(clock(3_720_000)).toBe('1:02:00');
    expect(clock(0)).toBe('0:00');
    expect(clock(-12_400)).toBe('+0:12');
  });

  it('rings once over', () => {
    expect(ringing(t(1000), 999)).toBe(false);
    expect(ringing(t(1000), 1000)).toBe(true);
  });

  it('extends from now once it has gone off', () => {
    expect(extend(t(10_000), 60_000, 5_000).endsAt).toBe(70_000);
    expect(extend(t(10_000), 60_000, 20_000).endsAt).toBe(80_000);
  });
});
