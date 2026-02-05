# Fly.io configuration

## Production (api.leadsnebula.com)

- **App:** `leadsnebula-rust`
- **Domain:** api.leadsnebula.com (see `fly certs list -a leadsnebula-rust`)

**Config (fly.toml):**

- `min_machines_running = 1` — Keeps one machine running so the API always responds. Prevents “app suspended / machine stopped” with no traffic to wake it.
- `auto_stop_machines = "off"` — The running machine is not stopped when idle.
- `auto_start_machines = false` — Incoming traffic does not start stopped machines (manual control).

**Turn off (stop API):**

```bash
fly machine stop <machine_id> -a leadsnebula-rust
# or scale to zero (removes machine):
fly scale count 0 -a leadsnebula-rust
```

**Turn on (start API):**

```bash
# If you scaled to 0, create and start a machine:
fly scale count 1 -a leadsnebula-rust
fly machine list -a leadsnebula-rust   # get machine id
fly machine start <machine_id> -a leadsnebula-rust
```

**Checks:**

```bash
fly status -a leadsnebula-rust
curl -s https://api.leadsnebula.com/health
```

---

## Dev (dev.api / dev.leadsnebula.com)

- **App:** `leadsnebula-rust-dev`
- **Domain:** dev.leadsnebula.com

**Config (fly.dev.toml):**

- `min_machines_running = 0` — Can scale to zero when idle.
- `auto_stop_machines = "off"` — Machines are not auto-stopped when idle.
- `auto_start_machines = false` — Traffic does not start stopped machines.

To have dev respond to traffic when the machine is stopped, set `auto_start_machines = true` in fly.dev.toml (then traffic will start the machine; cold start delay applies).
