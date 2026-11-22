import { useEffect, useMemo, useRef, useState } from "react";
import { Link } from "react-router-dom";
import {
  faArrowsRotate,
  faChevronDown,
  faChevronRight,
  faCloudArrowUp,
  faDownload,
  faFile,
  faFileZipper,
  faHistory,
  faInbox,
  faMagnifyingGlass,
  faTrash,
} from "@fortawesome/free-solid-svg-icons";
import { api, getToken, Snapshot, SnapshotFile } from "../api";
import AgentDownload from "../components/AgentDownload";
import Icon from "../components/Icon";
import { fmtBytes, statusTone } from "../lib/format";

export default function FilesPage() {
  const [files, setFiles] = useState<SnapshotFile[]>([]);
  const [snapshots, setSnapshots] = useState<Snapshot[]>([]);
  const [error, setError] = useState("");
  const [msg, setMsg] = useState("");
  const [busy, setBusy] = useState(false);
  const [progress, setProgress] = useState<number | null>(null);
  const [pick, setPick] = useState<File[]>([]);
  const [dragOver, setDragOver] = useState(false);
  const [query, setQuery] = useState("");
  const [showHistory, setShowHistory] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);

  async function refresh() {
    try {
      const [f, s] = await Promise.all([
        api<SnapshotFile[]>("/api/portal/files"),
        api<Snapshot[]>("/api/portal/snapshots"),
      ]);
      setFiles(f);
      setSnapshots(s);
      setError("");
    } catch (e) {
      setError(e instanceof Error ? e.message : "load failed");
    }
  }

  useEffect(() => {
    refresh();
  }, []);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return files;
    return files.filter((f) => f.relative_path.toLowerCase().includes(q));
  }, [files, query]);

  const totalSize = useMemo(
    () => files.reduce((acc, f) => acc + f.size_bytes, 0),
    [files],
  );

  function takeFiles(list: FileList | File[] | null) {
    if (!list) return;
    setPick(Array.from(list));
  }

  async function download(f: SnapshotFile) {
    if (!f.snapshot_id) {
      setError("ไม่พบ snapshot ของไฟล์นี้");
      return;
    }
    setError("");
    const token = getToken();
    const res = await fetch(
      `/api/portal/snapshots/${f.snapshot_id}/files/${f.id}/download`,
      { headers: token ? { Authorization: `Bearer ${token}` } : {} },
    );
    if (!res.ok) {
      setError(await res.text());
      return;
    }
    const blob = await res.blob();
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = f.relative_path.split("/").pop() || "file";
    a.click();
    URL.revokeObjectURL(url);
  }

  function upload() {
    if (!pick.length) return;
    setBusy(true);
    setError("");
    setMsg("");
    setProgress(0);

    const fd = new FormData();
    pick.forEach((f) => fd.append("files", f));
    const token = getToken();
    const xhr = new XMLHttpRequest();
    xhr.open("POST", "/api/portal/upload");
    if (token) xhr.setRequestHeader("Authorization", `Bearer ${token}`);
    xhr.upload.onprogress = (ev) => {
      if (ev.lengthComputable) {
        setProgress(Math.round((ev.loaded / ev.total) * 100));
      }
    };
    xhr.onload = async () => {
      setBusy(false);
      setProgress(null);
      if (xhr.status >= 200 && xhr.status < 300) {
        try {
          const data = JSON.parse(xhr.responseText);
          setMsg(`อัปโหลดสำเร็จ ${data.file_count} ไฟล์ · เนื้อหาซ้ำจะ dedup อัตโนมัติ`);
        } catch {
          setMsg("อัปโหลดสำเร็จ");
        }
        setPick([]);
        if (inputRef.current) inputRef.current.value = "";
        await refresh();
      } else {
        setError(xhr.responseText || `upload failed (${xhr.status})`);
      }
    };
    xhr.onerror = () => {
      setBusy(false);
      setProgress(null);
      setError("network error");
    };
    xhr.send(fd);
  }

  async function downloadZip(id: string) {
    const token = getToken();
    const res = await fetch(`/api/portal/snapshots/${id}/zip`, {
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
    a.download = `snapshot-${id}.zip`;
    a.click();
    URL.revokeObjectURL(url);
  }

  async function trashSnapshot(id: string) {
    if (!window.confirm("ย้าย snapshot นี้ไปถังขยะ? (ลบถาวรใน ~7 วัน)")) return;
    try {
      await api(`/api/portal/snapshots/${id}/trash`, { method: "POST" });
      await refresh();
    } catch (e) {
      setError(e instanceof Error ? e.message : "trash failed");
    }
  }

  return (
    <div className="stack">
      <header className="page-header">
        <div>
          <h1>ไฟล์</h1>
          <p>อัปโหลดและดาวน์โหลดไฟล์ของคุณ · รายละเอียด chunk ดูที่ Dice store</p>
        </div>
        <div className="page-actions">
          <button className="btn ghost" type="button" onClick={refresh}>
            <Icon icon={faArrowsRotate} />
            รีเฟรช
          </button>
          <Link className="btn ghost" to="/store">
            Dice store
          </Link>
        </div>
      </header>

      <AgentDownload />

      <section className="panel">
        <h2 className="section-title">
          <Icon icon={faCloudArrowUp} /> อัปโหลด
        </h2>
        <p className="section-lead">ลากไฟล์มาวาง หรือคลิกเพื่อเลือก — รองรับหลายไฟล์</p>

        <div
          className={`dropzone ${dragOver ? "active" : ""}`}
          onClick={() => inputRef.current?.click()}
          onDragOver={(e) => {
            e.preventDefault();
            setDragOver(true);
          }}
          onDragLeave={() => setDragOver(false)}
          onDrop={(e) => {
            e.preventDefault();
            setDragOver(false);
            takeFiles(e.dataTransfer.files);
          }}
          role="button"
          tabIndex={0}
          onKeyDown={(e) => {
            if (e.key === "Enter" || e.key === " ") inputRef.current?.click();
          }}
        >
          <div className="dz-icon">
            <Icon icon={faInbox} />
          </div>
          <strong>วางไฟล์ที่นี่</strong>
          <span>หรือคลิกเพื่อเลือกจากเครื่อง</span>
          <input
            ref={inputRef}
            type="file"
            multiple
            onChange={(e) => takeFiles(e.target.files)}
          />
        </div>

        {pick.length > 0 && (
          <div className="chip-row">
            {pick.slice(0, 8).map((f) => (
              <span key={`${f.name}-${f.size}`} className="chip">
                <Icon icon={faFile} /> {f.name}
              </span>
            ))}
            {pick.length > 8 && <span className="chip">+{pick.length - 8} อีก</span>}
          </div>
        )}

        {progress != null && (
          <div style={{ marginTop: 12 }}>
            <div
              style={{
                height: 8,
                background: "var(--line)",
                borderRadius: 999,
                overflow: "hidden",
              }}
            >
              <div
                style={{
                  width: `${progress}%`,
                  height: "100%",
                  background: "#000",
                  transition: "width 0.15s",
                }}
              />
            </div>
            <div className="mono" style={{ marginTop: 6, fontSize: "0.82rem", color: "var(--muted)" }}>
              {progress}%
            </div>
          </div>
        )}

        <div className="row" style={{ marginTop: 14 }}>
          <button
            className="btn"
            type="button"
            disabled={busy || !pick.length}
            onClick={upload}
          >
            <Icon icon={faCloudArrowUp} />
            {busy ? "กำลังอัปโหลด…" : `อัปโหลด${pick.length ? ` (${pick.length})` : ""}`}
          </button>
          {pick.length > 0 && (
            <button
              className="btn ghost"
              type="button"
              onClick={() => {
                setPick([]);
                if (inputRef.current) inputRef.current.value = "";
              }}
            >
              ล้างรายการ
            </button>
          )}
        </div>

        {msg && (
          <div className="alert ok" style={{ marginTop: 12 }}>
            {msg}
          </div>
        )}
        {error && (
          <div className="alert error" style={{ marginTop: 12 }}>
            {error}
          </div>
        )}
      </section>

      <section className="panel">
        <div className="row" style={{ justifyContent: "space-between", marginBottom: 12 }}>
          <div>
            <h2 className="section-title">
              <Icon icon={faDownload} /> ดาวน์โหลด
            </h2>
            <p className="section-lead" style={{ marginBottom: 0 }}>
              ไฟล์ล่าสุด · {files.length} รายการ · {fmtBytes(totalSize)}
            </p>
          </div>
          <div className="search-bar">
            <Icon icon={faMagnifyingGlass} />
            <input
              placeholder="ค้นหาชื่อไฟล์…"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
            />
          </div>
        </div>

        {filtered.length === 0 ? (
          <div className="empty">
            <div className="empty-icon">
              <Icon icon={faFile} />
            </div>
            {files.length === 0
              ? "ยังไม่มีไฟล์ — อัปโหลดด้านบน หรือรอ agent สำรอง"
              : "ไม่พบไฟล์ที่ตรงกับคำค้น"}
          </div>
        ) : (
          <table className="files-table">
            <thead>
              <tr>
                <th>ชื่อไฟล์</th>
                <th>ขนาด</th>
                <th>อัปเดตล่าสุด</th>
                <th></th>
              </tr>
            </thead>
            <tbody>
              {filtered.map((f) => (
                <tr key={f.id}>
                  <td>
                    <div className="file-name">
                      <Icon icon={faFile} /> {f.relative_path.split("/").pop()}
                    </div>
                    <div className="file-path mono">{f.relative_path}</div>
                  </td>
                  <td className="mono">{fmtBytes(f.size_bytes)}</td>
                  <td className="mono">
                    {f.backed_up_at ? new Date(f.backed_up_at).toLocaleString() : "—"}
                  </td>
                  <td>
                    <button className="btn" type="button" onClick={() => download(f)}>
                      <Icon icon={faDownload} />
                      ดาวน์โหลด
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </section>

      <section className="panel">
        <button
          type="button"
          className="history-toggle"
          onClick={() => setShowHistory((v) => !v)}
        >
          <Icon icon={showHistory ? faChevronDown : faChevronRight} />
          <Icon icon={faHistory} />
          ประวัติ snapshot ({snapshots.length})
        </button>
        {showHistory &&
          (snapshots.length === 0 ? (
            <p className="empty">ยังไม่มีประวัติ</p>
          ) : (
            <table className="files-table" style={{ marginTop: 12 }}>
              <thead>
                <tr>
                  <th>เวลา</th>
                  <th>ป้าย</th>
                  <th>ที่มา</th>
                  <th>สถานะ</th>
                  <th>ไฟล์</th>
                  <th>ขนาด</th>
                  <th></th>
                </tr>
              </thead>
              <tbody>
                {snapshots.map((s) => (
                  <tr key={s.id}>
                    <td className="mono">{new Date(s.started_at).toLocaleString()}</td>
                    <td>{s.label || "—"}</td>
                    <td>{s.source}</td>
                    <td>
                      <span className={`badge ${statusTone(s.status)}`}>{s.status}</span>
                    </td>
                    <td>{s.file_count}</td>
                    <td className="mono">{fmtBytes(s.raw_bytes)}</td>
                    <td>
                      <div className="row" style={{ gap: 6 }}>
                        <Link to={`/store/snapshot/${s.id}`}>ดู</Link>
                        <button
                          className="btn ghost"
                          type="button"
                          title="ดาวน์โหลด ZIP"
                          onClick={() => downloadZip(s.id)}
                        >
                          <Icon icon={faFileZipper} />
                        </button>
                        <button
                          className="btn ghost"
                          type="button"
                          title="ย้ายไปถังขยะ"
                          onClick={() => trashSnapshot(s.id)}
                        >
                          <Icon icon={faTrash} />
                        </button>
                      </div>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          ))}
      </section>
    </div>
  );
}
