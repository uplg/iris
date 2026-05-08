# Iris production deployment

Target: a Debian 13 (Trixie) box running rootless Docker, e.g. an OVH KS-5
(Xeon E3-1270v6, 32 GB, 2x450 GB NVMe in software RAID — `/dev/md3` mounted
at `/`, ≈ 820 GB usable). This guide assumes that machine and a
Cloudflare-tunnelled public URL.

If you're on a different host: skip the OVH-specific bits, but the rootless
Docker / systemd-user / data-root sections still apply unchanged.

---

## 1. Pre-flight on the box

```bash
# All commands as the unprivileged user that owns the docker socket.
sudo apt update && sudo apt install -y docker.io docker-compose-plugin git curl
# Rootless setup if not already done — see https://docs.docker.com/engine/security/rootless/
dockerd-rootless-setuptool.sh install
echo 'export DOCKER_HOST=unix:///run/user/$(id -u)/docker.sock' >> ~/.bashrc

# Lingering: keeps systemd user services (and rootless dockerd) running after
# you log out — required for the iris service to survive SSH disconnects.
sudo loginctl enable-linger "$USER"

# Verify
docker info | grep -i rootless   # → "Rootless: true"
```

**BitTorrent port (45100):** by default Debian Trixie ships with no
configured firewall (no `ufw`, and `nftables` exists but with empty
rule tables) — meaning the port is already reachable from the internet
once the iris container publishes it. Verify with an external port scan
**after** §5 once the service is up:

```bash
# From a different machine, ideally outside the OVH network:
nmap -Pn -p 45100 <server-public-ip>
# → 45100/tcp open
```

If you've explicitly installed a firewall, open the port:

```bash
# nftables (the modern default if you've configured anything)
sudo nft add rule inet filter input tcp dport 45100 accept
sudo nft add rule inet filter input udp dport 45100 accept

# OR ufw (if you opted in to it)
sudo ufw allow 45100/tcp
sudo ufw allow 45100/udp
```

OVH's per-IP "Network Firewall" (Manager → Bare Metal → IP → Firewall) is
**off by default** — it's opt-in per IP. If you haven't enabled it, skip;
the anti-DDoS that's always on doesn't block ordinary inbound traffic. If
you have enabled it, add `45100/tcp` and `45100/udp` accept rules.

The HTTP port stays unexposed — Cloudflare tunnel handles ingress, no
inbound 80/443 needed.

---

## 2. Clone & configure

```bash
mkdir -p /srv/iris && cd /srv/iris
git clone https://github.com/<your-fork>/iris.git .
cp .env.example .env
```

Edit `.env`:

```dotenv
# Required — generate with: openssl rand -base64 48
IRIS_JWT_SECRET=<48-byte base64>

# Bootstrap admin (first boot only — once you log in & create real users you
# can comment these out, or leave them and delete the user via the admin UI).
IRIS_AUTH__BOOTSTRAP_ADMIN__EMAIL=you@example.com
IRIS_AUTH__BOOTSTRAP_ADMIN__PASSWORD=<long random>

# Public URL — must match the Cloudflare tunnel hostname exactly. JWT issuer
# claims and the device pairing verification URL are derived from this.
IRIS_SERVER__PUBLIC_URL=https://iris.example.com

# BitTorrent inbound port. Leave the default unless you have a conflict;
# changing it here also changes the published port and the firewall rule
# you opened above.
IRIS_TORRENT_PORT=45100

# Cloudflare tunnel token — see §4.
CLOUDFLARE_TUNNEL_TOKEN=<paste>

# TMDB API key — free, used for poster/backdrop/title metadata. Without it
# the search and library work but show no posters.
IRIS_TMDB__API_KEY=<get one at https://www.themoviedb.org/settings/api>

# Tracker creds (one block per provider in config/providers.toml).
TORR9_USERNAME=…
TORR9_PASSWORD=…

# Less verbose by default in production. iris=info also surfaces ingest
# errors; iris=warn would be quieter still.
RUST_LOG=info,iris=info
```

---

## 3. Storage tuning for KS-5

Edit `config/config.toml`:

