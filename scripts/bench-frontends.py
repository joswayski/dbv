#!/usr/bin/env python3
"""Benchmark a DBM frontend on Linux: startup, memory, CPU, and one workload.

Every frontend shares crates/dbm-engine, so this measures the presentation
layer. Each run uses a fresh Xvfb display so startup timing is comparable.

Requirements: Xvfb, xdotool, ImageMagick (`import` and `convert`), a running
PostgreSQL with a saved profile, and a keyring the app can unlock (the script
runs the app inside `dbus-run-session` with gnome-keyring, like the orbs do).

Usage:
  bench-frontends.py <label> <binary> <profile_x> <profile_y> <editor_x> <editor_y> [runs]

Example, comparing the Tauri shell with the GTK4 frontend:

  python3 scripts/bench-frontends.py tauri target/release/dbm 56 142 700 300
  python3 scripts/bench-frontends.py native \
    experiments/linux-native/target/release/dbm-native-linux 60 118 700 250

The click coordinates are the saved-connection row and the SQL editor of the
frontend being measured; see docs/native-platforms.md for the recorded run.
"""

import os
import subprocess
import sys
import time

DISPLAY = ":99"
HZ = os.sysconf("SC_CLK_TCK")


def run(cmd, **kwargs):
    return subprocess.run(cmd, shell=True, capture_output=True, text=True, **kwargs)


