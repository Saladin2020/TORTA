import { FormEvent, useState } from "react";
import { faKey, faShieldHalved } from "@fortawesome/free-solid-svg-icons";
import { api } from "../api";
import Icon from "../components/Icon";

export default function AccountPage({
  force = false,
  onDone,
}: {
  force?: boolean;
  onDone?: () => void;
}) {
  const [current, setCurrent] = useState("");
  const [next, setNext] = useState("");
  const [confirm, setConfirm] = useState("");
  const [error, setError] = useState("");
  const [msg, setMsg] = useState("");
  const [busy, setBusy] = useState(false);

  async function submit(e: FormEvent) {
    e.preventDefault();
    setError("");
    setMsg("");
    if (next.length < 10) {
      setError("รหัสผ่านใหม่ต้องอย่างน้อย 10 ตัวอักษ");
      return;
    }
    if (next !== confirm) {
      setError("รหัสผ่านใหม่ไม่ตรงกัน");
      return;
    }
    setBusy(true);
    try {
      await api("/api/auth/change-password", {
        method: "POST",
        body: JSON.stringify({ current_password: current, new_password: next }),
      });
      setMsg("เปลี่ยนรหัสผ่านแล้ว");
      setCurrent("");
      setNext("");
      setConfirm("");
      onDone?.();
    } catch (err) {
      setError(err instanceof Error ? err.message : "failed");
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="stack">
      <header className="page-header">
        <div>
          <h1>บัญชี</h1>
          <p>
            {force
              ? "ต้องเปลี่ยนรหัสผ่านก่อนใช้งานต่อ"
              : "เปลี่ยนรหัสผ่านบัญชี local"}
          </p>
        </div>
      </header>

      {force && (
        <div className="alert warn">
          <Icon icon={faShieldHalved} />
          บัญชีนี้ถูกตั้งให้เปลี่ยนรหัสผ่าน (bootstrap / ผู้ดูแลสร้างใหม่)
        </div>
      )}

      <section className="panel" style={{ maxWidth: 420 }}>
        <h2 className="section-title">
          <Icon icon={faKey} /> เปลี่ยนรหัสผ่าน
        </h2>
        <form className="grid" onSubmit={submit}>
          <div className="field">
            <label>รหัสผ่านปัจจุบัน</label>
            <input
              type="password"
              value={current}
              onChange={(e) => setCurrent(e.target.value)}
              required
              autoComplete="current-password"
            />
          </div>
          <div className="field">
            <label>รหัสผ่านใหม่</label>
            <input
              type="password"
              value={next}
              onChange={(e) => setNext(e.target.value)}
              required
              autoComplete="new-password"
            />
          </div>
          <div className="field">
            <label>ยืนยันรหัสผ่านใหม่</label>
            <input
              type="password"
              value={confirm}
              onChange={(e) => setConfirm(e.target.value)}
              required
              autoComplete="new-password"
            />
          </div>
          {error && <div className="alert error">{error}</div>}
          {msg && <div className="alert ok">{msg}</div>}
          <button className="btn" type="submit" disabled={busy}>
            {busy ? "กำลังบันทึก…" : "บันทึกรหัสผ่าน"}
          </button>
        </form>
      </section>
    </div>
  );
}
