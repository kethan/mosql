#!/usr/bin/env python3
"""Run a command and report how much memory it needed.

The peak resident set size of the child is the number that matters: it is
the most the process ever held at once, which is what a server's memory
bill and a container's limit are about.

    ./rss.py ./nanobase --version

prints `rss_kb=<peak>` on stderr and passes the child's exit status on.
"""

import resource
import subprocess
import sys


def main() -> int:
    if len(sys.argv) < 2:
        print("usage: rss.py COMMAND [ARG...]", file=sys.stderr)
        return 2
    # The peak of every child this process has reaped, so a command that
    # spawns helpers is measured on the whole tree.
    before = resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss
    result = subprocess.run(sys.argv[1:])
    after = resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss
    peak = max(after, before)
    print(f"rss_kb={peak}", file=sys.stderr)
    return result.returncode


if __name__ == "__main__":
    sys.exit(main())
