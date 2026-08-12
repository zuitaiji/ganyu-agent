//! 进程级文件系统沙箱（H3）。
//!
//! 设计取舍：
//! - 仅在 `sandbox` 特性 **且** Linux 上真正生效（Landlock ABI v1），对 `exec` 派生的
//!   子进程施加 FS 限制，把写权限收敛到沙箱根，同时放行系统库/解释器所需只读/执行路径。
//! - 非 Linux（如 Windows/macOS）或特性未开启时为**安全无操作**（`Ok(())`）——
//!   主隔离仍由 C3/C4 的文件沙箱兜底，跨平台可用。
//! - 由调用方在子进程的 `pre_exec` 中调用，只约束派生子进程，不波及 agent 主进程。
//!
//! 已知边界：Landlock 只管文件系统；完整隔离（syscall/seccomp、网络、内存）应叠加
//! Docker / gVisor 等强隔离，本模块定位为「轻量第一道防线」。

#[cfg(all(feature = "sandbox", target_os = "linux"))]
pub fn apply_fs_sandbox(root: &std::path::Path) -> std::io::Result<()> {
    use landlock::{ABI, AccessFs, PathBeneath, PathFd, Ruleset};

    let root_canon = std::fs::canonicalize(root)
        .or_else(|_| {
            std::fs::create_dir_all(root)?;
            std::fs::canonicalize(root)
        })?;

    // 子进程需要的最小只读/执行路径（运行解释器与动态链接库）。
    let read_exec: &[&str] = &[
        "/usr", "/lib", "/lib64", "/bin", "/etc", "/proc", "/dev",
    ];

    let mut ruleset = Ruleset::new(ABI::V1)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("{e:?}")))?;

    // 沙箱根：读写（允许子进程在沙箱内产出）。
    ruleset = ruleset
        .add_rule(PathBeneath::new(
            PathFd::new(&root_canon).map_err(|e| {
                std::io::Error::new(std::io::ErrorKind::Other, format!("{e:?}"))
            })?,
            AccessFs::from_write(true),
        )?)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("{e:?}")))?;

    for p in read_exec {
        if let Ok(canon) = std::fs::canonicalize(p) {
            if let Ok(fd) = PathFd::new(&canon) {
                if let Ok(r) = ruleset.add_rule(PathBeneath::new(fd, AccessFs::from_read(true))) {
                    ruleset = r;
                }
            }
        }
    }

    match ruleset.apply() {
        Ok(_) => Ok(()),
        // 内核不支持 Landlock 时优雅降级（不阻断业务）。
        Err((_, e)) => Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("landlock apply failed: {e:?}"),
        )),
    }
}

#[cfg(not(all(feature = "sandbox", target_os = "linux")))]
pub fn apply_fs_sandbox(_root: &std::path::Path) -> std::io::Result<()> {
    // 非 Linux 或未开启 sandbox 特性：安全无操作。
    Ok(())
}

#[cfg(all(test, feature = "sandbox", target_os = "linux"))]
mod tests {
    #[test]
    fn sandbox_applies_without_panic() {
        // 在支持 Landlock 的 Linux 上应成功；不支持则降级返回 Ok。
        let tmp = std::env::temp_dir().join("ganyu_sandbox_test");
        let _ = std::fs::create_dir_all(&tmp);
        let r = super::apply_fs_sandbox(&tmp);
        assert!(r.is_ok());
    }
}
