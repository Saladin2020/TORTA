import { useEffect, useState } from "react";
import { faDownload, faRobot } from "@fortawesome/free-solid-svg-icons";
import { api, getToken } from "../api";
import Icon from "./Icon";
import { fmtBytes } from "../lib/format";

type Item = {
  id: string;
  label: string;
  filename: string;
  available: boolean;
  size_bytes: number | null;
};

export default function AgentDownload({ compact = false }: { compact?: boolean }) {
  const [items, setItems] = useState<Item[]>([]);
  const [error, setError] = useState("");

  useEffect(() => {
    api<Item[]>("/api/portal/agent/downloads")
      .then(setItems)
      .catch((e) => setError(e instanceof Error ? e.message : "load failed"));
  }, []);

  async function download(id: string, filename: string) {
    setError("");
    const token = getToken();
    const res = await fetch(`/api/portal/agent/download/${id}`, {
      headers: token ? { Authorization: `Bearer ${token}` } : {},
    });
    if (!res.ok) {
      setError(await res.text());
      return;
    }
    const blob = await res.blob();
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = filename;
    a.click();
    URL.revokeObjectURL(url);
  }

  if (compact) {
    return (
      <div className="row" style={{ gap: 8 }}>
        {items
          .filter((i) => i.available && (i.id === "windows-x64" || i.id === "linux-x64"))
          .map((i) => (
            <button
              key={i.id}
              type="button"
              className="btn"
              onClick={() => download(i.id, i.filename)}
            >
              <Icon icon={faDownload} />
              {i.id === "windows-x64" ? "Windows" : "Linux"}
            </button>
          ))}
        {error && <span className="error">{error}</span>}
      </div>
    );
  }

  return (
    <section className="panel">
      <h2 className="section-title">
        <Icon icon={faRobot} /> ดาวน์โหลด Agent
      </h2>
      <p className="section-lead">
        ติดตั้งบนเครื่อง client แล้วใส่ token จาก Admin → Agents · รัน{" "}
        <span className="mono">dice-agent tray</span> — คลิกไอคอนถาดเพื่อเปิดแผง GUI
        (ซิงค์ / หยุด / ตั้งค่าเชื่อมต่อ)
      </p>
      {error && <div className="alert error">{error}</div>}
      <table className="data-table">
        <thead>
          <tr>
            <th>แพ็กเกจ</th>
            <th>ขนาด</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          {items.map((i) => (
            <tr key={i.id}>
              <td>{i.label}</td>
              <td className="mono">
                {i.available && i.size_bytes != null ? fmtBytes(i.size_bytes) : "—"}
              </td>
              <td>
                <button
                  type="button"
                  className="btn"
                  disabled={!i.available}
                  onClick={() => download(i.id, i.filename)}
                >
                  <Icon icon={faDownload} />
                  {i.available ? "ดาวน์โหลด" : "ยังไม่มีไฟล์"}
                </button>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      {items.every((i) => !i.available) && (
        <p className="empty" style={{ marginTop: 12 }}>
          ยังไม่มี binary — รัน <span className="mono">deploy/publish-agent.ps1</span> บนเครื่อง build
        </p>
      )}
    </section>
  );
}
