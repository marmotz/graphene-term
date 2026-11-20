#!/usr/bin/env python3
"""Compare terminal emulators: throughput, refresh rate, startup and memory.

Linux only, Python 3 standard library only.

    benchmark-terminals.py run [--terminals a,b,..] [--runs N] [--out results.json]
                               [--columns N] [--lines N]
    benchmark-terminals.py render results.json
    benchmark-terminals.py payload {throughput|fire|idle} --result FILE [...]

`run` benchmarks every installed terminal and prints a Markdown table, `render` turns
saved results into that table, and `payload` is what runs inside the terminal under
test: it writes its measurement to `--result` and exits, which closes the terminal.

Every terminal is opened with the same grid (`--columns` x `--lines`), so that each one
draws as many cells; the grid each payload saw is recorded and a mismatch reported.
"""

import argparse
import fcntl
import hashlib
import json
import os
import platform
import random
import re
import shutil
import statistics
import struct
import subprocess
import sys
import tempfile
import termios
import time
import urllib.request
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent

SHAKESPEARE_URL = (
    "https://raw.githubusercontent.com/vizenc/terminal-benchmark/"
    "refs/heads/main/test/shakespeare.txt"
)
SHAKESPEARE_SHA256 = "5d1293ba864d3f7200f8ed45c7fe75b9bfbc110726a438cdb28835b7c08662b3"

PAYLOAD_TIMEOUT = 60.0
SAMPLE_INTERVAL = 0.1
IDLE_SAMPLE_AT = 3.0
IDLE_DURATION = 5.0
FIRE_DURATION = 10.0
GRID = (100, 30)
# First window sizes, in pixels, probed to fit a terminal sized in pixels to the grid.
CALIBRATION_SIZES = ((800, 600), (1200, 900))
CALIBRATION_ROUNDS = 4

def terminator_config(workdir):
    """Copy of the user's Terminator configuration whose windows open unmaximized, so
    that `--geometry` applies. Returns the `{config}` placeholder."""
    home = Path(os.environ.get("XDG_CONFIG_HOME") or Path.home() / ".config")
    try:
        text = (home / "terminator" / "config").read_text()
    except OSError:
        text = ""
    text = re.sub(r"(?m)^\s*window_state\s*=.*\n?", "", text)
    if re.search(r"(?m)^\[global_config\]", text):
        text = re.sub(
            r"(?m)^\[global_config\]\n", "[global_config]\n  window_state = normal\n", text
        )
    else:
        text = "[global_config]\n  window_state = normal\n" + text
    path = Path(workdir) / "terminator.config"
    path.write_text(text)
    return {"config": str(path)}


# How to start each terminal. `prefix` goes between the binary and the command to run,
# `{columns}` and `{lines}` replaced by the grid; for a terminal whose `size` is in
# `pixels`, `{width}` and `{height}` are replaced by a window size fitted to the grid.
# `setup`, when given, writes files into a temporary directory and returns more
# placeholders. Each one was checked against the terminal's `--help` or manual. `server`
# names the process that holds the windows when the launcher only forwards the request.
TERMINALS = [
    {
        "name": "Graphene",
        "binary": "graphene",
        "version": ["--version"],
        "prefix": ["--columns", "{columns}", "--lines", "{lines}", "-e"],
    },
    {
        "name": "Alacritty",
        "binary": "alacritty",
        "version": ["--version"],
        "prefix": [
            "-o", "window.dimensions.columns={columns}",
            "-o", "window.dimensions.lines={lines}",
            "-e",
        ],
    },
    {
        "name": "Kitty",
        "binary": "kitty",
        "version": ["--version"],
        "prefix": [
            "-o", "remember_window_size=no",
            "-o", "initial_window_width={columns}c",
            "-o", "initial_window_height={lines}c",
        ],
    },
    {
        "name": "WezTerm",
        "binary": "wezterm",
        "version": ["--version"],
        "prefix": [
            "--config", "initial_cols={columns}",
            "--config", "initial_rows={lines}",
            "start", "--always-new-process", "--",
        ],
    },
    {
        "name": "Terminator",
        "binary": "terminator",
        "version": ["--version"],
        # `--geometry` is in pixels: Terminator sets its cell size hints after parsing it.
        # A maximized `window_state` in the user's configuration would override it.
        "prefix": ["-u", "-g", "{config}", "--geometry={width}x{height}", "-x"],
        "size": "pixels",
        "setup": terminator_config,
    },
    {
        "name": "GNOME Terminal",
        "binary": "gnome-terminal",
        "version": ["--version"],
        "prefix": ["--wait", "--geometry={columns}x{lines}", "--"],
        "server": "gnome-terminal-server",
    },
]

