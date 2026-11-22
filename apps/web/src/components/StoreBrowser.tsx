import { Fragment, useEffect, useMemo, useState } from "react";
import { Link } from "react-router-dom";
import {
  faArrowsRotate,
  faChartSimple,
  faChevronDown,
  faChevronRight,
  faCloudArrowUp,
  faDownload,
  faFolderOpen,
  faLink,
  faListCheck,
  faShieldHalved,
  faXmark,
} from "@fortawesome/free-solid-svg-icons";
import { api, getToken, SnapshotFile } from "../api";
import { fmtBytes, shortHash } from "../lib/format";
import Icon from "./Icon";

type Props = {
  loadUrl: string;
  title?: string;
  subtitle?: string;
  expandAllByDefault?: boolean;
  browseOnly?: boolean;
};

export default function StoreBrowser({
  loadUrl,
  title = "Dice store",
  subtitle,
  expandAllByDefault = true,
  browseOnly = false,
}: Props) {
  const [files, setFiles] = useState<SnapshotFile[]>([]);
  const [error, setError] = useState("");
  const [status, setStatus] = useState("");
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState(false);
  const [query, setQuery] = useState("");
  const [relatedOf, setRelatedOf] = useState<SnapshotFile | null>(null);

  async function load() {
    setBusy(true);
    setError("");
    try {
      const rows = await api<SnapshotFile[]>(loadUrl);
      setFiles(rows);
      if (expandAllByDefault) {
        setExpanded(
          new Set(rows.filter((f) => (f.chunks?.length ?? 0) > 0).map((f) => f.id)),
        );
      }
      setStatus(`${rows.length} files · 0 selected`);
    } catch (e) {
      setError(e instanceof Error ? e.message : "load failed");
    } finally {
      setBusy(false);
    }
  }

  useEffect(() => {
    load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [loadUrl]);

  useEffect(() => {
    setStatus(`${files.length} files · ${selected.size} selected`);
  }, [files.length, selected.size]);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return files;
    return files.filter(
      (f) =>
        f.relative_path.toLowerCase().includes(q) ||
        f.content_hash.toLowerCase().includes(q) ||
        (f.aliases ?? []).some((a) => a.toLowerCase().includes(q)),
    );
  }, [files, query]);

  const totalSize = useMemo(
    () => files.reduce((acc, f) => acc + f.size_bytes, 0),
    [files],
  );

  function toggleSelect(fileId: string) {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(fileId)) next.delete(fileId);
      else next.add(fileId);
      return next;
    });
  }

  function selectAll() {
    setSelected(new Set(filtered.map((f) => f.id)));
  }

  function selectNone() {
    setSelected(new Set());
  }

  function toggleExpand(fileId: string) {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(fileId)) next.delete(fileId);
      else next.add(fileId);
      return next;
    });
  }

  function expandAll() {
    setExpanded(
      new Set(filtered.filter((f) => (f.chunks?.length ?? 0) > 0).map((f) => f.id)),
    );
  }

  function collapseAll() {
    setExpanded(new Set());
  }

  async function downloadOne(f: SnapshotFile) {
    const snapshotId = f.snapshot_id;
    if (!snapshotId) {
      setError("missing snapshot_id");
      return;
    }
    const token = getToken();
    const res = await fetch(
      `/api/portal/snapshots/${snapshotId}/files/${f.id}/download`,
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

  async function openSelected() {
    const targets = filtered.filter((f) => selected.has(f.id));
    if (targets.length === 0) {
      setError("เลือกไฟล์ก่อน");
      return;
    }
    setBusy(true);
    setError("");
    try {
      for (const f of targets) {
        await downloadOne(f);
      }
      setStatus(`opened ${targets.length} file(s)`);
    } finally {
      setBusy(false);
    }
  }

  async function verifySelected() {
    const targets = filtered.filter((f) => selected.has(f.id));
    if (targets.length === 0) {
      setError("เลือกไฟล์ก่อน");
      return;
    }
    setBusy(true);
    setError("");
    let ok = 0;
    let fail = 0;
    try {
      for (const f of targets) {
        if (!f.snapshot_id) {
          fail += 1;
          continue;
        }
        try {
          await api(`/api/portal/snapshots/${f.snapshot_id}/files/${f.id}/verify`, {
            method: "POST",
          });
          ok += 1;
        } catch {
          fail += 1;
        }
      }
      setStatus(
        fail === 0
          ? `verify ok · ${ok} file(s) reassembled + hashed`
          : `verify · ${ok} ok · ${fail} failed`,
      );
      if (fail > 0) setError("บางไฟล์ verify ไม่ผ่าน — ดู audit / ลองทีละไฟล์");
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="panel browser-panel">
      <div className="browser-panel-head">
        <div className="row" style={{ justifyContent: "space-between" }}>
          <div>
            <h2 className="section-title">
              <Icon icon={faFolderOpen} /> {title}
            </h2>
            <p className="section-lead" style={{ marginBottom: 0 }}>
              {subtitle ?? `Chunk storage · ${fmtBytes(totalSize)}`}
            </p>
          </div>
          {browseOnly && (
            <Link className="btn" to="/files">
              <Icon icon={faCloudArrowUp} />
              ไปหน้าไฟล์
            </Link>
          )}
        </div>
      </div>

      <div className="browser-toolbar">
        <button type="button" className="tb" onClick={load} disabled={busy}>
          <Icon icon={faArrowsRotate} /> Refresh
        </button>
        {!browseOnly && (
          <button type="button" className="tb" onClick={openSelected} disabled={busy}>
            <Icon icon={faDownload} /> Open
          </button>
        )}
        <span className="tb-sep" />
        <button type="button" className="tb" onClick={selectAll}>
          <Icon icon={faListCheck} /> All
        </button>
        <button type="button" className="tb" onClick={selectNone}>
          None
        </button>
        <span className="tb-sep" />
        <button type="button" className="tb" onClick={expandAll}>
          Expand
        </button>
        <button type="button" className="tb" onClick={collapseAll}>
          Collapse
        </button>
        <span className="tb-sep" />
        <button type="button" className="tb" onClick={verifySelected}>
          <Icon icon={faShieldHalved} /> Verify
        </button>
        <button
          type="button"
          className="tb"
          onClick={() =>
            setStatus(`stats · ${files.length} files · ${fmtBytes(totalSize)}`)
          }
        >
          <Icon icon={faChartSimple} /> Stats
        </button>
      </div>

      <div className="row" style={{ padding: "0 18px", marginTop: 10 }}>
        <div className="search-bar" style={{ flex: 1 }}>
          <input
            placeholder="ค้นหา path / hash / alias…"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
        </div>
      </div>

      <div className="browser-status">
        {status || (busy ? "loading…" : "")}
        {query.trim() ? ` · แสดง ${filtered.length}/${files.length}` : ""}
      </div>
      {error && (
        <div className="alert error" style={{ margin: "0 18px 10px" }}>
          {error}
        </div>
      )}

      {filtered.length === 0 ? (
        <div className="empty">
          <div className="empty-icon">
            <Icon icon={faFolderOpen} />
          </div>
          {files.length === 0
            ? "ยังไม่มีไฟล์ — อัปโหลดผ่านเว็บ หรือรอ agent สำรองข้อมูล"
            : "ไม่พบรายการที่ตรงกับคำค้น"}
        </div>
      ) : (
        <div className="browser-table-wrap">
          <table className="browser-table">
            <thead>
              <tr>
                <th className="col-check" />
                <th className="col-twist" />
                <th>Name</th>
                <th className="col-related">Related</th>
                <th className="col-size">Size</th>
              </tr>
            </thead>
            <tbody>
              {filtered.map((f) => {
                const chunks = f.chunks ?? [];
                const open = expanded.has(f.id);
                const checked = selected.has(f.id);
                const relatedCount = 1 + (f.aliases?.length ?? 0);
                return (
                  <Fragment key={f.id}>
                    <tr
                      className={checked ? "row-selected" : undefined}
                      onDoubleClick={() => {
                        if (!browseOnly) downloadOne(f);
                      }}
                    >
                      <td className="col-check">
                        <input
                          type="checkbox"
                          checked={checked}
                          onChange={() => toggleSelect(f.id)}
                          aria-label={`select ${f.relative_path}`}
                        />
                      </td>
                      <td className="col-twist">
                        <button
                          type="button"
                          className={`twist ${open ? "open" : ""}`}
                          onClick={() => toggleExpand(f.id)}
                          aria-label={open ? "collapse" : "expand"}
                          disabled={chunks.length === 0}
                        >
                          {chunks.length === 0 ? (
                            ""
                          ) : (
                            <Icon icon={open ? faChevronDown : faChevronRight} />
                          )}
                        </button>
                      </td>
                      <td className="col-name" title={f.content_hash}>
                        <button
                          type="button"
                          className="name-link"
                          onClick={() => toggleExpand(f.id)}
                        >
                          {f.relative_path}
                        </button>
                      </td>
                      <td className="col-related">
                        {relatedCount > 1 ? (
                          <button
                            type="button"
                            className="btn ghost related-btn"
                            onClick={() => setRelatedOf(f)}
                          >
                            <Icon icon={faLink} />
                            ดูที่เกี่ยวข้อง ({relatedCount})
                          </button>
                        ) : (
                          <span className="muted-dash">—</span>
                        )}
                      </td>
                      <td className="col-size mono">{fmtBytes(f.size_bytes)}</td>
                    </tr>
                    {open &&
                      chunks.map((c) => (
                        <tr key={`${f.id}-${c.index}`} className="chunk-row">
                          <td className="col-check" />
                          <td className="col-twist" />
                          <td className="col-name mono chunk-name" colSpan={2}>
                            <span className="chunk-idx">
                              #{String(c.index).padStart(4, "0")}
                            </span>{" "}
                            {shortHash(c.hash)}
                          </td>
                          <td className="col-size mono">{fmtBytes(c.raw_size)}</td>
                        </tr>
                      ))}
                  </Fragment>
                );
              })}
            </tbody>
          </table>
        </div>
      )}

      <div className="browser-footer mono">
        Chunk 256.0 KB / 1.00 MB / 4.00 MB · zstd · shared org dedup
      </div>

      {relatedOf && (
        <div
          className="modal-backdrop"
          onClick={() => setRelatedOf(null)}
          role="presentation"
        >
          <div
            className="modal-card"
            onClick={(e) => e.stopPropagation()}
            role="dialog"
            aria-label="ไฟล์ที่เกี่ยวข้อง"
          >
            <div className="row" style={{ justifyContent: "space-between" }}>
              <div>
                <h3 style={{ margin: 0 }}>
                  <Icon icon={faLink} /> ไฟล์ที่เกี่ยวข้อง
                </h3>
                <p className="lead" style={{ margin: "6px 0 0" }}>
                  เนื้อหาเดียวกัน (dedup) · hash{" "}
                  <span className="mono">{shortHash(relatedOf.content_hash)}</span>
                </p>
              </div>
              <button
                type="button"
                className="btn ghost"
                onClick={() => setRelatedOf(null)}
              >
                <Icon icon={faXmark} />
                ปิด
              </button>
            </div>
            <ul className="related-list">
              <li className="mono">
                <strong>{relatedOf.relative_path}</strong>
                <span className="badge muted">หลัก</span>
              </li>
              {(relatedOf.aliases ?? []).map((a) => (
                <li key={a} className="mono">
                  {a}
                </li>
              ))}
            </ul>
          </div>
        </div>
      )}
    </section>
  );
}