```toml
providers_file = "./config/providers.toml"

[server]
bind = "0.0.0.0:8080"
public_url = "https://iris.example.com"

[storage]
data_dir = "/data"
download_dir = "/data/downloads"
# Cap downloads at 700 GB on the 820 GB / volume — leaves ~120 GB headroom
# for OS, logs, the HLS cache and unforeseen Bad Days. The GC kicks in
# at 90 % full and trims back to 75 %, evicting the least-recently-played
# torrents first.
max_storage_gb = 700
cleanup_threshold_pct = 90
cleanup_target_pct = 75
torrent_port = 45100
# HLS cache eviction: re-segmented playlists that haven't been accessed
# for 7 days are pruned hourly. The source torrent stays seeded; the
# segments regenerate on next Play (~5-30s). Cuts effective disk use
# roughly in half on a movie-heavy library.
hls_idle_eviction_days = 7

[auth]
# Real values come from .env (IRIS_AUTH__JWT_SECRET, BOOTSTRAP_ADMIN, …).
# Keep these as a development fallback only.
jwt_secret = "change-me-in-production"
access_ttl_secs = 3600
refresh_ttl_secs = 604800
invitation_ttl_secs = 604800
```

**Why a Docker named volume + not a bind mount on `/dev/md3`** — rootless
Docker remaps UIDs through `/etc/subuid` (container UID 1001 → host UID
101001), so a bind-mounted directory needs `chown 101001:101001` before
first run. The named volume `iris-data` lives at
`~/.local/share/docker/volumes/iris-data/_data` which is on `/`, i.e. on
`/dev/md3`, so it gets the same 820 GB headroom without the chown dance.
Backups are still trivial:

```bash
# One-shot backup — pipe into your destination of choice.
docker run --rm -v iris-data:/data alpine tar -czC / data > iris-data.tgz
```

---

## 4. Cloudflare tunnel

1. <https://one.dash.cloudflare.com/> → **Networks → Tunnels → Create
   tunnel** → "Cloudflared" → name it `iris`.
2. Skip the "install on this machine" step; copy just the **tunnel token**
   into `.env` as `CLOUDFLARE_TUNNEL_TOKEN`.
3. **Public Hostname** tab → add an entry pointing your hostname
   (e.g. `iris.example.com`) at **Service: `http://iris:8080`**.
   `iris` here is the compose service name, resolved over the
   compose-internal network — *not* `localhost` (a frequent mistake that
   surfaces as a 502 from Cloudflare).
4. Cloudflare will create the DNS CNAME for you.

The compose file forces HTTP/2 transport (`--protocol http2`):
QUIC over UDP/7844 outbound is silently dropped by some ISPs and was the
cause of `failed to dial to edge with quic: timeout` loops in development.

---

## 5. First boot

```bash
# Compose v2 with both profiles: API + tunnel.
docker compose --profile cloudflared up -d --build

# Watch the bootstrap migration apply (0001..0005) and the bootstrap
# admin get created.
docker compose logs -f iris
```

You should see something like:

```
INFO  iris_db: applied migration 0005_torrents_tmdb_id.sql
INFO  iris_api: bootstrap admin created  email=you@example.com
INFO  iris_api: iris listening  addr=0.0.0.0:8080
```

Open `https://iris.example.com` and log in with the bootstrap admin. Once
you've created real users via the admin UI, comment out the bootstrap
admin lines in `.env` and `docker compose up -d` again — it becomes a
no-op when the users table isn't empty, but removing it stops shipping
those credentials in your environment.

---

## 6. Auto-start on reboot

Rootless Docker + lingering already keep the daemon up across reboots; we
just need a systemd user unit that runs `docker compose up -d` for the
project. Drop this in `~/.config/systemd/user/iris.service`:

```ini
[Unit]
Description=Iris (docker compose)
After=docker.service
Requires=docker.service

[Service]
Type=oneshot
RemainAfterExit=yes
WorkingDirectory=/srv/iris
ExecStart=/usr/bin/docker compose --profile cloudflared up -d
ExecStop=/usr/bin/docker compose --profile cloudflared down

[Install]
WantedBy=default.target
```

