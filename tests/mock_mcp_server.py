#!/usr/bin/env python3
"""Mock MCP server（stdio JSON-RPC 2.0）：用于 ganyu mcp 端到端验证。

支持：initialize / notifications/initialized / tools/list / tools/call。
工具：echo（回显 input）、calc（求简单算式）。
"""
import json
import sys
import re


def handle(method, params):
    if method == "initialize":
        return {
            "protocolVersion": "2024-11-05",
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "mock-mcp", "version": "0.1.0"},
        }
    if method == "tools/list":
        return {
            "tools": [
                {"name": "echo", "description": "回显输入", "inputSchema": {"type": "object"}},
                {"name": "calc", "description": "简单算式求值", "inputSchema": {"type": "object"}},
            ]
        }
    if method == "tools/call":
        name = params.get("name", "")
        args = params.get("arguments", {})
        inp = str(args.get("input", ""))
        if name == "echo":
            text = f"echo:{inp}"
        elif name == "calc":
            m = re.fullmatch(r"[\d+\-*/().\s]+", inp)
            if not m:
                text = "invalid"
            else:
                text = str(eval(inp))  # noqa: S307 mock only
        else:
            return {"content": [{"type": "text", "text": f"unknown tool {name}"}], "isError": True}
        return {"content": [{"type": "text", "text": text}]}
    return {"content": [{"type": "text", "text": f"unhandled {method}"}]}


def main():
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            msg = json.loads(line)
        except json.JSONDecodeError:
            continue
        method = msg.get("method", "")
        if "id" not in msg:
            continue  # notification
        result = handle(method, msg.get("params", {}))
        resp = {"jsonrpc": "2.0", "id": msg["id"], "result": result}
        sys.stdout.write(json.dumps(resp) + "\n")
        sys.stdout.flush()


if __name__ == "__main__":
    main()
