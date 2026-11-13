import { useEffect, useRef, useState } from "react";

export type UseAsyncResult<T> = {
  data: T | null;
  loading: boolean;
  error: unknown;
  reload: () => void;
};

type AsyncState<T> = {
  key: string;
  data: T | null;
  error: unknown;
};

function serializeDeps(deps: readonly unknown[]): string {
  try {
    return JSON.stringify(deps, (_key, value: unknown) => {
      if (typeof value === "function") {
        return "[function]";
      }
      if (typeof value === "bigint") {
        return value.toString();
      }
      return value;
    });
  } catch {
    return deps.map((dep) => String(dep)).join("|");
  }
}

/**
 * 轻量异步数据 hook：deps 变化或 reload 时自动重新请求，
 * 自动忽略卸载后与过期请求的 setState。
 */
export function useAsync<T>(fn: () => Promise<T>, deps: readonly unknown[]): UseAsyncResult<T> {
  const [reloadToken, setReloadToken] = useState(0);
  const [state, setState] = useState<AsyncState<T> | null>(null);
  const requestKey = `${reloadToken}:${serializeDeps(deps)}`;

  const fnRef = useRef(fn);
  useEffect(() => {
    fnRef.current = fn;
  });

  useEffect(() => {
    let cancelled = false;
    void Promise.resolve()
      .then(() => fnRef.current())
      .then(
        (data) => {
          if (!cancelled) {
            setState({ key: requestKey, data, error: null });
          }
        },
        (error: unknown) => {
          if (!cancelled) {
            setState((previous) => ({
              key: requestKey,
              data: previous ? previous.data : null,
              error,
            }));
          }
        },
      );
    return () => {
      cancelled = true;
    };
  }, [requestKey]);

  const loading = state === null || state.key !== requestKey;
  const current = state !== null && state.key === requestKey ? state : null;

  return {
    data: state ? state.data : null,
    loading,
    error: current ? current.error : null,
    reload: () => setReloadToken((token) => token + 1),
  };
}
