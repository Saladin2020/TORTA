const TOKEN_KEY = "dice_token";

export function getToken(): string | null {
  return localStorage.getItem(TOKEN_KEY);
}

export function setToken(token: string | null) {
  if (token) localStorage.setItem(TOKEN_KEY, token);
  else localStorage.removeItem(TOKEN_KEY);
}

export async function api<T>(
  path: string,
  opts: RequestInit & { auth?: boolean; agent?: boolean } = {},
): Promise<T> {
  const headers = new Headers(opts.headers);
  if (!headers.has("Content-Type") && !(opts.body instanceof FormData)) {
    headers.set("Content-Type", "application/json");
  }
  if (opts.auth !== false) {
    const t = getToken();
    if (t) headers.set("Authorization", `Bearer ${t}`);
  }
  const res = await fetch(path, { ...opts, headers });
  if (!res.ok) {
    const text = await res.text();
    throw new Error(text || res.statusText);
  }
  if (res.status === 204) return undefined as T;
  const ct = res.headers.get("content-type") || "";
  if (ct.includes("application/json")) return res.json();
  return res as unknown as T;
}

export type Me = {
  id: string;
  username: string;
  role: string;
  must_change_password?: boolean;
};
export type Snapshot = {
  id: string;
  label: string;
  source: string;
  status: string;
  started_at: string;
  finished_at: string | null;
  file_count: number;
  raw_bytes: number;
  stored_bytes: number;
  deleted_at?: string | null;
};
export type AuditEntry = {
  id: number;
  actor_user_id: string | null;
  action: string;
  detail: unknown;
  created_at: string;
  actor_username: string | null;
};
export type SnapshotChunk = {
  index: number;
  hash: string;
  raw_size: number;
  compressed_size: number;
};
export type SnapshotFile = {
  id: string;
  snapshot_id?: string;
  relative_path: string;
  size_bytes: number;
  content_hash: string;
  chunks: SnapshotChunk[];
  backed_up_at?: string;
  /** Other paths sharing the same content (unique store view) */
  aliases?: string[];
};
export type StorageTarget = {
  id: string;
  name: string;
  path: string;
  is_default: boolean;
  quota_bytes: number | null;
  used_bytes: number;
  notes: string;
};
export type Policy = {
  id: string;
  name: string;
  enabled: boolean;
  schedule_cron: string;
  paths_json: string[];
  excludes_json: string[];
  retention_days: number;
};
export type Agent = {
  id: string;
  name: string;
  username: string;
  hostname: string;
  os: string;
  enabled: boolean;
  local_enabled: boolean;
  status: string;
  last_seen_at: string | null;
  policy_id: string | null;
};
export type User = {
  id: string;
  username: string;
  display_name: string;
  role: string;
  auth_provider: string;
  enabled: boolean;
};
