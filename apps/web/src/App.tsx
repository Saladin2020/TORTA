import { useEffect, useState } from "react";
import { Navigate, Route, Routes, useNavigate } from "react-router-dom";
import { faSpinner } from "@fortawesome/free-solid-svg-icons";
import { api, getToken, Me, setToken } from "./api";
import AppShell from "./components/AppShell";
import Icon from "./components/Icon";
import AccountPage from "./pages/AccountPage";
import AdminPage from "./pages/AdminPage";
import FilesPage from "./pages/FilesPage";
import LoginPage from "./pages/LoginPage";
import PortalPage from "./pages/PortalPage";
import SnapshotPage from "./pages/SnapshotPage";

export default function App() {
  const [me, setMe] = useState<Me | null>(null);
  const [loading, setLoading] = useState(true);
  const nav = useNavigate();

  useEffect(() => {
    const token = getToken();
    if (!token) {
      setLoading(false);
      return;
    }
    api<Me>("/api/auth/me")
      .then(setMe)
      .catch(() => setToken(null))
      .finally(() => setLoading(false));
  }, []);

  function logout() {
    setToken(null);
    setMe(null);
    nav("/login");
  }

  if (loading) {
    return (
      <div className="login-wrap">
        <div className="panel login-panel" style={{ textAlign: "center" }}>
          <Icon icon={faSpinner} className="fa-spin" />
          <p className="sub" style={{ marginTop: 12 }}>
            กำลังโหลด…
          </p>
        </div>
      </div>
    );
  }

  if (!me) {
    return (
      <Routes>
        <Route path="/login" element={<LoginPage onLogin={setMe} />} />
        <Route path="*" element={<Navigate to="/login" replace />} />
      </Routes>
    );
  }

  if (me.must_change_password) {
    return (
      <AppShell me={me} onLogout={logout}>
        <AccountPage
          force
          onDone={() => setMe({ ...me, must_change_password: false })}
        />
      </AppShell>
    );
  }

  return (
    <AppShell me={me} onLogout={logout}>
      <Routes>
        <Route path="/" element={<Navigate to="/files" replace />} />
        <Route path="/files" element={<FilesPage />} />
        <Route path="/store" element={<PortalPage />} />
        <Route path="/store/snapshot/:id" element={<SnapshotPage />} />
        <Route path="/account" element={<AccountPage />} />
        <Route path="/portal" element={<Navigate to="/store" replace />} />
        <Route path="/portal/:id" element={<SnapshotPage />} />
        <Route path="/upload" element={<Navigate to="/files" replace />} />
        <Route
          path="/admin"
          element={
            me.role === "admin" || me.role === "auditor" ? (
              <AdminPage role={me.role} />
            ) : (
              <Navigate to="/files" replace />
            )
          }
        />
        <Route path="*" element={<Navigate to="/files" replace />} />
      </Routes>
    </AppShell>
  );
}
