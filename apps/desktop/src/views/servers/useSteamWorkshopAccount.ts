import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { describeError } from "../../app-state";
import { SingleFlightPoller } from "../../domain/single-flight-poller";
import { canUseSteamWorkshopAccountLogin, cancelSteamWorkshopAccountLogin, readSteamWorkshopAccountLogin,
  sendSteamWorkshopAccountInput, startSteamWorkshopAccountLogin, clearSteamServerAccountAuthorization, SteamWorkshopAccountInputError,
  type SteamWorkshopAccountInputErrorCode, type SteamWorkshopAccountLoginSnapshot, type SteamWorkshopAccountTarget } from "../../steam-workshop-account-api";

const finished = (snapshot: SteamWorkshopAccountLoginSnapshot) => ["authenticated", "failed", "cancelled"].includes(snapshot.phase);
const inputFailure = (reason: unknown): SteamWorkshopAccountInputErrorCode => reason instanceof SteamWorkshopAccountInputError
  ? reason.code : "steam_account_input_unconfirmed";
function sessionUnavailable(reason: unknown): boolean {
  if (reason instanceof SteamWorkshopAccountInputError) return reason.code === "steam_account_session_unavailable";
  let value: unknown = reason instanceof Error ? reason.message : reason;
  if (typeof value === "string") { try { value = JSON.parse(value); } catch { return false; } }
  return Boolean(value && typeof value === "object" && "code" in value && value.code === "steam_account_session_unavailable");
}

