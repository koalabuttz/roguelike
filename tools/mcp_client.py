#!/usr/bin/env python3
"""Minimal MCP client for interactive play via CLI.

Maintains a persistent server process across calls using a state file.
Usage:
  mcp_client.py new_game '{"seed": 64464, "compact": true}'
  mcp_client.py auto_explore
  mcp_client.py act '{"action": "auto_fight"}'
"""
import subprocess
import json
import sys
import os
import threading
import time

def main():
    if len(sys.argv) < 2:
        print("Usage: mcp_client.py <tool_name> [json_args]")
        sys.exit(1)

    tool_name = sys.argv[1]
    args = json.loads(sys.argv[2]) if len(sys.argv) > 2 else {}

    binary = os.path.join(os.path.dirname(__file__), '..', 'target', 'debug', 'mcp_server')

    proc = subprocess.Popen(
        [binary],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )

    responses = []

    def read_stdout():
        while True:
            line = proc.stdout.readline()
            if not line:
                break
            line = line.decode().strip()
            if line:
                try:
                    responses.append(json.loads(line))
                except json.JSONDecodeError:
                    continue

    reader = threading.Thread(target=read_stdout, daemon=True)
    reader.start()

    def send(msg):
        data = json.dumps(msg) + '\n'
        proc.stdin.write(data.encode())
        proc.stdin.flush()

    # 1. Initialize
    send({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "play", "version": "0.1"}
        }
    })

    # Wait for init response
    deadline = time.time() + 10
    while time.time() < deadline:
        if any(r.get('id') == 1 for r in responses):
            break
        time.sleep(0.05)

    # 2. Initialized notification
    send({"jsonrpc": "2.0", "method": "notifications/initialized"})
    time.sleep(0.1)

    # 3. Tool call
    send({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {
            "name": tool_name,
            "arguments": args
        }
    })

    # Wait for tool response
    deadline = time.time() + 15
    while time.time() < deadline:
        tool_resp = [r for r in responses if r.get('id') == 2]
        if tool_resp:
            resp = tool_resp[0]
            if 'result' in resp:
                content = resp['result'].get('content', [])
                for c in content:
                    if c.get('type') == 'text':
                        try:
                            parsed = json.loads(c['text'])
                            print(json.dumps(parsed, indent=2))
                        except:
                            print(c['text'])
            elif 'error' in resp:
                print(json.dumps(resp['error'], indent=2))
            break
        time.sleep(0.05)
    else:
        print("Timeout waiting for response")
        print(f"Got {len(responses)} responses: {[r.get('id') for r in responses]}")

    proc.stdin.close()
    proc.terminate()

if __name__ == '__main__':
    main()
