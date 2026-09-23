export type BreakSource = "manual" | "idle" | "sleep" | "offline";

export interface ShiftBreak {
  startTime: string;
  endTime: string | null;
  source?: BreakSource;
}

export interface Shift {
  _id: string;
  userId: string;
  date: string;
  checkInTime: string | null;
  checkOutTime: string | null;
  totalMinutes?: number;
  breaks?: ShiftBreak[];
  totalBreakMinutes?: number;
  status: "not_started" | "checked_in" | "checked_out";
  isActive?: boolean;
}

export interface TrackerUser {
  _id: string;
  email: string;
  firstName?: string;
  lastName?: string;
  idleTimeoutMinutes?: number;
  monitorScreenshots?: boolean;
  monitorAppUsage?: boolean;
}

export interface TrackingSummary {
  app: string | null;
  domain: string | null;
  pendingSegments: number;
}

export type LoginResult =
  | { kind: "authenticated"; state: TrackerState }
  | { kind: "requires2FA"; tempToken: string }
  | { kind: "requires2FASetup" };

export interface TrackerState {
  isAuthenticated: boolean;
  user: TrackerUser | null;
  shift: Shift | null;
  idleSeconds: number;
  idleTimeoutMinutes: number;
  isOnline: boolean;
  pendingCount: number;
  lastError: string | null;
  monitoringActive: boolean;
  monitoringError: string | null;
  lastSampleAt: string | null;
  trackingSummary: TrackingSummary | null;
}
