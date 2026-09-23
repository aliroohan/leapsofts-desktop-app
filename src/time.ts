import type { Shift, ShiftBreak } from "./types";

const getBreakSeconds = (breaks: ShiftBreak[], untilMs: number): number =>
  breaks.reduce((sum, b) => {
    const start = new Date(b.startTime).getTime();
    if (Number.isNaN(start)) return sum;
    const end = b.endTime ? new Date(b.endTime).getTime() : untilMs;
    if (Number.isNaN(end)) return sum;
    return sum + Math.max(0, Math.floor((end - start) / 1000));
  }, 0);

export function getWorkedSeconds(shift: Shift | null, untilMs: number = Date.now()): number {
  if (!shift?.checkInTime || shift.status !== "checked_in") return 0;
  const checkInMs = new Date(shift.checkInTime).getTime();
  if (Number.isNaN(checkInMs)) return 0;
  const wallSeconds = Math.floor((untilMs - checkInMs) / 1000);
  const breakSeconds = getBreakSeconds(shift.breaks ?? [], untilMs);
  return Math.max(0, wallSeconds - breakSeconds);
}

export function formatDuration(totalSeconds: number): string {
  const h = Math.floor(totalSeconds / 3600);
  const m = Math.floor((totalSeconds % 3600) / 60);
  const s = totalSeconds % 60;
  return `${String(h).padStart(2, "0")}:${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
}

export function openBreak(shift: Shift | null): ShiftBreak | null {
  const breaks = shift?.breaks;
  if (!breaks?.length) return null;
  const last = breaks[breaks.length - 1];
  return last.endTime ? null : last;
}
