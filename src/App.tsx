import { useEffect, useState, type JSX } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { LoginResult, TrackerState } from "./types";
import { formatDuration, getWorkedSeconds, openBreak } from "./time";

export default function App(): JSX.Element {
  const [state, setState] = useState<TrackerState | null>(null);
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [tempToken, setTempToken] = useState<string | null>(null);
  const [code, setCode] = useState("");
  const [busy, setBusy] = useState(false);
  const [formError, setFormError] = useState<string | null>(null);
  const [now, setNow] = useState(Date.now());

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
      </div>
    );
  }

  const shift = state.shift;
  const checkedIn = shift?.status === "checked_in";
  const currentBreak = openBreak(shift);
  const source = currentBreak?.source ?? (currentBreak ? "manual" : null);
  const elapsed = getWorkedSeconds(shift, now);
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
          {source ? (
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

      {checkedIn && !currentBreak ? (
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
            disabled={busy}
            onClick={() => run(() => invoke<TrackerState>("check_out"))}
          >
            Check out
          </button>
        )}
        {checkedIn && !currentBreak ? (
          <button
            className="secondary"
            disabled={busy}
            onClick={() => run(() => invoke<TrackerState>("start_break"))}
          >
            Start break
          </button>
        ) : null}
        {checkedIn && currentBreak ? (
          <button
            className="secondary"
            disabled={busy}
            onClick={() => run(() => invoke<TrackerState>("end_break"))}
          >
            End break
          </button>
        ) : null}
      </div>
    </div>
  );
}
