import { describe, expect, it } from 'vitest';
import { formatSpeed, formatTimeLeft } from './format';
import { advance, secondsLeft, startTransfer } from './transfer';

const MB = 1024 * 1024;

describe('transfer speed', () => {
  it('is unknown before the first report, then follows the rate', () => {
    let t = startTransfer(100 * MB, 0);
    expect(t.speed).toBeNull();
    expect(secondsLeft(t)).toBeNull();

    t = advance(t, 10 * MB, 100 * MB, 1000);
    expect(t.speed).toBe(10 * MB);
    expect(secondsLeft(t)).toBe(9);
  });

  it('moves toward a new rate instead of jumping to it', () => {
    let t = advance(startTransfer(null, 0), 10 * MB, null, 1000);
    // One second at 20 MB/s after nine seconds at 10.
    t = advance(t, 30 * MB, null, 2000);
    expect(t.speed!).toBeGreaterThan(10 * MB);
    expect(t.speed!).toBeLessThan(20 * MB);
  });

  it('reports with no time between them change nothing', () => {
    const t = advance(startTransfer(null, 0), 5 * MB, null, 1000);
    expect(advance(t, 6 * MB, null, 1000).speed).toBe(t.speed);
  });

  it('has no time left without a total', () => {
    expect(secondsLeft(advance(startTransfer(null, 0), 5 * MB, null, 1000))).toBeNull();
  });
});

describe('formatting', () => {
  it('reads like people talk', () => {
    expect(formatSpeed(8.2 * MB)).toBe('8.2 MB/s');
    expect(formatTimeLeft(4)).toBe('a few seconds left');
    expect(formatTimeLeft(47)).toBe('about 45 s left');
    expect(formatTimeLeft(170)).toBe('about 3 min left');
    expect(formatTimeLeft(58)).toBe('about 1 min left');
    expect(formatTimeLeft(3600)).toBe('about 1 h left');
    expect(formatTimeLeft(3900)).toBe('about 1 h 5 min left');
  });
});
