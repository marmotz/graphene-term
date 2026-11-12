#!/usr/bin/env python3
"""Records the raw PTY output of a command at a fixed terminal size.

Usage: record.py OUTPUT SECONDS COMMAND [ARGS...]

The command runs in a new PTY of 120 columns by 32 lines with TERM=xterm-256color.
Everything it writes during SECONDS is saved to OUTPUT, then it is killed.
"""

import fcntl
import os
import pty
import select
import signal
import struct
import sys
import termios
import time

COLUMNS, LINES = 120, 32


def main() -> None:
    output, seconds, command = sys.argv[1], float(sys.argv[2]), sys.argv[3:]
    pid, fd = pty.fork()
    if pid == 0:
        os.environ.update(TERM="xterm-256color", COLORTERM="truecolor",
                          COLUMNS=str(COLUMNS), LINES=str(LINES))
        os.execvp(command[0], command)
    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", LINES, COLUMNS, 0, 0))
    os.kill(pid, signal.SIGWINCH)

    data = bytearray()
    deadline = time.monotonic() + seconds
    while (left := deadline - time.monotonic()) > 0:
        ready, _, _ = select.select([fd], [], [], left)
        if not ready:
            continue
        try:
            chunk = os.read(fd, 65536)
        except OSError:
            break
        if not chunk:
            break
        data += chunk

    try:
        os.kill(pid, signal.SIGKILL)
    except ProcessLookupError:
        pass
    os.waitpid(pid, 0)
    with open(output, "wb") as f:
        f.write(data)
    print(f"{output}: {len(data)} bytes")


if __name__ == "__main__":
    main()
