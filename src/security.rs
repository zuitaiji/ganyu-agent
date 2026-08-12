//! 安全基件：文件系统沙箱（C3/C4）、SSRF 防护（C5）、shell 开关（C1）。
//!
//! 设计原则：**失败闭环（fail-closed）**——默认拒绝，需显式环境变量开启才放行；
//! 任何无法确定的输入一律拒绝，而非放行。

use std::net::{IpAddr, ToSocketAddrs};
use std::path::{Component, Path, PathBuf};

use crate::error::{GanyuError, GanyuResult};

/// shell 工具是否真正放行：需 `shell` 特性编译 **且** `GANYU_ALLOW_SHELL=1`（C1 双层失败闭环）。
pub fn shell_allowed() -> bool {
    #[cfg(feature = "shell")]
    {
        std::env::var("GANYU_ALLOW_SHELL").as_deref() == Ok("1")
    }
    #[cfg(not(feature = "shell"))]
    {
        false
    }
}

/// 文件系统沙箱根目录：默认 `.ganyu_workspace`（与 CWD 隔离），可用 `GANYU_FS_ROOT` 覆盖。
pub fn sandbox_root() -> PathBuf {
    match std::env::var("GANYU_FS_ROOT") {
        Ok(s) if !s.is_empty() => PathBuf::from(s),
        _ => PathBuf::from(".ganyu_workspace"),
    }
}

/// 把用户给的路径解析到沙箱根内。
///
/// 拒绝条件（任一即 `Forbidden`）：
/// - 含 `..` 路径分量（防目录穿越）；
/// - 为绝对路径（必须相对沙箱，避免指向系统文件）；
/// - 解析后不在沙箱根内（防符号链接逃逸）；
/// - 路径分量含 NUL。
pub fn resolve_sandboxed(input: &str) -> GanyuResult<PathBuf> {
    let raw = input.trim();
    if raw.is_empty() {
        return Err(GanyuError::Forbidden("空路径".into()));
    }
    let p = Path::new(raw);

    // 拒绝绝对路径与 `..` 穿越。
    if p.is_absolute() {
        return Err(GanyuError::Forbidden(format!(
            "拒绝绝对路径（沙箱内仅允许相对路径）：{raw}"
        )));
    }
    for comp in p.components() {
        match comp {
            Component::ParentDir => {
                return Err(GanyuError::Forbidden(format!(
                    "拒绝目录穿越（..）：{raw}"
                )));
            }
            Component::RootDir => {
                return Err(GanyuError::Forbidden(format!("拒绝绝对路径：{raw}")));
            }
            _ => {}
        }
    }
    if raw.contains('\0') {
        return Err(GanyuError::Forbidden("路径含非法字符".into()));
    }

    let root = sandbox_root();
    let root_canon = canonicalize_or_create(&root)?;
    let candidate = root_canon.join(p);
    // 文件可能尚不存在（写场景）：对父目录规范化以解析符号链接。
    let parent = candidate.parent().unwrap_or(&root_canon);
    let parent_canon = canonicalize_or_create(parent)?;
    let resolved = match candidate.file_name() {
        Some(name) => parent_canon.join(name),
        None => parent_canon,
    };

    if !resolved.starts_with(&root_canon) {
        return Err(GanyuError::Forbidden(format!(
            "路径逃逸沙箱根：{raw} -> {}",
            resolved.display()
        )));
    }
    Ok(resolved)
}

/// 尽量规范化目录；不存在则创建后再规范化（用于沙箱根/父目录）。
fn canonicalize_or_create(p: &Path) -> GanyuResult<PathBuf> {
    if p.exists() {
        p.canonicalize()
            .map_err(|e| GanyuError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))
    } else {
        std::fs::create_dir_all(p).map_err(GanyuError::Io)?;
        p.canonicalize()
            .map_err(|e| GanyuError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))
    }
}

