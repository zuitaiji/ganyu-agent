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

    // 若目标文件自身是符号链接，规范化后应仍在根内（防符号链接逃逸，F-11）。
    if let Ok(c) = resolved.canonicalize() {
        if !c.starts_with(&root_canon) {
            return Err(GanyuError::Forbidden(format!(
                "路径经符号链接逃逸沙箱根：{raw}"
            )));
        }
    }
    if !has_prefix(&resolved, &root_canon) {
        return Err(GanyuError::Forbidden(format!(
            "路径逃逸沙箱根：{raw} -> {}",
            resolved.display()
        )));
    }
    Ok(resolved)
}

/// 与 `Path::starts_with` 等价，但在此显式命名以强调“沙箱边界”语义（F-11）。
fn has_prefix(path: &Path, root: &Path) -> bool {
    path.starts_with(root)
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

/// C5：SSRF 防护（入口校验 + 返回已校验 IP 供连接层固定，防 DNS 重绑定）。
///
/// 在发起任何出站请求前调用。拒绝：
/// - 非 `http`/`https` 协议；
/// - 主机为空、含用户态信息（`@`）、或命中本机/内网/链路本地/云元数据域名；
/// - 主机解析出的任一 IP 落在私有/保留网段（含 169.254.169.254 云元数据）。
///
/// 返回 `(host, 已校验 IP 列表)`：调用方应把这些 IP 通过
/// reqwest `ClientBuilder::resolve(host, ip)` 固定到连接层，
/// 使连接不再重新解析 DNS —— 这是对 DNS 重绑定攻击的**连接层闭环**。
pub fn ssrf_guard_resolve(url: &str) -> GanyuResult<(String, Vec<IpAddr>)> {
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
    // 字面 IP（用户直接写 IP）→ 一律按严格规则拒绝内网/保留段（无代理可绕过语义）。
    let literal_ip = host.parse::<IpAddr>().is_ok();
    let mut verified: Vec<IpAddr> = Vec::with_capacity(candidates.len());
    for addr in &candidates {
        let ip: IpAddr = match addr.parse() {
            Ok(ip) => ip,
            Err(_) => {
                // 解析到的地址字符串本应是 IP；若异常则保守拒绝。
                return Err(GanyuError::Ssrf(format!("地址解析异常：{addr}")));
            }
        };
        // IPv4 内嵌 IPv6（mapped/compatible/6to4/NAT64 等）由 is_private_or_reserved
        // 统一解出并按 IPv4 规则判断（见 embedded_ipv4），杜绝 `::ffff:127.0.0.1` 等绕过。
        if literal_ip && is_private_or_reserved(ip) {
            return Err(GanyuError::Ssrf(format!(
                "拒绝字面内网/保留地址 {ip}（来自主机 {host}）"
            )));
        }
        // 代理 fake-ip 豁免：Clash 等代理把域名解析为 198.18.0.0/15 虚拟地址，
        // 连接实际经代理转发——该网段不视为内网攻击面，否则外网抓取全被误拦。
        if is_private_or_reserved(ip) && !is_fake_ip(ip) {
            return Err(GanyuError::Ssrf(format!(
                "拒绝内网/保留地址 {ip}（来自主机 {host}）"
            )));
        }
        verified.push(ip);
    }
    Ok((host, verified))
}

/// 入口校验（不返回 IP，兼容仅校验用途）。
pub fn ssrf_guard(url: &str) -> GanyuResult<()> {
    ssrf_guard_resolve(url).map(|_| ())
}

/// 从 IPv6 中解出内嵌的 IPv4 地址。覆盖：
/// - IPv4-mapped `::ffff:a.b.c.d`（RFC 4291，最常见）；
/// - IPv4-compatible `::a.b.c.d`（RFC 4291 已弃用，部分栈仍支持）；
/// - 6to4 `2002:V4ADDR::/32`（RFC 3056，隧道可解封装到内网 IPv4）；
/// - NAT64 `64:ff9b::/96`（RFC 6052，经网关翻译到 IPv4）。
/// 若命中以上任一前缀则返回解出的 IPv4，供 `is_private_or_reserved` 按 IPv4 规则判断。
fn embedded_ipv4(v6: &std::net::Ipv6Addr) -> Option<std::net::Ipv4Addr> {
    if let Some(v4) = v6.to_ipv4_mapped() {
        return Some(v4);
    }
    let s = v6.segments();
    // IPv4-compatible：前 96 位全 0 且低 32 位非 0（0 为本机 :: 表示，1 为 loopback ::1）
    if s[0] == 0 && s[1] == 0 && s[2] == 0 && s[3] == 0 && s[4] == 0 && s[5] == 0 {
        let lo = ((s[6] as u32) << 16) | (s[7] as u32);
        if lo != 0 && lo != 1 {
            return Some(std::net::Ipv4Addr::from(lo));
        }
        return None;
    }
    // 6to4：2002:V4ADDR:...（V4ADDR 为第 2、3 段）
    if s[0] == 0x2002 {
        return Some(std::net::Ipv4Addr::new(
            (s[1] >> 8) as u8,
            (s[1] & 0xff) as u8,
            (s[2] >> 8) as u8,
            (s[2] & 0xff) as u8,
        ));
    }
    // NAT64：64:ff9b::/96（低 32 位为 IPv4）
    if s[0] == 0x64 && s[1] == 0xff9b && s[2] == 0 && s[3] == 0 && s[4] == 0 && s[5] == 0 {
        return Some(std::net::Ipv4Addr::new(
            (s[6] >> 8) as u8,
            (s[6] & 0xff) as u8,
            (s[7] >> 8) as u8,
            (s[7] & 0xff) as u8,
        ));
    }
    None
}

/// 代理 fake-ip 虚拟地址段：IPv4 198.18.0.0/15、IPv6 fdfe:dcba:9876::/48（Clash 默认虚拟前缀）。
fn is_fake_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => {
            let o = v.octets();
            o[0] == 198 && o[1] == 18
        }
        IpAddr::V6(v) => {
            let s = v.segments();
            s[0] == 0xfdfe && s[1] == 0xdcba && s[2] == 0x9876
        }
    }
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
        IpAddr::V4(a) => is_private_v4(a),
        IpAddr::V6(a) => {
            if a.is_loopback() || a.is_unspecified() || a.is_unicast_link_local() {
                return true;
            }
            // IPv4 内嵌（mapped/compatible/6to4/NAT64）→ 解出后按 IPv4 规则判断。
            // 这是 SSRF 防绕过的关键：`::ffff:127.0.0.1` 等不落入任何 IPv6 保留段，
            // 但连接时会被栈翻译/解封装为内网 IPv4。
            if let Some(v4) = embedded_ipv4(&a) {
                return is_private_v4(v4);
            }
            let s = a.segments();
            // Teredo 隧道前缀 2001::/32（RFC 4380，可封装指向内网）
            if s[0] == 0x2001 && s[1] == 0 {
                return true;
            }
            // 文档段 2001:db8::/32（RFC 3849）
            if s[0] == 0x2001 && s[1] == 0xdb8 {
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

fn is_private_v4(a: std::net::Ipv4Addr) -> bool {
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

/// 把不可信外部数据（工具输出 / 其它 agent 产出 / 历史轨迹）包裹成显式边界，
/// 提示下游模型或流程将其视为“数据”而非“指令”，缓解提示注入（F-06）。
pub fn fence_untrusted(label: &str, content: &str) -> String {
    format!(
        "<<<BEGIN_UNTRUSTED_DATA[{label}]>>>\n{content}\n<<<END_UNTRUSTED_DATA[{label}]>>>"
    )
}

/// 把十六进制字符串解码为字节（用于 ed25519 公钥/签名的解析，R-1）。
/// 长度非偶数或含非 hex 字符返回 `None`。
pub fn decode_hex(s: &str) -> Option<Vec<u8>> {
    let s = s.trim();
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16))
        .collect::<Result<Vec<u8>, _>>()
        .ok()
}

/// 归档条目路径安全检查（自更新用，R-5 补强）：拒绝
/// - 空路径 / 含 NUL；
/// - 以 `/` 或 `\` 开头的绝对路径；
/// - 含 `..` 路径穿越；
/// - Windows 盘符绝对路径（`C:\…` / `C:/…`，`[A-Za-z]:` 开头）。
///
/// 仅允许相对、无穿越的条目（如 `ganyu-agent` 或 `bin/ganyu-agent`），
/// 配合 `tar -xzf … -C <bin_dir>` 即可杜绝"压缩包内恶意路径写出沙箱目录外"。
pub fn is_safe_archive_entry(entry: &str) -> bool {
    let t = entry.trim();
    if t.is_empty() || t.contains('\0') {
        return false;
    }
    if t.starts_with('/') || t.starts_with('\\') {
        return false;
    }
    if t.contains("..") {
        return false;
    }
    // Windows 盘符绝对路径：第 2 字符为 `:` 且第 1 字符是字母。
    if t.len() >= 2 && t.as_bytes()[1] == b':' && t.as_bytes()[0].is_ascii_alphabetic() {
        return false;
    }
    true
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
        // IPv4-mapped IPv6 不得绕过（::ffff:127.0.0.1 即 127.0.0.1）
        assert!(ssrf_guard("http://[::ffff:127.0.0.1]:8080/").is_err());
        assert!(ssrf_guard("http://[::ffff:169.254.169.254]/latest/").is_err());
        // IPv4-compatible（RFC 4291 弃用，部分栈仍支持连接）
        assert!(ssrf_guard("http://[::127.0.0.1]:8080/").is_err());
        // 6to4 隧道解封装到内网
        assert!(ssrf_guard("http://[2002:7f00:1::1]/").is_err());
        // NAT64 网关翻译到内网
        assert!(ssrf_guard("http://[64:ff9b::7f00:1]/").is_err());
        // Teredo 隧道前缀
        assert!(ssrf_guard("http://[2001::7f00:1]/").is_err());
        // 6to4 封装公网地址（2002:0101:0101::/32 = 1.1.1.1）→ 放行
        assert!(ssrf_guard("http://[2002:101:101::1]/").is_ok());
        // 使用公网 IP 字面量（无需 DNS），离线环境也可验证放行路径。
        assert!(ssrf_guard("http://1.1.1.1/").is_ok());
    }

    #[test]
    fn ssrf_resolve_returns_verified_ips() {
        // 公网字面 IP：返回 host 与校验通过的 IP 列表（供连接层 resolve 固定）。
        let (host, ips) = ssrf_guard_resolve("http://1.1.1.1:8080/path").unwrap();
        assert_eq!(host, "1.1.1.1");
        assert!(ips.contains(&"1.1.1.1".parse().unwrap()));
        // 内网一律不返回
        assert!(ssrf_guard_resolve("http://[::ffff:192.168.1.1]/").is_err());
    }

    #[test]
    fn archive_entry_rejects_traversal_and_abs() {
        // R-5：tar 条目安全检查——相对无穿越放行，绝对/穿越/盘符拒绝。
        assert!(is_safe_archive_entry("ganyu-agent"));
        assert!(is_safe_archive_entry("bin/ganyu-agent"));
        assert!(!is_safe_archive_entry("../etc/passwd"));
        assert!(!is_safe_archive_entry("/etc/passwd"));
        assert!(!is_safe_archive_entry("\\windows\\system32\\x"));
        assert!(!is_safe_archive_entry("C:\\windows\\system32\\x"));
        assert!(!is_safe_archive_entry("D:/tmp/x"));
        assert!(!is_safe_archive_entry(""));
    }

    #[test]
    fn decode_hex_roundtrip() {
        assert_eq!(decode_hex("deadBEEF").unwrap(), vec![0xde, 0xad, 0xbe, 0xef]);
        assert!(decode_hex("xyz").is_none());
        assert!(decode_hex("abc").is_none()); // 奇数长度
    }
}
