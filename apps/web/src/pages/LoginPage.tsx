import { FormEvent, useState } from "react";
import { useNavigate } from "react-router-dom";
import { faKey, faRightToBracket, faUser } from "@fortawesome/free-solid-svg-icons";
import { api, Me, setToken } from "../api";
import Icon from "../components/Icon";

export default function LoginPage({ onLogin }: { onLogin: (me: Me) => void }) {
  const [username, setUsername] = useState("admin");
  const [password, setPassword] = useState("");
  const [provider, setProvider] = useState("auto");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const nav = useNavigate();

  async function submit(e: FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError("");
    try {
      const res = await api<{
        token: string;
        username: string;
        role: string;
        must_change_password?: boolean;
      }>("/api/auth/login", {
        method: "POST",
        auth: false,
        body: JSON.stringify({ username, password, provider }),
      });
      setToken(res.token);
      const me = await api<Me>("/api/auth/me");
      onLogin({
        ...me,
        must_change_password: res.must_change_password ?? me.must_change_password,
      });
      nav(res.must_change_password ? "/account" : "/files");
    } catch (err) {
      setError(err instanceof Error ? err.message : "login failed");
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="login-wrap">
      <form className="panel login-panel" onSubmit={submit}>
        <div className="login-brand">
          <img className="login-logo" src="/logo.jpg" alt="TORTA Dice" width={40} height={40} />
          <div>
            <h1>TORTA Dice</h1>
            <p className="sub">สำรองข้อมูลองค์กร · local หรือ AD/LDAP</p>
          </div>
        </div>

        <div className="grid">
          <div className="field">
            <label>
              <Icon icon={faUser} /> ชื่อผู้ใช้
            </label>
            <input
              value={username}
              onChange={(e) => setUsername(e.target.value)}
              autoFocus
              autoComplete="username"
            />
          </div>
          <div className="field">
            <label>
              <Icon icon={faKey} /> รหัสผ่าน
            </label>
            <input
              type="password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              autoComplete="current-password"
            />
          </div>
          <div className="field">
            <label>วิธียืนยันตัวตน</label>
            <select value={provider} onChange={(e) => setProvider(e.target.value)}>
              <option value="auto">อัตโนมัติ (local แล้ว LDAP)</option>
              <option value="local">Local account</option>
              <option value="ldap">AD / LDAP</option>
            </select>
          </div>
        </div>

        {error && (
          <div className="alert error" style={{ marginTop: 14 }}>
            {error}
          </div>
        )}

        <button className="btn" disabled={busy} type="submit">
          <Icon icon={faRightToBracket} />
          {busy ? "กำลังเข้าสู่ระบบ…" : "เข้าสู่ระบบ"}
        </button>
      </form>
    </div>
  );
}
