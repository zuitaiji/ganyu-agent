//! 供应链签名 / 种子自检（R-1 自有 Rust 端）。
//!
//! 等价替换 `scripts/sign-release.py` 与 `scripts/  seed_selfcheck.py`：
//! - `gen`       生成密钥对（一次性，私钥仅打印到 stdout，须立即存入 CI secret）
//! - `sign`      对发布资产签名（64 字节原始签名，无 ASCII armor）
//! - `pub`       从种子反推公钥（核对用）
//! - `verify`    本地复核签名（便于离线核对）
//! - `seed-check` 把种子推导为公钥并与生产公钥比对（等价 seed_selfcheck.py）
//!
//! 密码学：RFC 8032 Ed25519，使用 `ed25519-dalek`（标准实现，与旧 Python 端
//! `cryptography`、现有 Rust 验签端 ring 完全互通）。随机种子用 ring 的 SystemRandom。
//!
//! 安全约束：缺失密钥时非零退出（fail-closed）；私钥仅从 `GANYU_UPDATE_SIGN_KEY`
//! 或 `--key` 读取，绝不落盘/打印非必要位置。

use crate::error::{GanyuError, GanyuResult};

/// 生产公钥（公开值，来自 docs/update-sign 轮换，2026-08-18）。等价 seed_selfcheck.py 的 PROD_PUBKEY。
const PROD_PUBKEY: &str = "241db1db27d3c19c58df6a35de52a158080e310bdeb57c50ddca8e5c647b9ba4";

#[cfg(feature = "sign")]
use ed25519_dalek::{Signer, SigningKey, Verifier, VerifyingKey};

/// 从 32 字节 RFC 8032 种子推导 32 字节公钥（等价 cryptography 的 from_private_bytes）。
#[cfg(feature = "sign")]
fn pub_from_seed(seed: &[u8; 32]) -> [u8; 32] {
    SigningKey::from_bytes(seed).verifying_key().to_bytes()
}

#[cfg(feature = "sign")]
fn sign_msg(seed: &[u8; 32], msg: &[u8]) -> [u8; 64] {
    let sk = SigningKey::from_bytes(seed);
    sk.sign(msg).to_bytes()
}

#[cfg(feature = "sign")]
fn verify_msg(pubkey: &[u8; 32], msg: &[u8], sig: &[u8]) -> bool {
    let Ok(vk) = VerifyingKey::from_bytes(pubkey) else {
        return false;
    };
    vk.verify(msg, sig).is_ok()
}

/// 安全读取 32 字节种子（64 hex），来自 `--key` 或环境变量。
#[cfg(feature = "sign")]
fn load_seed(arg_key: Option<  &str>) -> GanyuResult<[u8; 32]> {
    let raw = arg_key
        .or_else(|| std::env::var("GANYU_UPDATE_SIGN_KEY").ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            GanyuError::Forbidden(
                "未提供签名私钥。请用 --key <64hex> 或设置环境变量 GANYU_UPDATE_SIGN_KEY（来自 CI secret）。".into(),
            )
        })?;
    if raw.len() != 64 {
        return Err(GanyuError::Forbidden(format!(
            "种子长度 {} ≠ 64（需 32 字节 = 64 位 hex）",
            raw.len()
        )));
    }
    let bytes = decode_hex(&raw).ok_or_else(|| GanyuError::Forbidden("种子含非 hex 字符（只能 0-9a-f）".into()))?;
    if bytes.len() != 32 {
        return Err(GanyuError::Forbidden("种子解码后非 32 字节".into()));
    }
    let mut seed = [0u8; 32];
    seed.copy_from_slice(&bytes);
    Ok(seed)
}

/// hex 解码（复用 security::decode_hex，避免重复实现）。
pub fn decode_hex(s: &str) -> Option<Vec<u8>> {
    crate::security::decode_hex(s)
}

#[cfg(feature = "sign")]
fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

#[cfg(feature = "sign")]
fn keypair_gen() -> GanyuResult<()> {
    use ring::rand::{fill, SystemRandom};
    let rng = SystemRandom::new();
    let mut seed = [0u8; 32];
    fill(&rng, &mut seed).map_err(|_| GanyuError::Forbidden("随机数生成失败".into()))?;
    let sk = SigningKey::from_bytes(&seed);
    let pubkey = sk.verifying_key().to_bytes();
    let mut out = std::io::stdout();
    out.write_all("# --- 密钥对（仅生成一次，seed 仅存 CI secret）---\n".as_bytes())
        .ok();
    writeln!(out, "GANYU_UPDATE_SIGN_KEY={}   # 机密：仅存 CI secret，勿提交", hex(&seed)).ok();
    writeln!(out, "GANYU_UPDATE_PUBKEY ={}    # 公开：写入文档与用户环境", hex(&pubkey)).ok();
    Ok(())
}

#[cfg(feature = "sign")]
fn cmd_pub(key: Option<&str>) -> GanyuResult<()> {
    let seed = load_seed(key)?;
    let pubkey = pub_from_seed(&seed);
    println!("{}", hex(&pubkey));
    Ok(())
}