/** The target workspace owns its session across content and LanGameCMD views. */
export function useSteamWorkshopAccount(requestedTarget: SteamWorkshopAccountTarget | null) {
  const targetKey = requestedTarget ? `${requestedTarget.kind}:${requestedTarget.id}` : null;
  const target = useMemo(() => requestedTarget, [targetKey]);
  const [snapshot, setSnapshot] = useState<SteamWorkshopAccountLoginSnapshot | null>(null);
  const [selection, setSelection] = useState<{ targetKey: string | null; account: string | null }>({ targetKey, account: null });
  const [starting, setStarting] = useState(false), [sending, setSending] = useState(false), [cancelling, setCancelling] = useState(false);
  const [selecting, setSelecting] = useState(false);
  const [inputError, setInputError] = useState<SteamWorkshopAccountInputErrorCode | null>(null);
  const [error, setError] = useState<string | null>(null);
  const sessionFailure = useCallback((reason: unknown) => {
    if (sessionUnavailable(reason)) {
      scope.current += 1; current.current = null;
      startGate.current = false; inputGate.current = false; cancelGate.current = false; selectionGate.current = false;
      setSnapshot(null); setStarting(false); setSending(false); setCancelling(false); setSelecting(false);
      recovery.current?.reject(new Error(JSON.stringify({ code: "steam_account_session_unavailable", message: "The SteamCMD login session is unavailable." })));
      recovery.current = null; setRecoveryPending(false);
      setInputError("steam_account_session_unavailable"); setError(null); return;
    }
    if (reason instanceof SteamWorkshopAccountInputError) {
      setInputError(reason.code); setError(null);
    } else {
      setError(describeError(reason)); setInputError(null);
    }
  }, []);
  const [recoveryPending, setRecoveryPending] = useState(false);
  const recovery = useRef<{ targetKey: string; resolve: (account: string) => void; reject: (reason: Error) => void } | null>(null);
  const current = useRef<SteamWorkshopAccountLoginSnapshot | null>(null);
  const ownerToken = useMemo(() => Symbol("steam-workshop-account-owner"), [targetKey]);
  const owner = useRef<symbol | null>(null);
  const isCurrentOwner = useCallback(() => owner.current === ownerToken, [ownerToken]);
  const scope = useRef(0), startGate = useRef(false), inputGate = useRef(false), cancelGate = useRef(false), selectionGate = useRef(false);
  useEffect(() => {
    owner.current = ownerToken;
    scope.current += 1;
    current.current = null;
    startGate.current = false; inputGate.current = false; cancelGate.current = false; selectionGate.current = false;
    setSnapshot(null); setSelection({ targetKey, account: null });
    setStarting(false); setSending(false); setCancelling(false); setSelecting(false); setInputError(null); setError(null); setRecoveryPending(false);
    return () => {
      owner.current = null;
      scope.current += 1;
      recovery.current?.reject(new Error(JSON.stringify({ code: "steam_workshop_login_cancelled", message: "Workshop account login was cancelled." })));
      recovery.current = null;
      const active = current.current;
      if (active && !finished(active)) void cancelSteamWorkshopAccountLogin(active.target, active.session_id).catch(() => {
        console.warn("SteamCMD session cleanup was not confirmed.");
      });
    };
  }, [targetKey, ownerToken]);

  const receive = useCallback((next: SteamWorkshopAccountLoginSnapshot, version: number) => {
    if (scope.current !== version || `${next.target.kind}:${next.target.id}` !== targetKey) return;
    const previous = current.current;
    if (previous && previous.session_id === next.session_id && previous.revision > next.revision) return;
    current.current = next;
    setSnapshot(next);
    setError(null);
    if (previous && previous.session_id === next.session_id && previous.revision < next.revision) setInputError(null);
    if (next.phase === "authenticated" && next.account_name &&
      (previous?.session_id !== next.session_id || previous.phase !== "authenticated")) setSelection({ targetKey, account: next.account_name });
    if (recovery.current?.targetKey === `${next.target.kind}:${next.target.id}` && next.phase === "authenticated" && next.account_name) {
      recovery.current.resolve(next.account_name); recovery.current = null; setRecoveryPending(false);
    }
    if (recovery.current?.targetKey === `${next.target.kind}:${next.target.id}` && next.phase === "cancelled") {
      recovery.current.reject(new Error(JSON.stringify({ code: "steam_workshop_login_cancelled", message: "Workshop account login was cancelled." })));
      recovery.current = null; setRecoveryPending(false);
    }
  }, [targetKey]);
  const start = useCallback(async () => {
    if (!target || !isCurrentOwner() || startGate.current || inputGate.current || cancelGate.current || selectionGate.current || (current.current && !finished(current.current))) return;
    const version = ++scope.current;
    startGate.current = true;
    current.current = null; setSnapshot(null); setStarting(true); setInputError(null); setError(null);
    setSelection({ targetKey, account: null });
    try {
      const next = await startSteamWorkshopAccountLogin(target, `steam-workshop-login-${crypto.randomUUID()}`);
      if (scope.current !== version) {
        if (!finished(next)) {
          try { await cancelSteamWorkshopAccountLogin(next.target, next.session_id); }
          catch { console.warn("SteamCMD session cleanup was not confirmed."); }
        }
        return;
      }
      receive(next, version);
    } catch (reason) {
      if (scope.current === version) sessionFailure(reason);
    } finally {
      if (scope.current === version) { startGate.current = false; setStarting(false); }
    }
  }, [target, targetKey, isCurrentOwner, receive, sessionFailure]);
  const [recoveryPurpose, setRecoveryPurpose] = useState<"workshop" | "server-install">("workshop");
  const requestAccount = useCallback((purpose: "workshop" | "server-install" = "workshop") => {
    if (!target || !targetKey || !isCurrentOwner()) return Promise.reject(new Error(JSON.stringify({ code: "steam_workshop_login_cancelled", message: "Steam account login was cancelled." })));
    if (recovery.current) return Promise.reject(new Error("A Workshop account login is already pending."));
    const result = new Promise<string>((resolve, reject) => { recovery.current = { targetKey, resolve, reject }; });
    setRecoveryPending(true); setRecoveryPurpose(purpose);
    void start();
    return result;
  }, [target, targetKey, isCurrentOwner, start]);
  const active = snapshot && `${snapshot.target.kind}:${snapshot.target.id}` === targetKey ? snapshot : null;
  const loginActive = Boolean(active && !finished(active));
  useEffect(() => {
    if (!active || !loginActive) return;
    const version = scope.current, sessionId = active.session_id, id = active.target;
    const poller = new SingleFlightPoller({ intervalMs: 500,
      poll: () => readSteamWorkshopAccountLogin(id, sessionId),
      schedule: (callback, delay) => window.setTimeout(callback, delay), cancel: (timer: number) => window.clearTimeout(timer),
      onValue: (value) => receive(value, version),
      onError: (reason) => { if (scope.current === version) sessionFailure(reason); }
    });
    poller.start();
    return () => poller.dispose();
  }, [active?.session_id, loginActive, receive, sessionFailure]);

  const send = useCallback(async (input: string, prompt: Pick<SteamWorkshopAccountLoginSnapshot, "session_id" | "revision">) => {
    const target = current.current;
    if (!target || `${target.target.kind}:${target.target.id}` !== targetKey || inputGate.current || cancelGate.current) return;
    if (target.session_id !== prompt.session_id || target.revision !== prompt.revision) { setInputError("steam_account_prompt_changed"); return; }
    if (target.input_mode === "disabled") return;
    if (input.length === 0) { setInputError("steam_account_input_invalid"); return; }
    const version = scope.current;
    inputGate.current = true; setSending(true); setInputError(null); setError(null);
    try { receive(await sendSteamWorkshopAccountInput(target.target, target.session_id, prompt.revision, input), version); }
    // A rejected secret submission must never reflect an IPC error containing its input.
    catch (reason) { if (scope.current === version) setInputError(inputFailure(reason)); }
    finally { if (scope.current === version) { inputGate.current = false; setSending(false); } }
  }, [targetKey, receive]);
  const cancel = useCallback(async () => {
    const target = current.current;
    if (!target || finished(target)) {
      recovery.current?.reject(new Error(JSON.stringify({ code: "steam_workshop_login_cancelled", message: "Workshop account login was cancelled." })));
      recovery.current = null; setRecoveryPending(false); return;
    }
    if (cancelGate.current) return;
    const version = scope.current;
    cancelGate.current = true; setCancelling(true); setInputError(null); setError(null);
    try { receive(await cancelSteamWorkshopAccountLogin(target.target, target.session_id), version); }
    catch (reason) { if (scope.current === version) sessionFailure(reason); }
    finally { if (scope.current === version) { cancelGate.current = false; setCancelling(false); } }
  }, [receive, sessionFailure]);
  const loginAccount = selection.targetKey === targetKey ? selection.account ?? undefined : undefined;
  const selectAccount = useCallback(async (account: string | null) => {
    if (!target || !isCurrentOwner() || startGate.current || inputGate.current || cancelGate.current || selectionGate.current || (current.current && !finished(current.current))) return;
    if (account !== null && account !== current.current?.account_name) return;
    if (account !== null) { setSelection({ targetKey, account }); return; }
    const version = scope.current;
    selectionGate.current = true; setSelecting(true); setInputError(null); setError(null);
    try {
      await clearSteamServerAccountAuthorization(target);
      if (scope.current === version) setSelection({ targetKey, account: null });
    } catch (reason) { if (scope.current === version) sessionFailure(reason); }
    finally { if (scope.current === version) { selectionGate.current = false; setSelecting(false); } }
  }, [target, targetKey, isCurrentOwner, sessionFailure]);
  const isBusy = useCallback(() => Boolean(recovery.current) || startGate.current || inputGate.current || cancelGate.current ||
    selectionGate.current || Boolean(current.current && !finished(current.current)), []);
  return { snapshot: active, available: canUseSteamWorkshopAccountLogin(), start, send, cancel, selectAccount, loginAccount,
    authenticatedAccount: active?.phase === "authenticated" ? active.account_name : null,
    busy: starting || loginActive || sending || cancelling || selecting, isBusy, isCurrentOwner, starting, sending, cancelling, selecting, inputError, error, requestAccount, recoveryPending, recoveryPurpose, target };
}

export type SteamWorkshopAccountState = ReturnType<typeof useSteamWorkshopAccount>;