# (key, header, unit format, higher is better)
METRICS = [
    ("throughput", "Throughput (s)", "{:.2f}", False),
    ("fps", "Refresh rate (fps)", "{:.1f}", True),
    ("startup", "Startup (ms)", "{:.0f}", False),
    ("memory_idle", "Memory idle (MiB)", "{:.1f}", False),
    ("memory_load", "Memory under load (MiB)", "{:.1f}", False),
]


# --- Rendering ------------------------------------------------------------------------


def median(values):
    present = [v for v in values if v is not None]
    return statistics.median(present) if present else None


def unique(items):
    out = []
    for item in items:
        if item not in out:
            out.append(item)
    return out


def render(results):
    """Return the Markdown tables for a results dict."""
    terminals = sorted(results.get("terminals", []), key=lambda t: t["name"] != "Graphene")
    medians = [
        {key: median(t.get("runs", {}).get(key, [])) for key, *_ in METRICS} for t in terminals
    ]

    best = {}
    for key, _, _, higher in METRICS:
        values = [m[key] for m in medians if m[key] is not None]
        if values:
            best[key] = max(values) if higher else min(values)

    lines = [
        "| Terminal | " + " | ".join(header for _, header, _, _ in METRICS) + " |",
        "|---" * (len(METRICS) + 1) + "|",
    ]
    for terminal, values in zip(terminals, medians):
        name = terminal["name"]
        if name == "Graphene":
            name = f"**{name}**"
        cells = [name]
        for key, _, fmt, _ in METRICS:
            value = values[key]
            if value is None:
                cells.append("n/a")
                continue
            text = fmt.format(value)
            if value == best.get(key):
                text = f"**{text}**"
            cells.append(text)
        lines.append("| " + " | ".join(cells) + " |")

    machine = results.get("machine", {})
    runs = results.get("runs")
    lines += [
        "",
        "Machine:",
        "",
        f"- CPU: {machine.get('cpu', 'unknown')}",
        f"- GPU: {machine.get('gpu', 'unknown')}",
        f"- Kernel: {machine.get('kernel', 'unknown')}",
        f"- Session: {machine.get('session', 'unknown')}",
        f"- Resolution: {machine.get('resolution', 'unknown')}",
    ]
    if runs is not None:
        lines.append(f"- Runs: median of {runs}")
    grid = results.get("grid")
    if grid:
        lines.append(f"- Grid: {grid[0]}x{grid[1]} cells")
        for terminal in terminals:
            seen = [g for g in terminal.get("grids", []) if g != list(grid)]
            if seen:
                sizes = ", ".join(f"{g[0]}x{g[1]}" for g in unique(seen))
                lines.append(f"- Grid mismatch: {terminal['name']} ran at {sizes}")
    lines.append(
        "- Versions: " + ", ".join(t.get("version", t["name"]) for t in terminals)
    )
    return "\n".join(lines) + "\n"


# --- Payloads (run inside the terminal under test) -------------------------------------


def write_result(path, value, grid):
    """Write the measurement, then the grid `columns lines` the payload ran in."""
    Path(path).write_text(f"{value}\n{grid[0]} {grid[1]}\n")


def payload_throughput(args):
    grid = terminal_size()
    start = time.monotonic()
    subprocess.run(["/bin/cat", args.file], check=True)
    write_result(args.result, time.monotonic() - start, grid)


def payload_idle(args):
    grid = terminal_size()
    time.sleep(args.duration)
    write_result(args.result, args.duration, grid)


def terminal_size():
    try:
        packed = fcntl.ioctl(sys.stdout.fileno(), termios.TIOCGWINSZ, b"\0" * 8)
        rows, cols, _, _ = struct.unpack("HHHH", packed)
        if rows and cols:
            return cols, rows
    except OSError:
        pass
    return 80, 24


