#!/usr/bin/env python3
"""
ganyu-agent 发布签名工具（R-1 供应链强校验的「签名端」）

与 src/main.rs 中的验签端（ring::signature::ED25519）严格对齐：
  - 算法：标准 RFC 8032 Ed25519（ring 与 cryptography 实现互通）
  - 私钥：32 字节原始种子（seed），以 hex 形式经 GANYU_UPDATE_SIGN_KEY 传入
  - 公钥：32 字节原始公钥，以 hex 形式写入 GANYU_UPDATE_PUBKEY / 文档
  - 签名：对「tar.gz 原始字节」签名，输出 <file>.sig（原始 64 字节，无封装/无 ASCII armor）

用法：
  # 1) 生成密钥对（仅做一次，把 seed 存进 CI secret，pub 写进文档）
  python scripts/sign-release.py gen

  # 2) 对发布资产签名（CI 中由 release.yml 调用）
  GANYU_UPDATE_SIGN_KEY=<64hex> python scripts/sign-release.py sign dist/ganyu-agent-linux-x86_64.tar.gz
  # → 生成 dist/ganyu-agent-linux-x86_64.tar.gz.sig（原始 64 字节）

  # 3) 由 seed 反推公钥（核对用）
  python scripts/sign-release.py pub <64hex-seed>

  # 4) 本地复核签名（不依赖 rust）
  python scripts/sign-release.py verify dist/ganyu-agent-linux-x86_64.tar.gz <64hex-pub>

安全约束：
  - 缺失密钥时以非零码退出（fail-closed，绝不产出「未签名」资产）。
  - 签名后随即用同一公钥自验，捕获密钥/编码错误。
  - seed 属机密，只进 CI secret，绝不打印到日志固定位置或提交仓库。
"""

import argparse
import hashlib
import os
import sys

try:
    from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
    from cryptography.hazmat.primitives.serialization import (
        Encoding,
        PrivateFormat,
        PublicFormat,
        NoEncryption,
    )
except ImportError:
    sys.stderr.write(
        "[fatal] 缺少依赖 cryptography：请先 `python -m pip install cryptography`\n"
    )
    sys.exit(3)


def _seed_from_arg_or_env(hex_seed: str | None) -> bytes:
    raw = hex_seed or os.environ.get("GANYU_UPDATE_SIGN_KEY", "")
    raw = raw.strip()
    if not raw:
        sys.stderr.write(
            "[fatal] 未提供签名私钥。请用 --key <64hex> 或设置环境变量 "
            "GANYU_UPDATE_SIGN_KEY（来自 CI secret，32 字节种子的 hex）。\n"
        )
        sys.exit(2)
    try:
        seed = bytes.fromhex(raw)
    except ValueError:
        sys.stderr.write("[fatal] GANYU_UPDATE_SIGN_KEY 不是合法 hex。\n")
        sys.exit(2)
    if len(seed) != 32:
        sys.stderr.write(
            f"[fatal] Ed25519 种子必须为 32 字节，收到 {len(seed)} 字节。\n"
        )
        sys.exit(2)
    return seed


def _priv_from_seed(seed: bytes) -> Ed25519PrivateKey:
    return Ed25519PrivateKey.from_private_bytes(seed)


def cmd_gen(_args: argparse.Namespace) -> int:
    priv = Ed25519PrivateKey.generate()
    seed = priv.private_bytes(Encoding.Raw, PrivateFormat.Raw, NoEncryption())
    pub = priv.public_key().public_bytes(Encoding.Raw, PublicFormat.Raw)
    sys.stdout.write("# --- 密钥对（仅生成一次）---\n")
    sys.stdout.write(f"GANYU_UPDATE_SIGN_KEY={seed.hex()}   # 机密：仅存 CI secret，勿提交\n")
    sys.stdout.write(f"GANYU_UPDATE_PUBKEY ={pub.hex()}    # 公开：写入文档与用户环境\n")
    return 0


def cmd_pub(args: argparse.Namespace) -> int:
    seed = _seed_from_arg_or_env(args.key)
    priv = _priv_from_seed(seed)
    pub = priv.public_key().public_bytes(Encoding.Raw, PublicFormat.Raw)
    sys.stdout.write(f"{pub.hex()}\n")
    return 0


