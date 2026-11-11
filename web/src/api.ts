import type { Color, EngineInfo, GameState } from './types';

export class ApiError extends Error {
  constructor(
    readonly status: number,
    readonly code: string,
    message: string,
  ) {
    super(message);
  }
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  let response: Response;
  try {
    response = await fetch(path, init);
  } catch {
    throw new ApiError(0, 'network', 'サーバーに接続できません。通信環境を確認してください');
  }
  if (!response.ok) {
    let code = 'http';
    let message = `サーバーがエラーを返しました(${response.status})`;
    try {
      const body = await response.json();
      if (typeof body?.error?.message === 'string') {
        code = String(body.error.code);
        message = body.error.message;
      }
    } catch {
      // JSON でない応答(プロキシのエラーページなど)。
    }
    throw new ApiError(response.status, code, message);
  }
  return (await response.json()) as T;
}

export const fetchEngines = () => request<EngineInfo[]>('/api/engines');

export const fetchGame = (id: string) => request<GameState>(`/api/games/${encodeURIComponent(id)}`);

export interface CreateGameRequest {
  board_size: number;
  komi: number;
  /** human またはエンジン名。 */
  black: string;
  white: string;
  /** 両方が人間のときの、作成者の席。 */
  creator_color?: Color;
}

export interface CreateGameResponse {
  id: string;
  creator_color: Color | null;
  token: string | null;
  invite_token: string | null;
}

export const createGame = (body: CreateGameRequest) =>
  request<CreateGameResponse>('/api/games', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  });

export const sgfUrl = (id: string) => `/api/games/${encodeURIComponent(id)}/sgf`;
