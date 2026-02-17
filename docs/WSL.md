# WSL troubleshooting

When running `./autotestsall.sh` (with or without `--no-neon`) on WSL, Cursor often disconnects **during the compile phase** (0x80072746 "connection forcibly closed", 0x800705aa "Insufficient system resources"). That points to **compilation load** overwhelming the WSL VM, not just Neon or idle phases. Below are quick wins and deeper options.

---

## Quick win 1: Lower build parallelism (do this first)

**In `autotestsall.sh`**, for WSL set **CARGO_BUILD_JOBS=1** so compilation is serial and doesn’t spike the VM:

- Find the block that sets `CARGO_BUILD_JOBS` when `IS_WSL` is true (e.g. around the "WSL detected" message).
- Change the default from **2** to **1**:
  ```bash
  export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-1}"
  ```
- Run `./autotestsall.sh --no-neon` again. If it completes, you can try raising to 2 later.

---

## Quick win 2: Use WSL-native filesystem

If the repo or `target/` is on a Windows mount (`/mnt/c/...`), I/O is much slower and can amplify CPU hangs.

- Check: `df -h .` in the repo — should show **ext4** (WSL), not **drvfs**.
- If you’re on `/mnt/c`, move the repo into the WSL filesystem, then run tests from there:
  ```bash
  mv ~/projects/leadsnebula /home/badinoff/leadsnebula-wsl
  cd /home/badinoff/leadsnebula-wsl/rust && ./autotestsall.sh --no-neon
  ```

---

## Quick win 3: Conservative .wslconfig

This repo’s `.wslconfig` (project root) is set for stability:

- **memory=24GB** — leaves room for Windows + Cursor.
- **processors=12** — must be **less than** your host logical cores (Task Manager > Performance > CPU).
- **swap=0** — avoids paging thrash during spikes (re-enable if you see OOM).

After editing: **`wsl --shutdown`** then reopen WSL/Cursor.

---

## Quick win 4: Cursor WSL extension

Disconnects are sometimes fixed by using a newer WSL extension.

- In Cursor: Ctrl+Shift+X → search "WSL" → update or reinstall.
- Or install from VSIX: [Remote - WSL](https://marketplace.visualstudio.com/items?itemName=ms-vscode-remote.remote-wsl) (e.g. 0.88+).

---

## Quick win 5: Conflicts and antivirus

- Close other Hyper-V apps (Docker Desktop, BlueStacks, Android emulators).
- Exclude WSL/vmmem paths from Windows Defender or other real-time scanning if builds are slow or Cursor drops during compile.

---

## Quick win 6: Run tests outside Cursor

Run the suite in a **standalone WSL terminal** (e.g. Windows Terminal → Ubuntu). Use Cursor only for editing. This removes Cursor’s remote layer from the critical path and often stops disconnects.

---

## If it still fails

- **WSL kernel rollback:** `wsl --update --rollback` (PowerShell), then retest.
- **Try WSL1:** `wsl --set-version Ubuntu-24.04 1` — no VM, so no Hyper-V errors; slower but more stable.
- **Hyper-V logs:** PowerShell: `Get-WinEvent -LogName Microsoft-Windows-Hyper-V-Compute-Admin | Select -Last 10`.
- **Host:** Increase Windows virtual memory (page file); check cooling if CPU is pegged and thermals are high.

---

## DATABASE_URL and --no-neon

- `DATABASE_URL` must be set in `.env` or `.env.local` (same directory as the script). **Quote values that contain `&`** (e.g. `DATABASE_URL="postgresql://...?sslmode=require&channel_binding=require"`).
- If you see "DATABASE_URL not set" with `--no-neon`, run **`AUTOTESTSALL_DEBUG=1 ./autotestsall.sh --no-neon`** to see whether the env files exist and the var is set after sourcing. Fix CRLF with `sed -i 's/\r$//' .env.local` if needed.

---

## Other messages

- **"Failed to patch code.sh" (ENOENT):** Cursor can’t find its Windows path; repair/reinstall Cursor.
- **"131072x1 screen size is bogus":** Harmless; safe to ignore.
