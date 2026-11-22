import { Link, useParams } from "react-router-dom";
import { faArrowLeft, faCloudArrowUp, faFileZipper } from "@fortawesome/free-solid-svg-icons";
import { getToken } from "../api";
import StoreBrowser from "../components/StoreBrowser";
import Icon from "../components/Icon";

export default function SnapshotPage() {
  const { id } = useParams();
  if (!id) return null;

  async function downloadZip() {
    const token = getToken();
    const res = await fetch(`/api/portal/snapshots/${id}/zip`, {
      headers: token ? { Authorization: `Bearer ${token}` } : {},
    });
    if (!res.ok) {
      window.alert(await res.text());
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

  return (
    <div className="stack">
      <header className="page-header">
        <div>
          <h1>Snapshot</h1>
          <p className="mono" style={{ wordBreak: "break-all" }}>
            {id}
          </p>
        </div>
        <div className="page-actions">
          <Link className="btn ghost" to="/store">
            <Icon icon={faArrowLeft} />
            Dice store
          </Link>
          <Link className="btn ghost" to="/files">
            <Icon icon={faCloudArrowUp} />
            ไฟล์
          </Link>
          <button className="btn" type="button" onClick={downloadZip}>
            <Icon icon={faFileZipper} />
            ดาวน์โหลด ZIP
          </button>
        </div>
      </header>
      <StoreBrowser
        loadUrl={`/api/portal/snapshots/${id}/files`}
        title="ไฟล์ใน snapshot"
        subtitle="รายการ path ตาม snapshot นี้"
        expandAllByDefault
        browseOnly
      />
    </div>
  );
}
