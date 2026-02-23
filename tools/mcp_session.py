#!/usr/bin/env python3
"""Persistent MCP session manager using FIFOs for interactive play.

Start:   mcp_session.py start
Call:     mcp_session.py call <tool_name> [json_args]
Stop:    mcp_session.py stop
"""
import subprocess
import json
import sys
import os
import threading
import time
import signal
import fcntl

SESSION_DIR = "/tmp/roguelike_mcp_session"
PID_FILE = os.path.join(SESSION_DIR, "server.pid")
INPUT_FIFO = os.path.join(SESSION_DIR, "input.fifo")
OUTPUT_FILE = os.path.join(SESSION_DIR, "responses.jsonl")
LOCK_FILE = os.path.join(SESSION_DIR, "call.lock")
REQ_ID_FILE = os.path.join(SESSION_DIR, "next_id")


def start_session():
    """Start the MCP server as a background daemon."""
    # Clean up old session
    stop_session(quiet=True)

    os.makedirs(SESSION_DIR, exist_ok=True)

    # Create FIFO for input
    if os.path.exists(INPUT_FIFO):
        os.unlink(INPUT_FIFO)
    os.mkfifo(INPUT_FIFO)

    # Clear output file
    with open(OUTPUT_FILE, 'w'):
        pass  # truncate

    # Write initial request ID
    with open(REQ_ID_FILE, 'w') as f:
        f.write("2")

    binary = os.path.join(os.path.dirname(__file__), '..', 'target', 'debug', 'mcp_server')
    binary = os.path.realpath(binary)

    # Fork a daemon that manages the MCP server
    pid = os.fork()
    if pid > 0:
        # Parent: wait for the server to be ready
        time.sleep(0.5)
        print(f"Session started (daemon pid={pid})")
        return

    # Child: become session leader
    os.setsid()

    # Start MCP server
    proc = subprocess.Popen(
        [binary],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
    )

    # Write PID
    with open(PID_FILE, 'w') as f:
        f.write(str(os.getpid()))

    # Send initialize
    init_msg = json.dumps({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "play", "version": "0.1"}
        }
    }) + '\n'
    proc.stdin.write(init_msg.encode())
    proc.stdin.flush()

    # Read init response
    proc.stdout.readline()

    # Send initialized notification
    notif = json.dumps({"jsonrpc": "2.0", "method": "notifications/initialized"}) + '\n'
    proc.stdin.write(notif.encode())
    proc.stdin.flush()
    time.sleep(0.1)

    # Response reader thread
    def read_responses():
        while True:
            line = proc.stdout.readline()
            if not line:
                break
            line = line.decode().strip()
            if line:
                with open(OUTPUT_FILE, 'a') as f:
                    f.write(line + '\n')

    reader = threading.Thread(target=read_responses, daemon=True)
    reader.start()

    # Main loop: read commands from FIFO
    while True:
        try:
            with open(INPUT_FIFO, 'r') as fifo:
                for line in fifo:
                    line = line.strip()
                    if line == "QUIT":
                        proc.stdin.close()
                        proc.terminate()
                        cleanup_session()
                        os._exit(0)
                    if line:
                        proc.stdin.write((line + '\n').encode())
                        proc.stdin.flush()
        except (OSError, BrokenPipeError):
            break

    proc.terminate()
    cleanup_session()
    os._exit(0)


def call_tool(tool_name, args=None):
    """Send a tool call and wait for the response."""
    if args is None:
        args = {}

    if not os.path.exists(PID_FILE):
        print("No session running. Use 'start' first.")
        sys.exit(1)

    # Read and increment request ID atomically
    lock_fd = open(LOCK_FILE, 'w')
    fcntl.flock(lock_fd, fcntl.LOCK_EX)
    try:
        with open(REQ_ID_FILE, 'r') as f:
            req_id = int(f.read().strip())
        with open(REQ_ID_FILE, 'w') as f:
            f.write(str(req_id + 1))
    finally:
        fcntl.flock(lock_fd, fcntl.LOCK_UN)
        lock_fd.close()

    # Count existing responses before sending
    try:
        with open(OUTPUT_FILE, 'r') as f:
            existing_lines = len(f.readlines())
    except FileNotFoundError:
        existing_lines = 0

    # Send the tool call via FIFO
    msg = json.dumps({
        "jsonrpc": "2.0",
        "id": req_id,
        "method": "tools/call",
        "params": {
            "name": tool_name,
            "arguments": args
        }
    })

    with open(INPUT_FIFO, 'w') as fifo:
        fifo.write(msg + '\n')

    # Wait for response with matching ID
    deadline = time.time() + 20
    while time.time() < deadline:
        try:
            with open(OUTPUT_FILE, 'r') as f:
                lines = f.readlines()
            # Check new lines for our response
            for line in lines[existing_lines:]:
                line = line.strip()
                if not line:
                    continue
                try:
                    resp = json.loads(line)
                    if resp.get('id') == req_id:
                        if 'result' in resp:
                            content = resp['result'].get('content', [])
                            for c in content:
                                if c.get('type') == 'text':
                                    try:
                                        parsed = json.loads(c['text'])
                                        print(json.dumps(parsed, indent=2))
                                    except (json.JSONDecodeError, ValueError):
                                        print(c['text'])
                        elif 'error' in resp:
                            print(json.dumps(resp['error'], indent=2))
                        return
                except json.JSONDecodeError:
                    continue
        except FileNotFoundError:
            time.sleep(0.1)
            continue
        time.sleep(0.1)

    print(f"Timeout waiting for response (id={req_id})")


def stop_session(quiet=False):
    """Stop the running session."""
    if os.path.exists(PID_FILE):
        try:
            with open(PID_FILE, 'r') as f:
                pid = int(f.read().strip())
            os.kill(pid, signal.SIGTERM)
            if not quiet:
                print(f"Session stopped (pid={pid})")
        except (ProcessLookupError, ValueError, FileNotFoundError):
            if not quiet:
                print("Session was not running")
    elif not quiet:
        print("No session to stop")

    cleanup_session()


def cleanup_session():
    """Remove session files."""
    for path in [PID_FILE, INPUT_FIFO, OUTPUT_FILE, LOCK_FILE, REQ_ID_FILE]:
        try:
            os.unlink(path)
        except FileNotFoundError:
            continue
    try:
        os.rmdir(SESSION_DIR)
    except (FileNotFoundError, OSError):
        pass  # directory not empty or doesn't exist — that's fine


def main():
    if len(sys.argv) < 2:
        print("Usage:")
        print("  mcp_session.py start")
        print("  mcp_session.py call <tool_name> [json_args]")
        print("  mcp_session.py stop")
        sys.exit(1)

    cmd = sys.argv[1]
    if cmd == "start":
        start_session()
    elif cmd == "call":
        if len(sys.argv) < 3:
            print("Usage: mcp_session.py call <tool_name> [json_args]")
            sys.exit(1)
        tool_name = sys.argv[2]
        args = json.loads(sys.argv[3]) if len(sys.argv) > 3 else {}
        call_tool(tool_name, args)
    elif cmd == "stop":
        stop_session()
    else:
        print(f"Unknown command: {cmd}")
        sys.exit(1)


if __name__ == '__main__':
    main()