# DOOM-fire palette (37 steps from black to white) mapped to xterm 256 colors.
FIRE_PALETTE = [
    16, 52, 52, 88, 88, 124, 124, 160, 160, 196, 196, 202, 202, 202, 208, 208, 208, 214,
    214, 214, 214, 220, 220, 220, 220, 226, 226, 226, 227, 227, 228, 228, 229, 229, 230,
    231, 231,
]


def fire_frames(cols, rows, count):
    """Precompute `count` frames of the DOOM fire, two pixels per cell (half blocks)."""
    width, height = cols, rows * 2
    top = len(FIRE_PALETTE) - 1
    pixels = [0] * (width * height)
    for x in range(width):
        pixels[(height - 1) * width + x] = top
    rng = random.Random(1)
    frames = []
    for _ in range(count):
        for x in range(width):
            for y in range(1, height):
                src = y * width + x
                spread = rng.randint(0, 3)
                dst = src - spread + 1 - width
                if 0 <= dst < len(pixels):
                    pixels[dst] = max(pixels[src] - (spread & 1), 0)
        out = ["\x1b[H"]
        for row in range(rows):
            upper = pixels[(2 * row) * width : (2 * row + 1) * width]
            lower = pixels[(2 * row + 1) * width : (2 * row + 2) * width]
            last = None
            for u, d in zip(upper, lower):
                colors = (FIRE_PALETTE[u], FIRE_PALETTE[d])
                if colors != last:
                    out.append(f"\x1b[38;5;{colors[0]}m\x1b[48;5;{colors[1]}m")
                    last = colors
                out.append("▀")
            if row < rows - 1:
                out.append("\r\n")
        frames.append("".join(out).encode())
    return frames


def payload_fire(args):
    cols, rows = terminal_size()
    frames = fire_frames(cols, rows, args.frames)
    out = sys.stdout.buffer
    out.write(b"\x1b[?25l\x1b[2J")
    written = 0
    start = time.monotonic()
    while True:
        out.write(frames[written % len(frames)])
        out.flush()
        written += 1
        elapsed = time.monotonic() - start
        if elapsed >= args.duration:
            break
    out.write(b"\x1b[0m\x1b[?25h")
    out.flush()
    write_result(args.result, written / elapsed, (cols, rows))


# --- Process sampling ------------------------------------------------------------------


def processes():
    """Map pid -> (parent pid, command name) for every visible process."""
    table = {}
    for entry in os.listdir("/proc"):
        if not entry.isdigit():
            continue
        try:
            stat = Path(f"/proc/{entry}/stat").read_text()
        except OSError:
            continue
        name = stat[stat.index("(") + 1 : stat.rindex(")")]
        parent = int(stat[stat.rindex(")") + 2 :].split()[1])
        table[int(entry)] = (parent, name)
    return table


def cmdline(pid):
    try:
        return Path(f"/proc/{pid}/cmdline").read_bytes().replace(b"\0", b" ").decode(errors="replace")
    except OSError:
        return ""


def is_payload(pid):
    """True for the payload process: `<python> .../benchmark-terminals.py payload ...`."""
    try:
        argv = Path(f"/proc/{pid}/cmdline").read_bytes().split(b"\0")
    except OSError:
        return False
    return len(argv) > 2 and argv[1].endswith(b"benchmark-terminals.py") and argv[2] == b"payload"


def find_server(name, exclude=()):
    for pid in processes():
        if pid not in exclude and name in cmdline(pid):
            return pid
    return None


def terminal_tree(root, table):
    """The root and its descendants, minus the payload and its children."""
    children = {}
    for pid, (parent, _) in table.items():
        children.setdefault(parent, []).append(pid)
    tree, stack = [], [root]
    while stack:
        pid = stack.pop()
        if pid != root and is_payload(pid):
            continue
        tree.append(pid)
        stack.extend(children.get(pid, []))
    return tree


def pss_mib(pids):
    total_kib = 0
    for pid in pids:
        try:
            text = Path(f"/proc/{pid}/smaps_rollup").read_text()
        except OSError:
            continue
        match = re.search(r"^Pss:\s+(\d+) kB", text, re.MULTILINE)
        if match:
            total_kib += int(match.group(1))
    return total_kib / 1024


# --- Running a terminal ----------------------------------------------------------------


