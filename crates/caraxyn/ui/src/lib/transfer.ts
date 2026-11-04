// Speed and time left for a running transfer, from the progress reports the node sends.

export interface Transfer {
  bytes: number;
  total: number | null;
  /** Bytes per second, averaged; `null` until the first report. */
  speed: number | null;
  /** When `bytes` was last updated, in milliseconds. */
  at: number;
  /** `true` direct, `false` only through a relay (slower), `null` while not known. */
  direct: boolean | null;
}

/** Roughly how many seconds the speed averages over: shorter reacts faster, longer jumps less. */
const SMOOTHING_S = 3;

export function startTransfer(total: number | null, now: number): Transfer {
  return { bytes: 0, total, speed: null, at: now, direct: null };
}

/** The transfer after a report of `bytes` so far at `now`, over a connection that is `direct` or not. */
export function advance(
  transfer: Transfer,
  bytes: number,
  total: number | null,
  now: number,
  direct: boolean | null = transfer.direct,
): Transfer {
  const seconds = (now - transfer.at) / 1000;
  if (seconds <= 0) return { ...transfer, bytes, total, direct };
  const instant = Math.max(0, bytes - transfer.bytes) / seconds;
  // Weighted by the time passed, so uneven gaps between reports average out the same.
  const weight = 1 - Math.exp(-seconds / SMOOTHING_S);
  const speed = transfer.speed === null ? instant : transfer.speed + (instant - transfer.speed) * weight;
  return { bytes, total, speed, at: now, direct };
}

/** Seconds left at the current speed, or `null` while that is unknown. */
export function secondsLeft(transfer: Transfer): number | null {
  if (transfer.total === null || !transfer.speed) return null;
  return Math.max(0, (transfer.total - transfer.bytes) / transfer.speed);
}
