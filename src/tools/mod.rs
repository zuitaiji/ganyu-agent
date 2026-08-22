//! 工具层（`ganyu tool ...`）：把原 Python 辅助脚本迁移为 Rust 实现。
//! 全部列表式调用外部进程（无 shell 解析），不含硬编码密钥。

pub mod diagram;
pub mod git_diff;
pub mod upper;

use crate::error::GanyuResult;

/// 入口：处理 `ganyu tool <subcommand> ...`。
pub async fn run_tool(args: &[String]) -> GanyuResult<()> {
    let sub = args.first().map(|s| s.as_str()).unwrap_or("");
    let rest = &args[1..];
    match sub {
        "upper" => upper::run(rest),
        "diagram" => diagram::run(rest),
        "git-diff" => git_diff::run_local(rest),
        "pr-diff" => git_diff::run_remote(rest).await,
        other => Err(crate::error::GanyuError::Forbidden(format!(
            "未知 tool 子命令：{other}（可用 upper/diagram/git-diff/pr-diff）"
        ))),
    }
}