def start_xvfb():
    subprocess.Popen(
        ["Xvfb", DISPLAY, "-screen", "0", "1400x900x24"],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    time.sleep(1.5)


def stop_xvfb():
    run("pkill -f 'Xvfb :99'")


def window_id():
    result = run(f"DISPLAY={DISPLAY} xdotool search --onlyvisible --name DBM")
    ids = result.stdout.split()
    return ids[-1] if ids else None


def content_spread(window):
    """Rough measure of how much the window differs from a blank frame."""
    result = subprocess.run(
        f"DISPLAY={DISPLAY} import -window {window} png:- | "
        "convert png:- -colorspace gray -resize 32x32! -depth 8 txt:-",
        shell=True,
        capture_output=True,
        text=True,
    )
    values = []
    for line in result.stdout.splitlines():
        if "gray(" in line:
            try:
                values.append(int(line.split("gray(")[1].split(")")[0]))
            except (IndexError, ValueError):
                continue
    if not values:
        return 0.0
    mean = sum(values) / len(values)
    return (sum((v - mean) ** 2 for v in values) / len(values)) ** 0.5


def process_tree(root_pid):
    pids = {root_pid}
    changed = True
    while changed:
        changed = False
        for entry in os.listdir("/proc"):
            if not entry.isdigit() or int(entry) in pids:
                continue
            try:
                with open(f"/proc/{entry}/stat") as handle:
                    fields = handle.read().split()
                if int(fields[3]) in pids:
                    pids.add(int(entry))
                    changed = True
            except (FileNotFoundError, IndexError, ValueError):
                continue
    return pids


def cpu_ticks(pids):
    total = 0
    for pid in pids:
        try:
            with open(f"/proc/{pid}/stat") as handle:
                fields = handle.read().split()
            total += int(fields[13]) + int(fields[14])
        except (FileNotFoundError, IndexError, ValueError):
            continue
    return total


def rss_kb(pids):
    total = 0
    for pid in pids:
        try:
            with open(f"/proc/{pid}/status") as handle:
                for line in handle:
                    if line.startswith("VmRSS:"):
                        total += int(line.split()[1])
                        break
        except FileNotFoundError:
            continue
    return total


def pss_kb(pids):
    """Proportional set size: shared library pages counted once per user."""
    total = 0
    for pid in pids:
        try:
            with open(f"/proc/{pid}/smaps_rollup") as handle:
                for line in handle:
                    if line.startswith("Pss:"):
                        total += int(line.split()[1])
                        break
        except (FileNotFoundError, PermissionError):
            continue
    return total


def find_app_pid(binary):
    """The exec'd binary, matched by its full command line."""
    for entry in os.listdir("/proc"):
        if not entry.isdigit():
            continue
        try:
            with open(f"/proc/{entry}/cmdline", "rb") as handle:
                cmdline = handle.read().split(b"\0")
        except (FileNotFoundError, PermissionError):
            continue
        if cmdline and cmdline[0].decode(errors="ignore") == binary:
            return int(entry)
    return None


def kill_app(binary):
    pid = find_app_pid(binary)
    if pid is not None:
        run(f"kill {pid}")


def sample(app_pid, seconds):
    pids = process_tree(app_pid)
    before = cpu_ticks(pids)
    time.sleep(seconds)
    pids = process_tree(app_pid)
    after = cpu_ticks(pids)
    cpu = (after - before) / (seconds * HZ) * 100
    return rss_kb(pids), pss_kb(pids), cpu


def click(x, y):
    run(
        f"DISPLAY={DISPLAY} xdotool mousemove {x} {y} sleep 0.3 "
        "mousedown 1 sleep 0.15 mouseup 1"
    )


def run_once(label, binary, coords, index):
    profile = coords[:2]
    editor = coords[2:]
    kill_app(binary)
    time.sleep(1)

    started = time.monotonic()
    launch = (
        "dbus-run-session -- bash -c "
        "'eval \"$(printf \"\\n\" | gnome-keyring-daemon --unlock --components=secrets "
        f'2>/dev/null)" ; exec {binary}\''
    )
    log = open(f"/tmp/bench-{label}-{index}.log", "w")
    subprocess.Popen(
        launch,
        shell=True,
        stdout=log,
        stderr=log,
        env={**os.environ, "DISPLAY": DISPLAY},
        start_new_session=True,
    )

    window = None
    deadline = started + 60
    while time.monotonic() < deadline:
        window = window_id()
        if window:
            break
        time.sleep(0.02)
    if not window:
        return None
    t_window = time.monotonic() - started

    # First frame with real content (blank frames have a low spread). The
    # dark workbench sits around 7, so two samples above 3 mean it painted.
    t_paint = None
    hits = 0
    deadline = started + 60
    while time.monotonic() < deadline:
        if content_spread(window) > 3.0:
            hits += 1
            if hits >= 2:
                t_paint = time.monotonic() - started
                break
        else:
            hits = 0
        time.sleep(0.05)

    app_pid = find_app_pid(binary)
    if app_pid is None:
        return None

    rss_idle, pss_idle, cpu_idle = sample(app_pid, 4.0)

    # Workload: connect to the saved PostgreSQL profile and run the default
    # statement (Ctrl+Enter) from the editor.
    click(*profile)
    time.sleep(6)
    click(*editor)
    run(f"DISPLAY={DISPLAY} xdotool key --clearmodifiers ctrl+Return")
    time.sleep(4)
    time.sleep(4)
    rss_load, pss_load, cpu_load = sample(app_pid, 4.0)
    run(f"DISPLAY={DISPLAY} import -window root /tmp/bench-{label}-{index}.png")

    kill_app(binary)
    time.sleep(1)
    return {
        "window_s": t_window,
        "paint_s": t_paint,
        "rss_idle_mb": rss_idle / 1024,
        "pss_idle_mb": pss_idle / 1024,
        "cpu_idle_pct": cpu_idle,
        "rss_load_mb": rss_load / 1024,
        "pss_load_mb": pss_load / 1024,
        "cpu_load_pct": cpu_load,
    }


def main():
    label, binary = sys.argv[1], sys.argv[2]
    coords = [int(value) for value in sys.argv[3:7]]
    runs = int(sys.argv[7]) if len(sys.argv) > 7 else 3
    results = []
    for index in range(1, runs + 1):
        start_xvfb()
        try:
            result = run_once(label, binary, coords, index)
        finally:
            stop_xvfb()
        if result:
            results.append(result)
            print(f"{label} run {index}: {result}", flush=True)
        time.sleep(1)
    if not results:
        print(f"{label}: no results")
        return
    print(f"\n{label} median:")
    for key in results[0]:
        values = sorted(result[key] for result in results if result[key] is not None)
        if values:
            print(f"  {key}: {values[len(values) // 2]:.2f}")


if __name__ == "__main__":
    main()
