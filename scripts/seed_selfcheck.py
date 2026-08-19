#!/usr/bin/env python3
"""seed 自检：把 GANYU_UPDATE_SIGN_KEY 种子推导为公钥，与生产公钥对比。

用法（本机任意 python3，零第三方依赖）：
    python scripts/seed_selfcheck.py <64hex种子>

输出：
    derived pubkey: <32字节公钥hex>
    匹配生产公钥:   YES / NO

安全：只打印公钥（非机密），seed 本身不输出。
"""
import hashlib
import sys

# ---- 生产公钥（docs/update-signing.md 官方公钥，2026-08-18 轮换） ----
PROD_PUBKEY = "d2de2259cce226840e7acb743b89b98cf603d2781e7b1b5456855efe8bf02cec"

# ---- Ed25519 参数（RFC 8032） ----
P = 2**255 - 19
D = (-121665 * pow(121666, P - 2, P)) % P
I = pow(2, (P - 1) // 4, P)


def inv(x):
    return pow(x, P - 2, P)


def xrecover(y):
    xx = (y * y - 1) * inv(D * y * y + 1) % P
    x = pow(xx, (P + 3) // 8, P)
    if (x * x - xx) % P != 0:
        x = x * I % P
    if x & 1:
        x = P - x
    return x


By = 4 * inv(5) % P
Bx = xrecover(By)
B = (Bx, By)  # 基点


def edwards_add(p, q):
    x1, y1 = p
    x2, y2 = q
    x1y2 = x1 * y2 % P
    y1x2 = y1 * x2 % P
    y1y2 = y1 * y2 % P
    x1x2 = x1 * x2 % P
    dxxyy = D * x1x2 * y1y2 % P
    x3 = (x1y2 + y1x2) * inv(1 + dxxyy) % P
    y3 = (y1y2 + x1x2) * inv(1 - dxxyy) % P
    return (x3, y3)


def scalarmult(p, e):
    if e == 0:
        return (0, 1)
    q = scalarmult(p, e // 2)
    q = edwards_add(q, q)
    if e & 1:
        q = edwards_add(q, p)
    return q


def encodepoint(p):
    x, y = p
    yb = (y % P).to_bytes(32, "little")
    if x & 1:
        yb = yb[:31] + bytes([yb[31] | 0x80])
    return yb


def publickey(seed: bytes) -> bytes:
    h = hashlib.sha512(seed).digest()
    a = int.from_bytes(h[:32], "little")
    a &= (1 << 254) - 8  # clamp: 清低 3 位 + bit254/255
    a |= 1 << 254        # clamp: 置 bit254
    return encodepoint(scalarmult(B, a))


def main() -> int:
    if len(sys.argv) != 2:
        print("用法: python scripts/seed_selfcheck.py <64hex种子>")
        return 2
    seed_hex = sys.argv[1].strip().lower()
    if len(seed_hex) != 64:
        print(f"❌ 种子长度 {len(seed_hex)} ≠ 64（需 32 字节 = 64 位 hex）")
        return 1
    try:
        seed = bytes.fromhex(seed_hex)
    except ValueError:
        print("❌ 种子含非 hex 字符（只能 0-9a-f）")
        return 1
    pub = publickey(seed).hex()
    match = pub == PROD_PUBKEY
    print(f"derived pubkey: {pub}")
    print(f"生产公钥:       {PROD_PUBKEY}")
    print(f"匹配生产公钥:   {'YES ✅' if match else 'NO ❌'}")
    if match:
        print("→ 种子正确，可直接配置 GANYU_UPDATE_SIGN_KEY secret")
    else:
        print("→ 不是 2026-08-18 轮换的那对密钥（旧种子/记错/泄露作废的演示种子）")
    return 0 if match else 1


if __name__ == "__main__":
    # 内置自检：RFC 8032 测试向量 1（seed→pub 已知），保证脚本自身正确
    assert publickey(bytes.fromhex(
        "9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60"
    )).hex() == "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a", "内置向量校验失败"
    sys.exit(main())