def cmd_sign(args: argparse.Namespace) -> int:
    seed = _seed_from_arg_or_env(args.key)
    priv = _priv_from_seed(seed)
    pub = priv.public_key().public_bytes(Encoding.Raw, PublicFormat.Raw)

    path = args.file
    if not os.path.isfile(path):
        sys.stderr.write(f"[fatal] 待签名文件不存在：{path}\n")
        sys.exit(2)

    with open(path, "rb") as fh:
        data = fh.read()

    sig = priv.sign(data)
    assert len(sig) == 64, "ed25519 签名必须为 64 字节"

    # 自验：捕获密钥/编码错误，避免上传坏签名
    try:
        priv.public_key().verify(sig, data)
    except Exception as exc:  # noqa: BLE001
        sys.stderr.write(f"[fatal] 签名自验失败：{exc}\n")
        sys.exit(1)

    sig_path = f"{path}.sig"
    with open(sig_path, "wb") as fh:
        fh.write(sig)

    sha = hashlib.sha256(data).hexdigest()
    sys.stdout.write(
        f"[sign] {os.path.basename(path)} ({len(data)} bytes, sha256={sha[:16]}…)\n"
    )
    sys.stdout.write(f"[sign] 公钥(hex)={pub.hex()}\n")
    sys.stdout.write(f"[sign] 已写出 {sig_path}（原始 64 字节 ed25519 签名）✅\n")
    return 0


def cmd_verify(args: argparse.Namespace) -> int:
    pub_hex = args.pubkey.strip()
    try:
        pub = bytes.fromhex(pub_hex)
    except ValueError:
        sys.stderr.write("[fatal] 公钥 hex 非法。\n")
        sys.exit(2)
    if len(pub) != 32:
        sys.stderr.write(f"[fatal] 公钥应为 32 字节，收到 {len(pub)} 字节。\n")
        sys.exit(2)

    sig_path = f"{args.file}.sig"
    if not os.path.isfile(args.file) or not os.path.isfile(sig_path):
        sys.stderr.write(f"[fatal] 缺少文件或签名：{args.file} / {sig_path}\n")
        sys.exit(2)

    with open(args.file, "rb") as fh:
        data = fh.read()
    with open(sig_path, "rb") as fh:
        sig = fh.read()

    if len(sig) != 64:
        sys.stderr.write(f"[fatal] 签名长度异常：{len(sig)} 字节（应为 64）。\n")
        sys.exit(1)

    from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey

    try:
        Ed25519PublicKey.from_public_bytes(pub).verify(sig, data)
    except Exception:  # noqa: BLE001
        sys.stderr.write("[fatal] 签名校验失败 ❌\n")
        sys.exit(1)

    sys.stdout.write("[verify] 签名有效 ✅\n")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description="ganyu-agent 发布签名工具 (Ed25519)")
    sub = parser.add_subparsers(dest="cmd", required=True)

    sub.add_parser("gen", help="生成 Ed25519 密钥对（seed + pub，hex）")

    p_pub = sub.add_parser("pub", help="由 seed 反推公钥 hex")
    p_pub.add_argument("key", nargs="?", help="32 字节 seed 的 hex（缺省读 GANYU_UPDATE_SIGN_KEY）")

    p_sign = sub.add_parser("sign", help="对文件签名，输出 <file>.sig（原始 64 字节）")
    p_sign.add_argument("file", help="待签名的发布资产（tar.gz）")
    p_sign.add_argument("--key", help="32 字节 seed 的 hex（缺省读 GANYU_UPDATE_SIGN_KEY）")

    p_ver = sub.add_parser("verify", help="本地复核签名")
    p_ver.add_argument("file", help="已签名的文件（自动找 <file>.sig）")
    p_ver.add_argument("pubkey", help="32 字节公钥 hex")

    args = parser.parse_args()
    handlers = {
        "gen": cmd_gen,
        "pub": cmd_pub,
        "sign": cmd_sign,
        "verify": cmd_verify,
    }
    return handlers[args.cmd](args)


if __name__ == "__main__":
    raise SystemExit(main())
