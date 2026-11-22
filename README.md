# TORTA Dice

ระบบสำรองข้อมูลองค์กรบน Ubuntu — ใช้เทคนิคเดียวกับ [Dice](https://github.com/Saladin2020/Dice): **FastCDC + BLAKE3 + zstd** พร้อม **dedup ร่วมทั้งองค์กร**

| ส่วน | เทคโนโลยี |
|------|-----------|
| `dice-core` | content-addressed chunk store |
| `dice-server` | Rust / Axum / PostgreSQL / local+LDAP auth |
| `dice-agent` | Windows + Linux agent (poll policy → push missing chunks) |
| `apps/web` | เว็บ admin + portal ผู้ใช้ |

## สิ่งที่ได้ในเฟสนี้

- Admin: ตั้ง storage path, policy/schedule, สร้าง agent token, เปิด/ปิด agent, จัดการ local users
- Agent: heartbeat, ดึงนโยบาย, backup ตาม cron, ส่งเฉพาะ chunk ที่ยังไม่มี
- ผู้ใช้: ดู snapshot ของตัวเอง, ดาวน์โหลดไฟล์, อัปโหลดเฉพาะเจาะจงผ่านเว็บ
- Auth: local account หรือ AD/LDAP (เลือกตอนล็อกอิน / auto)

## โครงสร้าง

```text
crates/dice-core     # FastCDC store engine
crates/dice-server   # control plane + shared store API
crates/dice-agent    # client agent
apps/web             # React UI
migrations/          # Postgres schema
deploy/              # systemd + nginx examples
```

## เริ่มต้นแบบเร็ว (dev)

### 1) Postgres

```bash
docker compose up -d
```

### 2) Server

```bash
cp dice-server.toml.example dice-server.toml
cargo run -p dice-server
```

บัญชีเริ่มต้น: `admin` / `admin123` (เปลี่ยนทันที)

### 3) Web UI

```bash
cd apps/web
npm install
npm run dev
```

เปิด http://127.0.0.1:5173 (proxy ไป API ที่ `:8080`)

### 4) Agent

1. Admin → สร้าง agent → คัดลอก token  
2. บนเครื่อง client:

```bash
cargo run -p dice-agent -- init
# แก้ dice-agent.toml: server_url + agent_token
cargo run -p dice-agent -- backup-now
# หรือรันยาวๆ
cargo run -p dice-agent -- run
```

Local kill-switch:

```bash
cargo run -p dice-agent -- set-enabled false
```

## Deploy บน Ubuntu

1. ติดตั้ง Postgres 16 + สร้าง DB `dice`
2. `cargo build --release -p dice-server -p dice-agent`
3. วาง binary ที่ `/opt/dice/`, ตั้ง `dice-server.toml`, `store_path` เช่น `/var/lib/dice/store`
4. `npm run build` ใน `apps/web` แล้วชี้ `web_root`
5. เปิด `deploy/dice-server.service` + reverse proxy จาก `deploy/nginx.conf.example`
6. แจก `dice-agent` ให้ Windows/Linux พร้อม token ต่อเครื่อง

### LDAP / AD

ใน `dice-server.toml`:

```toml
[ldap]
enabled = true
url = "ldap://dc.example.local:389"
bind_dn = "CN=BackupSvc,OU=Service,DC=example,DC=local"
bind_password = "..."
base_dn = "DC=example,DC=local"
user_filter = "(&(objectClass=user)(sAMAccountName={username}))"
username_attr = "sAMAccountName"
```

ผู้ใช้ LDAP จะถูก mirror เข้า Postgres เมื่อล็อกอินสำเร็จ (role เริ่มเป็น `user`)

## โมเดลข้อมูลสำคัญ

- **Chunks** อยู่บน disk store เดียวทั้งองค์กร → dedup ข้ามเครื่อง/คน
- **Snapshot + ownership** อยู่ใน Postgres → portal เห็นเฉพาะของตัวเอง
- Server กำหนด schedule/path; agent เป็นฝ่าย push (ผ่าน NAT/firewall ได้)

## ฟีเจอร์ operations ที่พร้อมใช้

- **Run now** จาก Admin → WebSocket push ทันที + fallback ผ่าน poll flag
- **Retention GC** ตาม `policies.retention_days` (รันทุกชั่วโมง + ปุ่มใน Admin)
- **Quota** ต่อ storage target และต่อ user
- **Encrypt-at-rest** ด้วย `store_key_hex` (ChaCha20-Poly1305) — ยัง dedup ทั้งองค์กรได้
- **Agent tray**: `dice-agent tray` (Backup now / Enable-Disable / Quit)
- **ติดตั้ง agent**: `deploy/install-agent.ps1` (Windows) · `deploy/install-agent.sh` (Linux)

### สร้าง store key

```bash
openssl rand -hex 32
# ใส่ใน dice-server.toml → store_key_hex
```

### Agent โหมด tray

```bash
cargo run -p dice-agent -- tray
```

## Production

ดูรายละเอียดใน [SECURITY.md](SECURITY.md)

```bash
# secrets
export DICE_PRODUCTION=true
export JWT_SECRET="$(openssl rand -base64 48)"
export DICE_STORE_KEY="$(openssl rand -hex 32)"
export DICE_CORS_ORIGINS="https://backup.example.com"
export DICE_BOOTSTRAP_ADMIN_PASSWORD='…long…'

# metadata DR (cron daily)
deploy/backup-metadata.sh

# compose Postgres โปร
docker compose -f deploy/docker-compose.prod.yml up -d
```

ฟีเจอร์โปรที่พร้อมใช้เพิ่มเติม: rate-limit login, เปลี่ยนรหัสผ่าน, verify จริง, ZIP restore, soft-delete, audit log, role `auditor`, webhook แจ้งเตือน, health ลึก, CI

## ขั้นตอนถัดไปที่แนะนำ

- Per-tenant / client-side encryption keys (แลกกับ dedup ข้ามคน)
- MSI / code-signed Windows installer
- Agent auto-update
