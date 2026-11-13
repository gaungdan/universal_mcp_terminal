#!/usr/bin/env python3
"""Telnet 设备模拟器（仅用于联调，不参与生产运行）。

模拟一台网络设备的登录、提示符、命令回显与分页输出，用于在没有真实硬件时
验证 Universal MCP Terminal 的 Telnet Provider 端到端链路。

用法：
    python scripts/telnet_device_sim.py --port 2323 --user admin --password admin
"""

from __future__ import annotations

import argparse
import asyncio
import sys

IAC = 255
DONT, DO, WONT, WILL = 254, 253, 252, 251


def strip_iac(data: bytes) -> bytes:
    """丢弃 IAC 协商字节，只保留业务数据。"""
    out = bytearray()
    i = 0
    while i < len(data):
        if data[i] == IAC and i + 1 < len(data):
            command = data[i + 1]
            if command in (DO, DONT, WILL, WONT) and i + 2 < len(data):
                i += 3
                continue
            i += 2
            continue
        out.append(data[i])
        i += 1
    return bytes(out)


async def read_line(reader: asyncio.StreamReader) -> str:
    raw = await reader.readline()
    return strip_iac(raw).decode("utf-8", errors="replace").strip("\r\n")


async def handle(reader: asyncio.StreamReader, writer: asyncio.StreamWriter, user: str, password: str) -> None:
    peer = writer.get_extra_info("peername")
    print(f"[sim] connection from {peer}", flush=True)

    def send(text: str) -> None:
        writer.write(text.encode())

    try:
        send("UMT Telnet Device Simulator\r\nUsername: ")
        await writer.drain()
        given_user = await read_line(reader)
        send("Password: ")
        await writer.drain()
        given_password = await read_line(reader)

        if given_user != user or given_password != password:
            send("\r\nLogin invalid\r\n")
            await writer.drain()
            return

        send("\r\nWelcome to UMT-SIM\r\nadmin> ")
        await writer.drain()

        while True:
            line = await read_line(reader)
            command = line.strip()
            if not command:
                send("admin> ")
                await writer.drain()
                continue
            if command in ("exit", "quit", "logout"):
                send("\r\nBye\r\n")
                await writer.drain()
                return
            if command == "show version":
                send(
                    "UMT Simulator OS version 1.0.0\r\n"
                    "Build: integration-test\r\n"
                    "Uptime: 1 day, 02:03:04\r\n"
                    "admin> "
                )
                await writer.drain()
                continue
            if command == "show running-config":
                lines = [f"interface GigabitEthernet0/{i} description uplink-{i}" for i in range(1, 61)]
                for index, text in enumerate(lines):
                    send(text + "\r\n")
                    if index % 20 == 19:
                        send("--More--")
                        await writer.drain()
                        await reader.read(1)  # 等待空格翻页
                send("\r\nadmin> ")
                await writer.drain()
                continue
            send(f"% Invalid command: {command}\r\nadmin> ")
            await writer.drain()
    except (asyncio.IncompleteReadError, ConnectionResetError):
        pass
    finally:
        print(f"[sim] connection closed {peer}", flush=True)
        writer.close()
        try:
            await writer.wait_closed()
        except Exception:
            pass


async def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=2323)
    parser.add_argument("--user", default="admin")
    parser.add_argument("--password", default="admin")
    args = parser.parse_args()

    server = await asyncio.start_server(
        lambda r, w: handle(r, w, args.user, args.password),
        args.host,
        args.port,
    )
    print(f"[sim] listening on {args.host}:{args.port} (user={args.user})", flush=True)
    async with server:
        await server.serve_forever()


if __name__ == "__main__":
    try:
        asyncio.run(main())
    except KeyboardInterrupt:
        sys.exit(0)