```bash
systemctl --user daemon-reload
systemctl --user enable --now iris.service
systemctl --user status iris.service
```

Reboot once to verify. The unit comes up before any login because
`enable-linger` was set in §1.

---

## 7. Updates

```bash
cd /srv/iris
git pull
docker compose --profile cloudflared up -d --build   # rebuilds iris:dev
```

Migrations apply automatically on container start. Worst case (failed
migration), the container exits and the previous version stays in the
image cache — `docker compose down && docker compose up -d` rolls
forward, never back. To roll back an image, pin a previous git SHA and
rebuild.

The Android TV app is independent: its release APK is sideloaded
manually (Downloader, ADB sideload, etc.). Rebuilding the server doesn't
require a TV-side update unless you bumped a wire-format DTO.

---

## 8. Backup strategy

The *only* persistent state is the `iris-data` volume:

* `iris.db` — SQLite, holds users, refresh tokens, torrent provenance,
  playback progress, paired devices.
* `downloads/` — the actual media on disk.
* `librqbit/` — torrent session resume data (regeneratable from disk +
  the DB, but losing it means re-checking every torrent on next boot).
* `hls/` — re-segmented HLS, regenerable.

Realistic backup target: **just `iris.db`**. Everything else is either
huge and replaceable (`downloads/`) or ephemeral (`hls/`).

```bash
# Daily DB dump, kept for 14 days. Drop in /etc/cron.daily/iris-db-backup
docker exec iris-iris-1 sqlite3 /data/iris.db ".backup '/data/iris.db.bak'"
docker cp iris-iris-1:/data/iris.db.bak /backup/iris-db-$(date +%F).db
find /backup -name 'iris-db-*.db' -mtime +14 -delete
```

(Use `cron` directly or copy the snippet into a systemd timer — either
works fine for once-a-day.)

---

## 9. Troubleshooting

| Symptom | Where to look |
|---|---|
| 502 from Cloudflare | The tunnel's "Public Hostname" service must be `http://iris:8080`, not `localhost:8080`. |
| `failed to dial to edge with quic: timeout` | Confirm `--protocol http2` is in the compose file. (Some hosts still QUIC-handshake even with the flag — `TUNNEL_TRANSPORT_PROTOCOL=http2` env var is the belt for the suspenders.) |
| Torrents never connect to peers | First verify reachability from outside: `nmap -Pn -p 45100 <ip>`. If the port is filtered, check (in this order): the iris container is publishing it (`docker compose ps`), no host-level firewall is blocking (`sudo nft list ruleset`, `sudo ufw status` if installed), and finally OVH's per-IP Network Firewall in the manager (off by default — only relevant if you opted in). |
| `admin` login fails on first boot | The bootstrap block is a no-op if any user exists. Check `docker compose exec iris sqlite3 /data/iris.db 'select email from users;'` — you may have an orphaned account from a prior install. |
| `Field 'X' is required …` JSON errors | Wire format drift. Pin the iris image and the Android APK to the same git SHA. |
| Downloads slow despite a 1 Gbps line | rootless slirp4netns caps around 600 Mbps. If you genuinely saturate that, switch the iris service to `network_mode: host` (you'll then need to point Cloudflare's "Public Hostname" service at `http://localhost:8080` instead of `http://iris:8080`). |

---

## 10. Sanity check after deploy

```bash
# Health
curl -fsS https://iris.example.com/api/health
# → "ok"

# Pairing flow live (auth-free)
curl -fsS -X POST https://iris.example.com/api/auth/device/code \
     -H 'Content-Type: application/json' -d '{"kind":"android-tv"}'
# → {"code":"XXXX-XXXX","device_id":"…","verification_url":"https://iris.example.com/account?pair=XXXX-XXXX","expires_in":600}

# Static frontend served
curl -fsSI https://iris.example.com | grep -i content-type
# → content-type: text/html

# Torrent port reachable from outside
nmap -Pn -p 45100 <server-public-ip>
# → 45100/tcp open
```

If all four pass you're good — sideload the Android TV release APK and
re-pair, library / Continue Watching / Downloading shelves should
populate within a couple of seconds.
