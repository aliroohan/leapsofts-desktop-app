import { useEffect, useState, type JSX } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { LoginResult, TrackerState } from "./types";
import {
  formatClockTime,
  formatDuration,
  formatMinutesAsDuration,
  getWorkedSeconds,
  openBreak,
  openMeeting,
} from "./time";
import { checkAndInstallUpdate, restartApp } from "./updater";

export default function App(): JSX.Element {
  const [state, setState] = useState<TrackerState | null>(null);
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [tempToken, setTempToken] = useState<string | null>(null);
  const [code, setCode] = useState("");
  const [busy, setBusy] = useState(false);
  const [formError, setFormError] = useState<string | null>(null);
  const [now, setNow] = useState(Date.now());
  const [updateBusy, setUpdateBusy] = useState(false);
  const [updateReadyVersion, setUpdateReadyVersion] = useState<string | null>(null);
  const [updateInfo, setUpdateInfo] = useState<string | null>(null);

  useEffect(() => {
    void invoke<TrackerState>("get_state").then(setState);
    const unlisten = listen<TrackerState>("tracker-state", (event) => {
      setState(event.payload);
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, []);

  useEffect(() => {
    const id = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(id);
  }, []);

  // Production launches: install quietly if a release exists. Dev / offline / missing plugin: no crash.
  useEffect(() => {
    if (import.meta.env.DEV) return;
    let cancelled = false;
    void (async () => {
      const result = await checkAndInstallUpdate();
      if (cancelled) return;
      if (result.kind === "ready") {
        setUpdateReadyVersion(result.version);
        setUpdateInfo(`Update ${result.version} is ready. Restart to apply.`);
      }
      // kind "none" / "error": stay quiet on launch
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  const runUpdateCheck = async (opts?: { silent?: boolean }): Promise<void> => {
    setUpdateBusy(true);
    if (!opts?.silent) {
      setUpdateInfo(null);
      setFormError(null);
    }
    try {
      const result = await checkAndInstallUpdate();
      if (result.kind === "ready") {
        setUpdateReadyVersion(result.version);
        setUpdateInfo(`Update ${result.version} is ready. Restart to apply.`);
      } else if (result.kind === "none") {
        if (!opts?.silent) setUpdateInfo("You're up to date.");
      } else if (!opts?.silent) {
        setUpdateInfo(result.message);
      }
    } finally {
      setUpdateBusy(false);
    }
  };

  const run = async (fn: () => Promise<TrackerState>): Promise<boolean> => {
    setBusy(true);
    setFormError(null);
    try {
      setState(await fn());
      return true;
    } catch (err) {
      setFormError(
        typeof err === "string" ? err : err instanceof Error ? err.message : "Request failed",
      );
      return false;
    } finally {
      setBusy(false);
    }
  };

  if (!state) {
    return (
      <div className="app">
        <p className="sub">Loading…</p>
      </div>
    );
  }

  const signIn = async (): Promise<void> => {
    setBusy(true);
    setFormError(null);
    try {
      const result = await invoke<LoginResult>("login", { email, password });
      if (result.kind === "requires2FA") {
        setTempToken(result.tempToken);
        setCode("");
        return;
      }
      if (result.kind === "requires2FASetup") {
        setFormError("Two-factor setup is required. Finish it in the web app, then sign in here.");
        return;
      }
      setTempToken(null);
      setState(result.state);
    } catch (err) {
      setFormError(
        typeof err === "string" ? err : err instanceof Error ? err.message : "Request failed",
      );
    } finally {
      setBusy(false);
    }
  };

  const updateBanner =
    updateReadyVersion || updateInfo ? (
      <div className="card">
        {updateInfo ? <p className="sub">{updateInfo}</p> : null}
        {updateReadyVersion ? (
          <button
            disabled={updateBusy}
            onClick={() => {
              void restartApp().catch(() => {
                setUpdateInfo("Restart failed. Quit and reopen the app to finish updating.");
              });
            }}
          >
            Restart now
          </button>
        ) : null}
      </div>
    ) : null;

  const updateCheckButton = (
    <div className="row">
      <button
        className="secondary"
        disabled={busy || updateBusy}
        onClick={() => void runUpdateCheck()}
      >
        {updateBusy ? "Checking…" : "Check for updates"}
      </button>
    </div>
  );

  if (!state.isAuthenticated && tempToken) {
    return (
      <div className="app">
        <div>
          <h1>Two-factor authentication</h1>
          <p className="sub">
            Enter the 6-digit code from your authenticator, or a backup code.
          </p>
        </div>
        <form
          className="card"
          onSubmit={(e) => {
            e.preventDefault();
            void run(() =>
              invoke<TrackerState>("verify_2fa", { tempToken, code: code.trim() }),
            ).then((ok) => {
              if (ok) setTempToken(null);
            });
          }}
        >
          <label htmlFor="otp">Authenticator or backup code</label>
          <input
            id="otp"
            autoFocus
            inputMode="text"
            autoComplete="one-time-code"
            maxLength={16}
            value={code}
            onChange={(e) =>
              setCode(e.target.value.toUpperCase().replace(/[^A-Z0-9]/g, "").slice(0, 16))
            }
          />
          {formError ? <p className="error">{formError}</p> : null}
          <button type="submit" disabled={busy || code.trim().length < 6}>
            {busy ? "Verifying…" : "Verify"}
          </button>
          <button
            type="button"
            className="secondary"
            disabled={busy}
            onClick={() => {
              setTempToken(null);
              setCode("");
              setFormError(null);
            }}
          >
            Back to sign in
          </button>
        </form>
        {updateBanner}
        {updateCheckButton}
      </div>
    );
  }

  if (!state.isAuthenticated) {
    return (
      <div className="app">
        <div>
          <h1>Leapsofts Time Tracker</h1>
          <p className="sub">Sign in with your ERP account</p>
        </div>
        <form
          className="card"
          onSubmit={(e) => {
            e.preventDefault();
            void signIn();
          }}
        >
          <label htmlFor="email">Email</label>
          <input
            id="email"
            type="email"
            autoComplete="username"
            value={email}
            onChange={(e) => setEmail(e.target.value)}
          />
          <label htmlFor="password">Password</label>
          <input
            id="password"
            type="password"
            autoComplete="current-password"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
          />
          {formError ? <p className="error">{formError}</p> : null}
          <button type="submit" disabled={busy || !email || !password}>
            {busy ? "Signing in…" : "Sign in"}
          </button>
        </form>
        {updateBanner}
        {updateCheckButton}
      </div>
    );
  }

  const shift = state.shift;
  const checkedIn = shift?.status === "checked_in";
  const currentBreak = openBreak(shift);
  const currentMeeting = openMeeting(shift);
  const source = currentBreak?.source ?? (currentBreak ? "manual" : null);
  const elapsed = getWorkedSeconds(shift, now);
  const sessions = shift?.sessions ?? [];
  const meetings = shift?.meetings ?? [];
  const name =
    `${state.user?.firstName ?? ""} ${state.user?.lastName ?? ""}`.trim() ||
    state.user?.email ||
    "You";

  return (
    <div className="app">
      <div className="top">
        <div>
          <h1>{name}</h1>
          <p className="sub">Idle timeout {state.idleTimeoutMinutes} min · OS-wide</p>
        </div>
        <button
          className="secondary"
          disabled={busy}
          onClick={() => run(() => invoke<TrackerState>("logout"))}
        >
          Sign out
        </button>
      </div>

      <div className="card">
        <div className="meta">
          <span>{checkedIn ? "Worked today" : "Not checked in"}</span>
          {currentMeeting ? (
            <span className="badge meeting">In meeting</span>
          ) : source ? (
            <span className={`badge ${source}`}>
              {source === "idle"
                ? "Idle break"
                : source === "sleep"
                  ? "Sleep"
                  : source === "offline"
                    ? "Offline break"
                    : "Manual break"}
            </span>
          ) : (
            <span className="badge">{state.isOnline ? "Online" : "Offline"}</span>
          )}
        </div>
        <div className="clock">{formatDuration(elapsed)}</div>
        <p className="sub">
          System idle {Math.floor(state.idleSeconds / 60)}m {state.idleSeconds % 60}s
          {state.pendingCount ? ` · ${state.pendingCount} queued interval(s)` : ""}
        </p>
        {state.lastError ? <p className="error">{state.lastError}</p> : null}
        {formError ? <p className="error">{formError}</p> : null}
      </div>

      {checkedIn && !currentBreak && !currentMeeting ? (
        <div className="card">
          {state.monitoringError ? (
            <p className="error">{state.monitoringError}</p>
          ) : (
            <p className="sub">
              Screen &amp; activity monitoring: {state.monitoringActive ? "On" : "Starting…"}
              {state.lastSampleAt
                ? ` · last capture ${new Date(state.lastSampleAt).toLocaleTimeString()}`
                : ""}
            </p>
          )}
          {state.trackingSummary ? (
            <p className="sub">
              Recording: {state.trackingSummary.app ?? "no focused app"}
              {state.trackingSummary.domain ? ` · ${state.trackingSummary.domain}` : ""}
              {" · "}
              {state.trackingSummary.pendingSegments} pending segment
              {state.trackingSummary.pendingSegments === 1 ? "" : "s"}
            </p>
          ) : (
            <p className="sub">Recording: waiting for window tracking…</p>
          )}
        </div>
      ) : null}

      <div className="row">
        {!checkedIn ? (
          <button disabled={busy} onClick={() => run(() => invoke<TrackerState>("check_in"))}>
            Check in
          </button>
        ) : (
          <button
            className="danger"
            disabled={busy || !!currentMeeting}
            onClick={() => run(() => invoke<TrackerState>("check_out"))}
          >
            Check out
          </button>
        )}
        {checkedIn && !currentBreak && !currentMeeting ? (
          <button
            className="secondary"
            disabled={busy}
            onClick={() => run(() => invoke<TrackerState>("start_break"))}
          >
            Start break
          </button>
        ) : null}
        {checkedIn && currentBreak && !currentMeeting ? (
          <button
            className="secondary"
            disabled={busy}
            onClick={() => run(() => invoke<TrackerState>("end_break"))}
          >
            End break
          </button>
        ) : null}
        {checkedIn && !currentMeeting ? (
          <button
            className="secondary"
            disabled={busy || !!currentBreak}
            onClick={() => run(() => invoke<TrackerState>("start_meeting"))}
          >
            Start meeting
          </button>
        ) : null}
        {checkedIn && currentMeeting ? (
          <button
            className="secondary"
            disabled={busy}
            onClick={() => run(() => invoke<TrackerState>("end_meeting"))}
          >
            End meeting
          </button>
        ) : null}
      </div>

      {shift && (sessions.length > 0 || meetings.length > 0 || (shift.totalMinutes ?? 0) > 0) ? (
        <div className="card day-log">
          <div className="meta">
            <span>Today</span>
            <span>Total {formatMinutesAsDuration(shift.totalMinutes)}</span>
          </div>
          {sessions.length > 0 ? (
            <ul className="day-list">
              {sessions.map((session, index) => (
                <li key={`session-${index}`}>
                  <span className="day-label">Session {index + 1}</span>
                  <span>
                    {formatClockTime(session.checkInTime)}
                    {" → "}
                    {session.checkOutTime ? formatClockTime(session.checkOutTime) : "open"}
                    {session.checkoutReason
                      ? ` · ${session.checkoutReason === "inactivity" ? "auto" : "user"}`
                      : ""}
                  </span>
                </li>
              ))}
            </ul>
          ) : null}
          {meetings.length > 0 ? (
            <ul className="day-list">
              {meetings.map((meeting, index) => (
                <li key={`meeting-${index}`}>
                  <span className="day-label">Meeting {index + 1}</span>
                  <span>
                    {formatClockTime(meeting.startTime)}
                    {" → "}
                    {meeting.endTime ? formatClockTime(meeting.endTime) : "open"}
                  </span>
                </li>
              ))}
            </ul>
          ) : null}
        </div>
      ) : null}

      {updateBanner}
      {updateCheckButton}
    </div>
  );
}
