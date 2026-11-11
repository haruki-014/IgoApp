import { useCallback, useEffect, useRef, useState } from 'react';
import type { ClientMessage, ErrorMessage, GameState } from './types';

export type Connection = 'connecting' | 'open' | 'reconnecting';

export interface GameError extends ErrorMessage {
  /** 同じエラーが続いても表示し直せるようにするための通し番号。 */
  seq: number;
}

/**
 * 対局の WebSocket 接続。切れたら自動でつなぎ直し、席トークンで着席し直す。
 * 盤面はサーバーから届く state をそのまま使う(着手の合否はサーバーが判定する)。
 */
export function useGame(id: string, token: string | null, enabled: boolean) {
  const [state, setState] = useState<GameState | null>(null);
  const [connection, setConnection] = useState<Connection>('connecting');
  const [error, setError] = useState<GameError | null>(null);
  const socketRef = useRef<WebSocket | null>(null);
  const seq = useRef(0);

  useEffect(() => {
    if (!enabled) return;
    let disposed = false;
    let attempts = 0;
    let timer: number | undefined;

    const connect = () => {
      const scheme = window.location.protocol === 'https:' ? 'wss' : 'ws';
      const socket = new WebSocket(`${scheme}://${window.location.host}/ws/games/${encodeURIComponent(id)}`);
      socketRef.current = socket;

      socket.onopen = () => {
        attempts = 0;
        setConnection('open');
        const join: ClientMessage = token ? { type: 'join', token } : { type: 'join' };
        socket.send(JSON.stringify(join));
      };
      socket.onmessage = (event) => {
        let message: { type?: string } & Record<string, unknown>;
        try {
          message = JSON.parse(String(event.data));
        } catch {
          return;
        }
        if (message.type === 'state') {
          setState(message as unknown as GameState);
        } else if (message.type === 'error') {
          seq.current += 1;
          setError({ code: String(message.code), message: String(message.message), seq: seq.current });
        }
      };
      socket.onclose = () => {
        if (disposed) return;
        setConnection('reconnecting');
        const delay = Math.min(5000, 400 * 2 ** attempts);
        attempts += 1;
        timer = window.setTimeout(connect, delay);
      };
      socket.onerror = () => socket.close();
    };

    setConnection('connecting');
    connect();
    return () => {
      disposed = true;
      window.clearTimeout(timer);
      socketRef.current?.close();
      socketRef.current = null;
    };
  }, [id, token, enabled]);

  const send = useCallback((message: ClientMessage): boolean => {
    const socket = socketRef.current;
    if (!socket || socket.readyState !== WebSocket.OPEN) {
      seq.current += 1;
      setError({ code: 'offline', message: 'サーバーに接続していません。再接続をお待ちください', seq: seq.current });
      return false;
    }
    socket.send(JSON.stringify(message));
    return true;
  }, []);

  const clearError = useCallback(() => setError(null), []);

  return { state, connection, error, clearError, send };
}