#[cfg(feature = "sign")]
fn cmd_sign(key: Option<&str>, file: Option<&str>) -> GanyuResult<()> {
    let seed = load_seed(key)?;
    let file = file.ok_or_else(|| GanyuError::Forbidden("sign 需要 <file> 参数".into()))?;
    let msg = std::fs::read(file)
        .map_err(|e| GanyuError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
    let sig = sign_msg(&seed, &msg);
    let sig_path = format!("{file}.sig");
    std::fs::write(&sig_path, &sig)
        .map_err(|e| GanyuError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
    // 签名后随即用同一公钥自验（捕获密钥/编码错误，fail-closed）。
    let pubkey = pub_from_seed(&seed);
    if !verify_msg(&pubkey, &msg, &sig) {
        return Err(GanyuError::Forbidden("自验失败：签名后立即复核未通过，拒绝产出".into()));
    }
    println!("已签名: {sig_path} ({} 字节)", sig.len());
    Ok(())
}

#[cfg(feature = "sign")]
fn cmd_verify(file: Option<&str>, pub_hex: Option<&str>) -> GanyuResult<()> {
    let file = file.ok_or_else(|| GanyuError::Forbidden("verify 需要 <file> 参数".into()))?;
    let pub_hex = pub_hex.ok_or_else(|| GanyuError::Forbidden("verify 需要 <pub-hex> 参数".into()))?;
    let pubkey = decode_hex(pub_hex).ok_or_else(|| GanyuError::Forbidden("公钥 hex 非法".into()))?;
    if pubkey.len() != 32 {
        return Err(GanyuError::Forbidden("公钥必须为 32 字节".into()));
    }
    let mut p = [0u8; 32];
    p.copy_from_slice(&pubkey);
    let msg = std::fs::read(file)
        .map_err(|e| GanyuError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
    let sig_path = format!("{file}.sig");
    let sig = std::fs::read(&sig_path)
        .map_err(|e| GanyuError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
    if verify_msg(&p, &msg, &sig) {
        println!("OK: {file} 签名有效");
        Ok(())
    } else {
        Err(GanyuError::Forbidden("签名校验失败".into()))
    }
}

#[cfg(feature = "sign")]
fn cmd_seed_check(seed_hex: Option<&str>) -> GanyuResult<()> {
    let seed_hex = seed_hex.ok_or_else(|| GanyuError::Forbidden("seed-check 需要 <64hex种子>".into()))?;
    if seed_hex.len() != 64 {
        return Err(GanyuError::Forbidden(format!("种子长度 {} ≠ 64", seed_hex.len())));
    }
    let bytes = decode_hex(seed_hex).ok_or_else(|| GanyuError::Forbidden("种子含非 hex 字符".into()))?;
    if bytes.len() != 32 {
        return Err(GanyuError::Forbidden("种子非 32 字节".into()));
    }
    let mut seed = [0u8; 32];
    seed.copy_from_slice(&bytes);
    let pubkey = pub_from_seed(&seed);
    let derived = hex(&pubkey);
    let match_prod = derived == PROD_PUBKEY;
    println!("derived pubkey: {derived}");
    println!("生产公钥:       {PROD_PUBKEY}");
    println!("匹配生产公钥:   {}", if match_prod { "YES" } else { "NO" });
    if match_prod {
        println!("→ 种子正确，可直接配置 GANYU_UPDATE_SIGN_KEY secret");
    } else {
        println!("→ 不是 2026-08-18 轮换的那对密钥（旧种子/记错/泄露作废的演示种子）");
    }
    if match_prod { Ok(()) } else { Err(GanyuError::Forbidden("种子与生产公钥不匹配".into())) }
}

/// 入口：处理 `ganyu release <subcommand> ...`。
#[cfg(feature = "sign")]
pub fn run_release(args: &[String]) -> GanyuResult<()> {
    // 内置自检断言（RFC 8032 测试向量 1），保证 seed→pub 推导正确（与 cryptography/dalek 一致）。
    let tv = [
        0x9d, 0x61, 0xb1, 0x9d, 0xef, 0xfd, 0x5a, 0x60, 0xba, 0x84, 0x4a, 0xf4, 0x92, 0xec, 0x2c,
        0xc4, 0x44, 0x49, 0xc5, 0x69, 0x7b, 0x32, 0x69, 0x19, 0x70, 0x3b, 0xac, 0x03, 0x1c, 0xae,
        0x7f, 0x60,
    ];
    let mut seed = [0u8; 32];
    seed.copy_from_slice(&tv);
    let expected = "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a";
    assert_eq!(hex(&pub_from_seed(&seed)), expected, "内置向量校验失败");

    let sub = args.first().map(|s| s.as_str()).unwrap_or("");
    let rest = &args[1..];
    match sub {
        "gen" => keypair_gen(),
        "pub" => cmd_pub(rest.first().map(|s| s.as_str())),
        "sign" => cmd_sign(rest.first().map(|s| s.as_str()), rest.get(1).map(|s| s.as_str())),
        "verify" => cmd_verify(rest.first().map(|s| s.as_str()), rest.get(1).map(|s| s.as_str())),
        "seed-check" => cmd_seed_check(rest.first().map(|s| s.as_str())),
        other => Err(GanyuError::Forbidden(format!(
            "未知 release 子命令：{other}（可用 gen/pub/sign/verify/seed-check）"
        ))),
    }
}

#[cfg(not(feature = "sign"))]
pub fn run_release(_args: &[String]) -> GanyuResult<()> {
    Err(GanyuError::Forbidden(
        "release 签名工具未启用，请用 --features sign 或 --features hardened 编译。".into(),
    ))
}
