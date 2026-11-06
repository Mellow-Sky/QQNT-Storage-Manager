import { useCallback, useEffect, useRef, useState } from "react";
import { api, desktopAvailable, message } from "../services/api";
import type { Grant, Session, Status } from "../types";

export function useManager() {
  const [grants, setGrants] = useState<Grant[]>([]);
  const [grantId, setGrantId] = useState("");
  const [sessions, setSessions] = useState<Session[]>([]);
  const [sessionId, setSessionId] = useState("");
  const [status, setStatus] = useState<Status | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const completed = useRef("");
  const refresh = useCallback(async (selectId?: string) => {
    const history = await api.sessions();
    setSessions(history);
    if (selectId) setSessionId(selectId);
    else setSessionId((current) => current || history[0]?.id || "");
  }, []);
  useEffect(() => {
    if (!desktopAvailable) return;
    let active = true;
    void api
      .sessions()
      .then((history) => {
        if (active) {
          setSessions(history);
          setSessionId(history[0]?.id ?? "");
        }
      })
      .catch((e) => {
        if (active) setError(message(e));
      });
    let timer: ReturnType<typeof setTimeout>;
    const poll = async () => {
      try {
        const next = await api.status();
        if (!active) return;
        setStatus(next);
        if (
          next.sessionId &&
          ["completed", "cancelled", "failed"].includes(next.state) &&
          completed.current !== next.sessionId
        ) {
          completed.current = next.sessionId;
          await refresh(next.sessionId);
          if (next.error && active) setError(next.error);
        }
      } catch (e) {
        if (active) setError(message(e));
      }
      if (active) timer = setTimeout(() => void poll(), 350);
    };
    void poll();
    return () => {
      active = false;
      clearTimeout(timer);
    };
  }, [refresh]);
  const applySession = useCallback((session: Session) => {
    setSessions((current) =>
      current.some((item) => item.id === session.id)
        ? current.map((item) => (item.id === session.id ? session : item))
        : [session, ...current],
    );
  }, []);
  const selectSession = (id: string) => {
    setSessionId(id);
    if (desktopAvailable)
      void api
        .session(id)
        .then(applySession)
        .catch((error) => setError(message(error)));
  };
  const perform = useCallback(async (operation: () => Promise<void>) => {
    setError("");
    setBusy(true);
    try {
      await operation();
    } catch (e) {
      setError(message(e));
    } finally {
      setBusy(false);
    }
  }, []);
  const choose = (automatic = false) =>
    perform(async () => {
      const found = await (automatic ? api.discover() : api.choose());
      if (!found.length) {
        if (automatic)
          setError(
            "默认位置中未找到结构可验证的 QQNT 目录，请手动选择自定义存储目录。",
          );
        return;
      }
      setGrants((current) => [
        ...current.filter((g) => !found.some((f) => f.id === g.id)),
        ...found,
      ]);
      setGrantId(found[0].id);
      await api.start(found[0].id);
      setStatus(await api.status());
    });
  const start = (id = grantId) =>
    perform(async () => {
      await api.start(id);
      setStatus(await api.status());
    });
  const cancel = () =>
    perform(async () => {
      if (status?.sessionId) await api.cancel(status.sessionId);
    });
  return {
    grants,
    grantId,
    setGrantId,
    grant: grants.find((g) => g.id === grantId),
    sessions,
    sessionId,
    setSessionId: selectSession,
    applySession,
    session: sessions.find((s) => s.id === sessionId),
    status,
    error,
    setError,
    busy,
    choose,
    start,
    cancel,
    perform,
    reload: refresh,
  };
}