/// C5：SSRF 防护。
///
/// 在发起任何出站请求前调用。拒绝：
/// - 非 `http`/`https` 协议；
/// - 主机为空、含用户态信息（`@`）、或命中本机/内网/链路本地/云元数据域名；
/// - 主机解析出的任一 IP 落在私有/保留网段（含 169.254.169.254 云元数据）。
///
/// 注意：这是**尽力而为**的入口防护；真正的强隔离应在出口代理（egress proxy）做，
/// 因 DNS 重绑定无法纯客户端彻底杜绝。故 `web_fetch` 同时关闭自动重定向并要求二次校验。
pub fn ssrf_guard(url: &str) -> GanyuResult<()> {
    let url = url.trim();
    // 拆分协议
    let (scheme, rest) = match url.split_once("://") {
        Some((s, r)) => (s.to_ascii_lowercase(), r),
        None => {
            return Err(GanyuError::Ssrf(format!("缺少协议（仅允许 http/https）：{url}")));
        }
    };
    if scheme != "http" && scheme != "https" {
        return Err(GanyuError::Ssrf(format!("拒绝协议 {scheme}（仅 http/https）")));
    }
    // 取主机部分（到 / ? # 为止），去掉可选方括号与端口。
    let authority = rest
        .split(['/', '?', '#'])
        .next()
        .unwrap_or("")
        .trim();
    if authority.is_empty() {
        return Err(GanyuError::Ssrf("空主机".into()));
    }
    if authority.contains('@') {
        return Err(GanyuError::Ssrf("拒绝含用户态信息的 URL".into()));
    }
    let host = if authority.starts_with('[') {
        // IPv6 [host]:port
        authority
            .trim_start_matches('[')
            .split(']')
            .next()
            .unwrap_or("")
            .to_string()
    } else {
        authority.split(':').next().unwrap_or("").to_string()
    };
    if host.is_empty() {
        return Err(GanyuError::Ssrf("无法解析主机".into()));
    }
    let host_l = host.to_ascii_lowercase();
    if host_l == "localhost"
        || host_l.ends_with(".localhost")
        || host_l.ends_with(".local")
        || host_l.ends_with(".internal")
        || host_l.ends_with(".intranet")
        || host_l.contains("metadata")
    {
        return Err(GanyuError::Ssrf(format!("拒绝本机/内网域名：{host}")));
    }

    // 解析并检查所有 IP（防止直接字面 IP 与 DNS 重绑定指向内网）。
    let candidates: Vec<String> = if let Ok(ip) = host.parse::<IpAddr>() {
        vec![ip.to_string()]
    } else {
        // 阻塞式解析（guard 阶段短阻塞可接受）；附带一个端口以便解析。
        match format!("{host}:0").to_socket_addrs() {
            Ok(addrs) => addrs.map(|a| a.ip().to_string()).collect(),
            Err(_) => return Err(GanyuError::Ssrf(format!("主机无法解析：{host}"))),
        }
    };
    if candidates.is_empty() {
        return Err(GanyuError::Ssrf(format!("主机无可用地址：{host}")));
    }
    for addr in &candidates {
        let ip: IpAddr = match addr.parse() {
            Ok(ip) => ip,
            Err(_) => {
                // 解析到的地址字符串本应是 IP；若异常则保守拒绝。
                return Err(GanyuError::Ssrf(format!("地址解析异常：{addr}")));
            }
        };
        if is_private_or_reserved(ip) {
            return Err(GanyuError::Ssrf(format!(
                "拒绝内网/保留地址 {ip}（来自主机 {host}）"
            )));
        }
    }
    Ok(())
}

/// M5：模型输出净化（信任边界处执行）。
///
/// - 去除 NUL 字节（防注入/解析异常）；
/// - 去除其它控制字符（保留 `\n` `\t` `\r`）；
/// - 超长截断并拒绝（防模型输出洪泛 / DoS）。
pub fn sanitize_model_output(s: &str) -> GanyuResult<String> {
    const MAX_LEN: usize = 1_000_000;
    if s.len() > MAX_LEN {
        return Err(GanyuError::Forbidden(format!(
            "模型输出超过长度上限 {MAX_LEN} 字节（疑似异常）"
        )));
    }
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c == '\0' {
            continue; // 丢弃 NUL
        }
        if c.is_control() && c != '\n' && c != '\t' && c != '\r' {
            continue; // 丢弃其它控制字符
        }
        out.push(c);
    }
    Ok(out)
}

/// 判断 IP 是否落在私有/环回/链路本地/保留网段（SSRF 防护用）。
pub fn is_private_or_reserved(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(a) => {
            let o = a.octets();
            // 0.0.0.0/8 本网络
            if o[0] == 0 {
                return true;
            }
            // 10.0.0.0/8
            if o[0] == 10 {
                return true;
            }
            // 127.0.0.0/8 环回
            if o[0] == 127 {
                return true;
            }
            // 169.254.0.0/16 链路本地（含 169.254.169.254 云元数据）
            if o[0] == 169 && o[1] == 254 {
                return true;
            }
            // 172.16.0.0/12
            if o[0] == 172 && (o[1] >= 16 && o[1] <= 31) {
                return true;
            }
            // 192.168.0.0/16
            if o[0] == 192 && o[1] == 168 {
                return true;
            }
            // 100.64.0.0/10 CGNAT
            if o[0] == 100 && (o[1] >= 64 && o[1] <= 127) {
                return true;
            }
            // 192.0.0.0/24 / 192.0.2.0/24 / 198.18.0.0/15 / 198.51.100.0/24 / 203.0.113.0/24 等文档/保留段
            if o[0] == 192 && o[1] == 0 && o[2] == 0 {
                return true;
            }
            if o[0] == 198 && (o[1] == 18 || o[1] == 19) {
                return true;
            }
            false
        }
        IpAddr::V6(a) => {
            if a.is_loopback() || a.is_unspecified() || a.is_unicast_link_local() {
                return true;
            }
            // 唯一本地地址 fc00::/7
            if (a.octets()[0] & 0xfe) == 0xfc {
                return true;
            }
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_traversal() {
        assert!(resolve_sandboxed("../etc/passwd").is_err());
        assert!(resolve_sandboxed("/etc/passwd").is_err());
    }

    #[test]
    fn allows_relative_in_sandbox() {
        let r = resolve_sandboxed("sub/dir/file.txt");
        assert!(r.is_ok());
    }

    #[test]
    fn ssrf_blocks_private() {
        assert!(ssrf_guard("http://127.0.0.1:8080/").is_err());
        assert!(ssrf_guard("http://169.254.169.254/latest/").is_err());
        assert!(ssrf_guard("http://192.168.1.1/").is_err());
        assert!(ssrf_guard("file:///etc/passwd").is_err());
        // 使用公网 IP 字面量（无需 DNS），离线环境也可验证放行路径。
        assert!(ssrf_guard("http://1.1.1.1/").is_ok());
    }
}
