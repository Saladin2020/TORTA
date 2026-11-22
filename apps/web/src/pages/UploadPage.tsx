import { FormEvent, useState } from "react";
import { useNavigate } from "react-router-dom";
import { getToken } from "../api";

export default function UploadPage() {
  const [files, setFiles] = useState<FileList | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [msg, setMsg] = useState("");
  const nav = useNavigate();

  async function submit(e: FormEvent) {
    e.preventDefault();
    if (!files || files.length === 0) return;
    setBusy(true);
    setError("");
    setMsg("");
    try {
      const fd = new FormData();
      Array.from(files).forEach((f) => fd.append("files", f));
      const token = getToken();
      const res = await fetch("/api/portal/upload", {
        method: "POST",
        headers: token ? { Authorization: `Bearer ${token}` } : {},
        body: fd,
      });
      if (!res.ok) throw new Error(await res.text());
      const data = await res.json();
      setMsg(`อัปโหลดสำเร็จ ${data.file_count} ไฟล์`);
      nav(`/portal/${data.snapshot_id}`);
    } catch (err) {
      setError(err instanceof Error ? err.message : "upload failed");
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="panel">
      <h2>อัปโหลดเฉพาะเจาะจง</h2>
      <p className="lead">
        เลือกไฟล์จากเครื่องนี้ ส่งเข้า shared Dice store ขององค์กร (dedup ร่วมกับ agent)
      </p>
      <form onSubmit={submit} className="grid">
        <div className="field">
          <label>ไฟล์</label>
          <input
            type="file"
            multiple
            onChange={(e) => setFiles(e.target.files)}
          />
        </div>
        {error && <p className="error">{error}</p>}
        {msg && <p>{msg}</p>}
        <div className="row">
          <button className="btn" disabled={busy || !files?.length} type="submit">
            {busy ? "กำลังอัปโหลด…" : "อัปโหลด"}
          </button>
        </div>
      </form>
    </section>
  );
}
