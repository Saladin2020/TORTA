import { FormEvent, useEffect, useState } from "react";
import {
  faClipboardList,
  faClock,
  faDatabase,
  faHardDrive,
  faPlay,
  faPlus,
  faRobot,
  faTrashCan,
  faUserPlus,
  faUsers,
} from "@fortawesome/free-solid-svg-icons";
import type { IconDefinition } from "@fortawesome/fontawesome-svg-core";
import { Agent, api, AuditEntry, Policy, StorageTarget, User } from "../api";
import AgentDownload from "../components/AgentDownload";
import Icon from "../components/Icon";
import { fmtBytes, statusTone } from "../lib/format";

type Stats = {
  users: number;
  agents: number;
  snapshots: number;
  store_path: string;
  store_used_bytes: number;
  store_quota_bytes: number | null;
};

type TabId = "overview" | "storage" | "policies" | "agents" | "users" | "audit";

const TABS: { id: TabId; label: string; icon: IconDefinition }[] = [
  { id: "overview", label: "ภาพรวม", icon: faDatabase },
  { id: "storage", label: "Storage", icon: faHardDrive },
  { id: "policies", label: "Policies", icon: faClock },
  { id: "agents", label: "Agents", icon: faRobot },
  { id: "users", label: "ผู้ใช้", icon: faUsers },
  { id: "audit", label: "Audit", icon: faClipboardList },
];

