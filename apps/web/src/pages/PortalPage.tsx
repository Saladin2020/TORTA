import StoreBrowser from "../components/StoreBrowser";

/** Dice store — unique content objects (dedup view), not per-filename. */
export default function PortalPage() {
  return (
    <div className="stack">
      <header className="page-header">
        <div>
          <h1>Dice store</h1>
          <p>
            เนื้อหาที่ไม่ซ้ำในคลังองค์กร · ชื่อ path หลายอันที่เนื้อหาเดียวกันดูผ่าน Related
          </p>
        </div>
      </header>
      <StoreBrowser
        loadUrl="/api/portal/store"
        title="เนื้อหาในคลัง"
        subtitle="Unique by content hash · FastCDC + BLAKE3 + zstd"
        expandAllByDefault
        browseOnly
      />
    </div>
  );
}
