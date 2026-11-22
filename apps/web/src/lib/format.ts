export function fmtBytes(n: number) {
  if (n < 1024) return `${n} B`;
  if (n < 1024 ** 2) return `${(n / 1024).toFixed(1)} KB`;
  if (n < 1024 ** 3) return `${(n / 1024 ** 2).toFixed(1)} MB`;
  return `${(n / 1024 ** 3).toFixed(2)} GB`;
}

export function shortHash(h: string) {
  if (h.length <= 18) return h;
  return `${h.slice(0, 8)}…${h.slice(-6)}`;
}

export function statusTone(status: string) {
  if (status === "success" || status === "online") return "ok";
  if (status === "running" || status === "queued") return "warn";
  if (status === "failed" || status === "disabled") return "bad";
  return "muted";
}
