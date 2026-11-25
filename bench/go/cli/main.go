// The Go baseline for the command-line benchmark.
//
// It does what `nanobase --version` does: read the arguments, find the
// one it was asked for, and print a line. That is the whole of the work,
// so what the benchmark measures is how long the runtime takes to get
// there — the floor under every other command.
package main

import (
	"fmt"
	"os"
)

const version = "nanobase 0.1.0"

const usage = `nanobase — a PocketBase-style backend on your own database

USAGE:
    nanobase init   [--email E] [--password P] [--data FILE]   create the owner
    nanobase login  <email> [--password P] [--data FILE]       print a session token
    nanobase me     [--token T] [--data FILE]                  show who a token belongs to
    nanobase status [--data FILE]                              setup state, app count
`

func main() {
	args := os.Args[1:]
	if len(args) == 0 {
		fmt.Fprint(os.Stderr, usage)
		os.Exit(2)
	}
	switch args[0] {
	case "-V", "--version", "version":
		fmt.Println(version)
	case "-h", "--help", "help":
		fmt.Print(usage)
	default:
		fmt.Fprintf(os.Stderr, "nanobase: unknown command %q\n", args[0])
		fmt.Fprint(os.Stderr, usage)
		os.Exit(2)
	}
}
