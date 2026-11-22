import { useState, type ReactNode } from "react";
import { Link, useLocation } from "react-router-dom";
import {
  faBars,
  faCloudArrowUp,
  faDatabase,
  faGear,
  faRightFromBracket,
  faUser,
  faXmark,
} from "@fortawesome/free-solid-svg-icons";
import type { Me } from "../api";
import Icon from "./Icon";

type Props = {
  me: Me;
  onLogout: () => void;
  children: ReactNode;
};

export default function AppShell({ me, onLogout, children }: Props) {
  const loc = useLocation();
  const [open, setOpen] = useState(false);

  const links = [
    { to: "/files", label: "ไฟล์", icon: faCloudArrowUp, match: (p: string) => p.startsWith("/files") },
    {
      to: "/store",
      label: "Dice store",
      icon: faDatabase,
      match: (p: string) => p.startsWith("/store") || p.startsWith("/portal"),
    },
    {
      to: "/account",
      label: "บัญชี",
      icon: faUser,
      match: (p: string) => p.startsWith("/account"),
    },
    ...(me.role === "admin" || me.role === "auditor"
      ? [
          {
            to: "/admin",
            label: "จัดการระบบ",
            icon: faGear,
            match: (p: string) => p.startsWith("/admin"),
          },
        ]
      : []),
  ];

  return (
    <div className="app-shell">
      {open && <div className="scrim" onClick={() => setOpen(false)} role="presentation" />}

      <aside className={`sidebar ${open ? "open" : ""}`}>
        <div className="sidebar-brand">
          <img
            className="sidebar-logo"
            src="/logo.jpg"
            alt="TORTA Dice"
            width={28}
            height={28}
          />
          <div>
            <strong>TORTA Dice</strong>
            <span>องค์กร · backup</span>
          </div>
        </div>

        <nav className="sidebar-nav">
          {links.map((l) => (
            <Link
              key={l.to}
              to={l.to}
              className={l.match(loc.pathname) ? "active" : ""}
              onClick={() => setOpen(false)}
            >
              <Icon icon={l.icon} />
              {l.label}
            </Link>
          ))}
        </nav>

        <div className="sidebar-footer">
          <div className="sidebar-user">
            เข้าสู่ระบบเป็น <strong>{me.username}</strong>
            {me.role === "admin" ? " · admin" : ""}
          </div>
          <button type="button" className="sidebar-logout" onClick={onLogout}>
            <Icon icon={faRightFromBracket} />
            ออกจากระบบ
          </button>
        </div>
      </aside>

      <div>
        <div className="mobile-top">
          <button
            type="button"
            className="btn ghost"
            aria-label={open ? "ปิดเมนู" : "เปิดเมนู"}
            onClick={() => setOpen((v) => !v)}
          >
            <Icon icon={open ? faXmark : faBars} />
          </button>
          <strong>TORTA Dice</strong>
          <span className="mono" style={{ color: "var(--muted)", fontSize: "0.8rem" }}>
            {me.username}
          </span>
        </div>
        <main className="app-main">{children}</main>
      </div>
    </div>
  );
}