export default function AdminPage({ role = "admin" }: { role?: string }) {
  const isAdmin = role === "admin";
  const [tab, setTab] = useState<TabId>("overview");
  const [stats, setStats] = useState<Stats | null>(null);
  const [storage, setStorage] = useState<StorageTarget[]>([]);
  const [policies, setPolicies] = useState<Policy[]>([]);
  const [agents, setAgents] = useState<Agent[]>([]);
  const [users, setUsers] = useState<User[]>([]);
  const [audit, setAudit] = useState<AuditEntry[]>([]);
  const [error, setError] = useState("");
  const [tokenFlash, setTokenFlash] = useState("");
  const [gcMsg, setGcMsg] = useState("");
  const [busyGc, setBusyGc] = useState(false);

  const [storeName, setStoreName] = useState("secondary");
  const [storePath, setStorePath] = useState("/var/lib/dice/store2");
  const [policyName, setPolicyName] = useState("nightly-docs");
  const [policyCron, setPolicyCron] = useState("0 2 * * *");
  const [policyPaths, setPolicyPaths] = useState("Documents,Desktop");
  const [newUser, setNewUser] = useState({ username: "", password: "", role: "user" });
  const [agentForm, setAgentForm] = useState({ user_id: "", name: "", policy_id: "" });

  async function refresh() {
    try {
      const [s, st, p, a, u, au] = await Promise.all([
        api<Stats>("/api/admin/stats"),
        api<StorageTarget[]>("/api/admin/storage"),
        api<Policy[]>("/api/admin/policies"),
        api<Agent[]>("/api/admin/agents"),
        api<User[]>("/api/admin/users"),
        api<AuditEntry[]>("/api/admin/audit"),
      ]);
      setStats(s);
      setStorage(st);
      setPolicies(p);
      setAgents(a);
      setUsers(u);
      setAudit(au);
      setError("");
      if (!agentForm.user_id && u[0]) {
        setAgentForm((f) => ({ ...f, user_id: u[0].id, policy_id: p[0]?.id || "" }));
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : "load failed");
    }
  }

  useEffect(() => {
    refresh();
  }, []);

  async function createStorage(e: FormEvent) {
    e.preventDefault();
    await api("/api/admin/storage", {
      method: "POST",
      body: JSON.stringify({ name: storeName, path: storePath, is_default: false }),
    });
    refresh();
  }

  async function createPolicy(e: FormEvent) {
    e.preventDefault();
    await api("/api/admin/policies", {
      method: "POST",
      body: JSON.stringify({
        name: policyName,
        schedule_cron: policyCron,
        paths: policyPaths.split(",").map((s) => s.trim()).filter(Boolean),
        retention_days: 30,
      }),
    });
    refresh();
  }

  async function createUser(e: FormEvent) {
    e.preventDefault();
    await api("/api/admin/users", {
      method: "POST",
      body: JSON.stringify(newUser),
    });
    setNewUser({ username: "", password: "", role: "user" });
    refresh();
  }

  async function createAgent(e: FormEvent) {
    e.preventDefault();
    const res = await api<{ agent_token: string }>("/api/admin/agents", {
      method: "POST",
      body: JSON.stringify({
        user_id: agentForm.user_id,
        name: agentForm.name,
        policy_id: agentForm.policy_id || null,
      }),
    });
    setTokenFlash(res.agent_token);
    refresh();
  }

  async function toggleAgent(id: string, enabled: boolean) {
    await api(`/api/admin/agents/${id}`, {
      method: "PUT",
      body: JSON.stringify({ enabled }),
    });
    refresh();
  }

  async function triggerRunNow(id: string) {
    await api(`/api/admin/agents/${id}/run-now`, { method: "POST" });
    setGcMsg("สั่ง run-now แล้ว — agent จะเริ่มเมื่อ poll / WebSocket รอบถัดไป");
  }

  async function runGc() {
    setBusyGc(true);
    setGcMsg("");
    try {
      const res = await api<{ message: string }>("/api/admin/gc", { method: "POST" });
      setGcMsg(res.message);
      refresh();
    } catch (e) {
      setError(e instanceof Error ? e.message : "GC failed");
    } finally {
      setBusyGc(false);
    }
  }

  async function setStorageQuota(id: string) {
    const raw = window.prompt("โควต้า (bytes) — ว่าง = ไม่จำกัด", "");
    if (raw === null) return;
    const quota_bytes = raw.trim() === "" ? -1 : Number(raw);
    await api(`/api/admin/storage/${id}`, {
      method: "PUT",
      body: JSON.stringify({ quota_bytes }),
    });
    refresh();
  }

  async function setUserQuota(id: string) {
    const raw = window.prompt("โควต้าผู้ใช้ (bytes) — ว่าง = ไม่จำกัด, -1 = ล้าง", "");
    if (raw === null) return;
    const quota_bytes = raw.trim() === "" ? null : Number(raw);
    await api(`/api/admin/users/${id}`, {
      method: "PUT",
      body: JSON.stringify({ quota_bytes: quota_bytes ?? -1 }),
    });
    refresh();
  }

  async function togglePolicy(id: string, enabled: boolean) {
    await api(`/api/admin/policies/${id}`, {
      method: "PUT",
      body: JSON.stringify({ enabled }),
    });
    refresh();
  }

  return (
    <div className="stack">
      <header className="page-header">
        <div>
          <h1>จัดการระบบ</h1>
          <p>shared Dice store · แยกตามแท็บ</p>
        </div>
        <div className="page-actions">
          {isAdmin && tab === "overview" && (
            <button className="btn" type="button" disabled={busyGc} onClick={runGc}>
              <Icon icon={faTrashCan} />
              {busyGc ? "กำลัง GC…" : "รัน retention GC"}
            </button>
          )}
        </div>
      </header>

      {error && <div className="alert error">{error}</div>}
      {gcMsg && <div className="alert ok">{gcMsg}</div>}

      <div className="tabs" role="tablist" aria-label="ส่วนจัดการระบบ">
        {TABS.map((t) => (
          <button
            key={t.id}
            type="button"
            role="tab"
            aria-selected={tab === t.id}
            className={`tab ${tab === t.id ? "active" : ""}`}
            onClick={() => setTab(t.id)}
          >
            <Icon icon={t.icon} />
            {t.label}
          </button>
        ))}
      </div>

      {tab === "overview" && (
        <section className="panel" role="tabpanel">
          <h2 className="section-title">
            <Icon icon={faDatabase} /> ภาพรวมระบบ
          </h2>
          <p className="section-lead">สถานะคลังและจำนวนทรัพยากรหลัก</p>
          {stats && (
            <div className="grid stats">
              <div className="stat">
                <div className="label">
                  <Icon icon={faUsers} /> ผู้ใช้
                </div>
                <div className="value">{stats.users}</div>
              </div>
              <div className="stat">
                <div className="label">
                  <Icon icon={faRobot} /> Agents
                </div>
                <div className="value">{stats.agents}</div>
              </div>
              <div className="stat">
                <div className="label">Snapshots</div>
                <div className="value">{stats.snapshots}</div>
              </div>
              <div className="stat">
                <div className="label">
                  <Icon icon={faHardDrive} /> Store used
                </div>
                <div className="value" style={{ fontSize: "0.95rem" }}>
                  {fmtBytes(stats.store_used_bytes)}
                  {stats.store_quota_bytes != null
                    ? ` / ${fmtBytes(stats.store_quota_bytes)}`
                    : ""}
                </div>
              </div>
              <div className="stat">
                <div className="label">Store path</div>
                <div className="value" style={{ fontSize: "0.85rem" }}>
                  {stats.store_path}
                </div>
              </div>
            </div>
          )}
        </section>
      )}

      {tab === "storage" && (
        <section className="panel" role="tabpanel">
          <h2 className="section-title">
            <Icon icon={faHardDrive} /> Storage targets
          </h2>
          <p className="section-lead">ตำแหน่งเก็บ chunk บน Ubuntu server</p>
          <table className="data-table">
            <thead>
              <tr>
                <th>ชื่อ</th>
                <th>path</th>
                <th>used / quota</th>
                <th>default</th>
                <th></th>
              </tr>
            </thead>
            <tbody>
              {storage.map((s) => (
                <tr key={s.id}>
                  <td>{s.name}</td>
                  <td className="mono">{s.path}</td>
                  <td className="mono">
                    {fmtBytes(s.used_bytes || 0)}
                    {s.quota_bytes != null ? ` / ${fmtBytes(s.quota_bytes)}` : " / ∞"}
                  </td>
                  <td>
                    {s.is_default ? <span className="badge ok">default</span> : "—"}
                  </td>
                  <td>
                    {isAdmin && (
                      <button
                        className="btn ghost"
                        type="button"
                        onClick={() => setStorageQuota(s.id)}
                      >
                        ตั้งโควต้า
                      </button>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          {isAdmin && (
            <form className="row" style={{ marginTop: 14 }} onSubmit={createStorage}>
              <div className="field">
                <label>ชื่อ</label>
                <input value={storeName} onChange={(e) => setStoreName(e.target.value)} />
              </div>
              <div className="field">
                <label>path</label>
                <input value={storePath} onChange={(e) => setStorePath(e.target.value)} />
              </div>
              <button className="btn" type="submit">
                <Icon icon={faPlus} /> เพิ่มที่เก็บ
              </button>
            </form>
          )}
        </section>
      )}

      {tab === "policies" && (
        <section className="panel" role="tabpanel">
          <h2 className="section-title">
            <Icon icon={faClock} /> Policies / ตารางดึงข้อมูล
          </h2>
          <p className="section-lead">cron แบบ `นาที ชั่วโมง * * *` — agent จะรันเมื่อถึงเวลา</p>
          <table className="data-table">
            <thead>
              <tr>
                <th>ชื่อ</th>
                <th>schedule</th>
                <th>paths</th>
                <th>retention</th>
                <th>สถานะ</th>
              </tr>
            </thead>
            <tbody>
              {policies.map((p) => (
                <tr key={p.id}>
                  <td>{p.name}</td>
                  <td className="mono">{p.schedule_cron}</td>
                  <td className="mono">
                    {(Array.isArray(p.paths_json) ? p.paths_json : []).join(", ")}
                  </td>
                  <td>{p.retention_days} วัน</td>
                  <td>
                    {isAdmin ? (
                      <button
                        className="btn ghost"
                        type="button"
                        onClick={() => togglePolicy(p.id, !p.enabled)}
                      >
                        <span className={`badge ${p.enabled ? "ok" : "muted"}`}>
                          {p.enabled ? "เปิดอยู่" : "ปิดอยู่"}
                        </span>
                      </button>
                    ) : (
                      <span className={`badge ${p.enabled ? "ok" : "muted"}`}>
                        {p.enabled ? "เปิดอยู่" : "ปิดอยู่"}
                      </span>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          {isAdmin && (
            <form className="row" style={{ marginTop: 14 }} onSubmit={createPolicy}>
              <div className="field">
                <label>ชื่อ</label>
                <input value={policyName} onChange={(e) => setPolicyName(e.target.value)} />
              </div>
              <div className="field">
                <label>cron</label>
                <input value={policyCron} onChange={(e) => setPolicyCron(e.target.value)} />
              </div>
              <div className="field">
                <label>paths (คั่นด้วย ,)</label>
                <input value={policyPaths} onChange={(e) => setPolicyPaths(e.target.value)} />
              </div>
              <button className="btn" type="submit">
                <Icon icon={faPlus} /> สร้าง policy
              </button>
            </form>
          )}
        </section>
      )}

      {tab === "agents" && (
        <section className="panel" role="tabpanel">
          <h2 className="section-title">
            <Icon icon={faRobot} /> Agents
          </h2>
          <p className="section-lead">เปิด/ปิดจาก server — client ยังมี local kill-switch แยก</p>
          <div className="row" style={{ marginBottom: 14, justifyContent: "space-between" }}>
            <span style={{ color: "var(--muted)", fontSize: "0.88rem" }}>
              ดาวน์โหลดไบนารีสำหรับแจกเครื่อง client
            </span>
            <AgentDownload compact />
          </div>
          {tokenFlash && (
            <div className="alert warn">
              agent token (คัดลอกครั้งเดียว):
              <div className="token-box">{tokenFlash}</div>
              <div className="row" style={{ marginTop: 10 }}>
                <AgentDownload compact />
              </div>
            </div>
          )}
          <table className="data-table">
            <thead>
              <tr>
                <th>ชื่อ</th>
                <th>ผู้ใช้</th>
                <th>host</th>
                <th>OS</th>
                <th>สถานะ</th>
                <th>server</th>
                <th></th>
              </tr>
            </thead>
            <tbody>
              {agents.map((a) => (
                <tr key={a.id}>
                  <td>{a.name}</td>
                  <td>{a.username}</td>
                  <td className="mono">{a.hostname || "—"}</td>
                  <td>{a.os || "—"}</td>
                  <td>
                    <span className={`badge ${statusTone(a.status)}`}>{a.status}</span>
                    {!a.local_enabled && (
                      <span className="badge warn" style={{ marginLeft: 6 }}>
                        local off
                      </span>
                    )}
                  </td>
                  <td>
                    {isAdmin && (
                      <button
                        className="btn ghost"
                        type="button"
                        onClick={() => toggleAgent(a.id, !a.enabled)}
                      >
                        {a.enabled ? "เปิด" : "ปิด"}
                      </button>
                    )}
                  </td>
                  <td>
                    {isAdmin && (
                      <button
                        className="btn"
                        type="button"
                        disabled={!a.enabled}
                        onClick={() => triggerRunNow(a.id)}
                      >
                        <Icon icon={faPlay} /> Run now
                      </button>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          {isAdmin && (
            <form className="row" style={{ marginTop: 14 }} onSubmit={createAgent}>
              <div className="field">
                <label>ผู้ใช้</label>
                <select
                  value={agentForm.user_id}
                  onChange={(e) => setAgentForm({ ...agentForm, user_id: e.target.value })}
                >
                  {users.map((u) => (
                    <option key={u.id} value={u.id}>
                      {u.username}
                    </option>
                  ))}
                </select>
              </div>
              <div className="field">
                <label>ชื่อ agent</label>
                <input
                  value={agentForm.name}
                  onChange={(e) => setAgentForm({ ...agentForm, name: e.target.value })}
                  placeholder="laptop-01"
                  required
                />
              </div>
              <div className="field">
                <label>policy</label>
                <select
                  value={agentForm.policy_id}
                  onChange={(e) => setAgentForm({ ...agentForm, policy_id: e.target.value })}
                >
                  <option value="">—</option>
                  {policies.map((p) => (
                    <option key={p.id} value={p.id}>
                      {p.name}
                    </option>
                  ))}
                </select>
              </div>
              <button className="btn" type="submit">
                <Icon icon={faPlus} /> สร้าง agent + token
              </button>
            </form>
          )}
        </section>
      )}

      {tab === "users" && (
        <section className="panel" role="tabpanel">
          <h2 className="section-title">
            <Icon icon={faUsers} /> ผู้ใช้ (local)
          </h2>
          <p className="section-lead">LDAP users จะถูกสร้างอัตโนมัติเมื่อล็อกอินครั้งแรก</p>
          <table className="data-table">
            <thead>
              <tr>
                <th>username</th>
                <th>role</th>
                <th>provider</th>
                <th>enabled</th>
                <th></th>
              </tr>
            </thead>
            <tbody>
              {users.map((u) => (
                <tr key={u.id}>
                  <td>{u.username}</td>
                  <td>
                    <span className={`badge ${u.role === "admin" ? "ok" : "muted"}`}>
                      {u.role}
                    </span>
                  </td>
                  <td>{u.auth_provider}</td>
                  <td>
                    <span className={`badge ${u.enabled ? "ok" : "bad"}`}>
                      {u.enabled ? "yes" : "no"}
                    </span>
                  </td>
                  <td>
                    {isAdmin && (
                      <button
                        className="btn ghost"
                        type="button"
                        onClick={() => setUserQuota(u.id)}
                      >
                        ตั้งโควต้า
                      </button>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          {isAdmin && (
            <form className="row" style={{ marginTop: 14 }} onSubmit={createUser}>
              <div className="field">
                <label>username</label>
                <input
                  value={newUser.username}
                  onChange={(e) => setNewUser({ ...newUser, username: e.target.value })}
                  required
                />
              </div>
              <div className="field">
                <label>password</label>
                <input
                  type="password"
                  value={newUser.password}
                  onChange={(e) => setNewUser({ ...newUser, password: e.target.value })}
                  required
                />
              </div>
              <div className="field">
                <label>role</label>
                <select
                  value={newUser.role}
                  onChange={(e) => setNewUser({ ...newUser, role: e.target.value })}
                >
                  <option value="user">user</option>
                  <option value="admin">admin</option>
                  <option value="auditor">auditor</option>
                </select>
              </div>
              <button className="btn" type="submit">
                <Icon icon={faUserPlus} /> สร้างผู้ใช้
              </button>
            </form>
          )}
        </section>
      )}

      {tab === "audit" && (
        <section className="panel" role="tabpanel">
          <h2 className="section-title">
            <Icon icon={faClipboardList} /> Audit log
          </h2>
          <p className="section-lead">เหตุการณ์ล่าสุด 200 รายการ · login / download / GC / agents</p>
          {audit.length === 0 ? (
            <p className="empty">ยังไม่มีบันทึก</p>
          ) : (
            <table className="data-table">
              <thead>
                <tr>
                  <th>เวลา</th>
                  <th>ผู้กระทำ</th>
                  <th>action</th>
                  <th>detail</th>
                </tr>
              </thead>
              <tbody>
                {audit.map((a) => (
                  <tr key={a.id}>
                    <td className="mono">{new Date(a.created_at).toLocaleString()}</td>
                    <td>{a.actor_username || "—"}</td>
                    <td className="mono">{a.action}</td>
                    <td className="mono" style={{ fontSize: "0.75rem", maxWidth: 360 }}>
                      {JSON.stringify(a.detail)}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </section>
      )}
    </div>
  );
}