class Terminal:
    def __init__(self, spec, binary):
        self.name = spec["name"]
        self.binary = binary
        self.prefix = spec["prefix"]
        self.server = spec.get("server")
        self.pixels = spec.get("size") == "pixels"
        self.setup = spec.get("setup")
        self.size = {}
        self.extra = {}
        self.version = self.read_version(spec["version"])

    def prepare(self, workdir):
        """Run the terminal's `setup` in `workdir`, which outlives its launches."""
        if self.setup is not None:
            self.extra = self.setup(workdir)

    def set_grid(self, columns, lines, width=None, height=None):
        """Size the next windows: `width` x `height` pixels for a terminal sized in pixels."""
        self.size = {"columns": columns, "lines": lines, "width": width, "height": height}

    def fit_pixels(self, columns, lines, workdir):
        """Find the window size, in pixels, that holds `columns` x `lines` cells.

        Probes two sizes to find the cell size, then corrects the size until the grid
        matches. Keeps the closest size found when it never matches.
        """
        result = os.path.join(workdir, "probe")

        def probe(width, height):
            self.set_grid(columns, lines, width, height)
            Path(result).unlink(missing_ok=True)
            elapsed, _, _ = self.launch(payload_command("idle", result, "--duration", "0"))
            return None if elapsed is None else read_result(result)[1]

        (w1, h1), (w2, h2) = CALIBRATION_SIZES
        first, second = probe(w1, h1), probe(w2, h2)
        if not first or not second or first[0] == second[0] or first[1] == second[1]:
            self.set_grid(columns, lines, w1, h1)
            return
        cell_width = (w2 - w1) / (second[0] - first[0])
        cell_height = (h2 - h1) / (second[1] - first[1])
        width = round(w1 + (columns - first[0] + 0.5) * cell_width)
        height = round(h1 + (lines - first[1] + 0.5) * cell_height)
        for _ in range(CALIBRATION_ROUNDS):
            grid = probe(width, height)
            if grid is None or grid == (columns, lines):
                break
            width += round((columns - grid[0]) * cell_width)
            height += round((lines - grid[1]) * cell_height)
        self.set_grid(columns, lines, width, height)

    def command(self, command):
        values = {**self.size, **self.extra}
        return [self.binary, *(arg.format(**values) for arg in self.prefix), *command]

    def read_version(self, args):
        try:
            done = subprocess.run(
                [self.binary, *args], capture_output=True, text=True, timeout=10
            )
        except (OSError, subprocess.TimeoutExpired):
            return self.name
        for line in (done.stdout + done.stderr).splitlines():
            line = line.strip().lstrip("#").strip()
            if line:
                return line
        return self.name

    def launch(self, command, sample_at=None):
        """Run `command` inside the terminal and sample the terminal's memory.

        Returns (elapsed seconds or None on timeout, peak PSS, PSS at `sample_at`).
        """
        existing = set(processes())
        start = time.monotonic()
        proc = subprocess.Popen(
            self.command(command),
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        root = None if self.server else proc.pid
        peak, at_sample = 0.0, None
        timed_out = False
        while proc.poll() is None:
            now = time.monotonic() - start
            if now > PAYLOAD_TIMEOUT:
                proc.kill()
                proc.wait()
                timed_out = True
                break
            if root is None:
                root = find_server(self.server, existing)
            if root is not None:
                memory = pss_mib(terminal_tree(root, processes()))
                peak = max(peak, memory)
                if sample_at is not None and at_sample is None and now >= sample_at:
                    at_sample = memory
            time.sleep(SAMPLE_INTERVAL)
        elapsed = time.monotonic() - start
        if self.server and root is not None:
            wait_exit(root)
        return (None if timed_out else elapsed), peak or None, at_sample


def wait_exit(pid, timeout=30.0):
    deadline = time.monotonic() + timeout
    while Path(f"/proc/{pid}").exists() and time.monotonic() < deadline:
        time.sleep(0.2)


def read_result(path):
    """The measurement and the grid `(columns, lines)` a payload wrote, each None when
    missing."""
    try:
        lines = Path(path).read_text().splitlines()
    except OSError:
        return None, None
    try:
        value = float(lines[0])
    except (IndexError, ValueError):
        value = None
    try:
        columns, rows = (int(n) for n in lines[1].split())
        grid = (columns, rows)
    except (IndexError, ValueError):
        grid = None
    return value, grid


def payload_command(kind, result, *extra):
    return [sys.executable, str(Path(__file__).resolve()), "payload", kind, "--result", result, *extra]


def measure(terminal, shakespeare, runs, grid):
    """Run every benchmark `runs` times. Returns the values of each metric and the grids
    the payloads ran in."""
    data = {key: [] for key, *_ in METRICS}
    grids = []

    def payload(kind, *extra, sample_at=None):
        Path(result).unlink(missing_ok=True)
        elapsed, peak, sampled = terminal.launch(
            payload_command(kind, result, *extra), sample_at=sample_at
        )
        if elapsed is None:
            return None, None, None
        value, seen = read_result(result)
        if seen is not None:
            grids.append(list(seen))
            if seen != tuple(grid):
                print(
                    f"  warning: {terminal.name} ran at {seen[0]}x{seen[1]} cells, "
                    f"not {grid[0]}x{grid[1]}",
                    file=sys.stderr,
                )
        return value, peak, sampled

    with tempfile.TemporaryDirectory(prefix="graphene-benchmark-") as tmp:
        result = os.path.join(tmp, "result")
        terminal.prepare(tmp)
        if terminal.pixels:
            print(f"  {terminal.name}: fitting the window to the grid", file=sys.stderr)
            terminal.fit_pixels(*grid, tmp)
        else:
            terminal.set_grid(*grid)
        for run in range(runs):
            print(f"  {terminal.name}: run {run + 1}/{runs}", file=sys.stderr)

            elapsed, _, _ = terminal.launch(["true"])
            data["startup"].append(None if elapsed is None else elapsed * 1000)

            value, peak, _ = payload("throughput", "--file", str(shakespeare))
            data["throughput"].append(value)
            data["memory_load"].append(peak if value is not None else None)

            value, _, _ = payload("fire", "--duration", str(FIRE_DURATION))
            data["fps"].append(value)

            _, _, idle = payload(
                "idle", "--duration", str(IDLE_DURATION), sample_at=IDLE_SAMPLE_AT
            )
            data["memory_idle"].append(idle)
    return data, unique(grids)


def shakespeare_file():
    cache = Path(os.environ.get("XDG_CACHE_HOME") or Path.home() / ".cache")
    path = cache / "graphene-benchmark" / "shakespeare.txt"
    if not path.exists():
        path.parent.mkdir(parents=True, exist_ok=True)
        print(f"Downloading {SHAKESPEARE_URL}", file=sys.stderr)
        with urllib.request.urlopen(SHAKESPEARE_URL, timeout=60) as response:
            path.write_bytes(response.read())
    digest = hashlib.sha256(path.read_bytes()).hexdigest()
    if digest != SHAKESPEARE_SHA256:
        path.unlink()
        sys.exit(f"error: {path} has SHA-256 {digest}, expected {SHAKESPEARE_SHA256}")
    return path


def graphene_binary():
    """Build the current checkout in release mode and return the built binary."""
    cargo = shutil.which("cargo")
    if cargo is None:
        sys.exit("error: cargo is needed to build Graphene")
    print("Building Graphene (cargo build --release)", file=sys.stderr)
    done = subprocess.run(
        [cargo, "build", "--release", "--bin", "graphene"], cwd=REPO, stdout=sys.stderr
    )
    if done.returncode != 0:
        sys.exit(f"error: cargo build --release failed (exit code {done.returncode})")
    target = Path(os.environ.get("CARGO_TARGET_DIR") or REPO / "target")
    if not target.is_absolute():
        target = REPO / target
    built = target / "release" / "graphene"
    if not (built.is_file() and os.access(built, os.X_OK)):
        sys.exit(f"error: {built} not found after the build")
    return str(built)


def machine_info():
    cpu = "unknown"
    try:
        for line in Path("/proc/cpuinfo").read_text().splitlines():
            if line.startswith("model name"):
                cpu = line.split(":", 1)[1].strip()
                break
    except OSError:
        pass

    gpu = "unknown"
    try:
        lspci = subprocess.run(["lspci"], capture_output=True, text=True, timeout=10).stdout
        gpus = [
            line.split(": ", 1)[1]
            for line in lspci.splitlines()
            if ("VGA compatible controller" in line or "3D controller" in line) and ": " in line
        ]
        if gpus:
            gpu = "; ".join(gpus)
    except (OSError, subprocess.TimeoutExpired):
        pass

    resolution = "unknown"
    try:
        xrandr = subprocess.run(
            ["xrandr", "--current"], capture_output=True, text=True, timeout=10
        ).stdout
        match = re.search(r"current (\d+) x (\d+)", xrandr)
        if match:
            resolution = f"{match.group(1)}x{match.group(2)}"
    except (OSError, subprocess.TimeoutExpired):
        pass

    return {
        "cpu": cpu,
        "gpu": gpu,
        "kernel": platform.release(),
        "session": os.environ.get("XDG_SESSION_TYPE", "unknown"),
        "resolution": resolution,
    }


def command_run(args):
    if not sys.platform.startswith("linux"):
        sys.exit("error: the benchmark runs on Linux only")
    wanted = None
    if args.terminals:
        wanted = {name.strip().lower() for name in args.terminals.split(",") if name.strip()}

    terminals, skipped = [], []
    for spec in TERMINALS:
        if wanted is not None and spec["name"].lower() not in wanted:
            continue
        binary = graphene_binary() if spec["name"] == "Graphene" else shutil.which(spec["binary"])
        if binary is None:
            skipped.append(f"{spec['name']} (not installed)")
            continue
        if spec.get("server") and find_server(spec["server"]) is not None:
            skipped.append(
                f"{spec['name']} (another {spec['server']} is running; close it to include it)"
            )
            continue
        terminals.append(Terminal(spec, binary))

    for reason in skipped:
        print(f"Skipped: {reason}", file=sys.stderr)
    if not terminals:
        sys.exit("error: no terminal to benchmark")

    shakespeare = shakespeare_file()
    grid = [args.columns, args.lines]
    results = {"machine": machine_info(), "runs": args.runs, "grid": grid, "terminals": []}
    for terminal in terminals:
        print(f"Benchmarking {terminal.name} ({terminal.version})", file=sys.stderr)
        runs, grids = measure(terminal, shakespeare, args.runs, grid)
        results["terminals"].append(
            {"name": terminal.name, "version": terminal.version, "runs": runs, "grids": grids}
        )

    Path(args.out).write_text(json.dumps(results, indent=2) + "\n")
    print(f"Raw results written to {args.out}", file=sys.stderr)
    sys.stdout.write(render(results))


def command_render(args):
    sys.stdout.write(render(json.loads(Path(args.results).read_text())))


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    sub = parser.add_subparsers(dest="command", required=True)

    run = sub.add_parser("run", help="benchmark the installed terminals")
    run.add_argument("--terminals", help="comma-separated names, e.g. graphene,alacritty")
    run.add_argument("--runs", type=int, default=5, help="runs per terminal (median reported)")
    run.add_argument("--out", default="benchmark-results.json", help="raw results JSON")
    run.add_argument("--columns", type=int, default=GRID[0], help="grid width, in cells")
    run.add_argument("--lines", type=int, default=GRID[1], help="grid height, in cells")
    run.set_defaults(func=command_run)

    rend = sub.add_parser("render", help="print the Markdown tables of a results JSON")
    rend.add_argument("results")
    rend.set_defaults(func=command_render)

    payload = sub.add_parser("payload", help="run inside the terminal under test")
    payload.add_argument("kind", choices=["throughput", "fire", "idle"])
    payload.add_argument("--result", required=True, help="file the measurement is written to")
    payload.add_argument("--file", help="file to cat (throughput)")
    payload.add_argument("--duration", type=float, help="seconds (fire, idle)")
    payload.add_argument("--frames", type=int, default=32, help="precomputed frames (fire)")
    payload.set_defaults(func=command_payload)

    args = parser.parse_args()
    if args.command == "run" and args.runs < 1:
        parser.error("--runs must be at least 1")
    if args.command == "run" and (args.columns < 2 or args.lines < 1):
        parser.error("the grid needs at least 2 columns and 1 line")
    args.func(args)


def command_payload(args):
    if args.kind == "throughput":
        if not args.file:
            sys.exit("error: payload throughput needs --file")
        payload_throughput(args)
    elif args.kind == "fire":
        args.duration = FIRE_DURATION if args.duration is None else args.duration
        payload_fire(args)
    else:
        args.duration = IDLE_DURATION if args.duration is None else args.duration
        payload_idle(args)


if __name__ == "__main__":
    main()
