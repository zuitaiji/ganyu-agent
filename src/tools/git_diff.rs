//! `ganyu tool git-diff` / `ganyu tool pr-diff`：获取 Git diff 用于代码 Review。
//! - `git-diff` 本地：等价于 `code-review-assistant/scripts/get_diff.py`（全程列表式 subprocess，无 shell）。
//! - `pr-diff` 远程：等价于 `code-review-assistant/scripts/get_pr_diff.py`（GitHub/GitLab，token 取自参数或环境变量）。
//! 远程部分依赖 reqwest，仅在 `network` 特性下启用（与 Rust 凭据基件一致）。

use crate::error::GanyuResult;

fn run_git(args: &[&str], cwd: &str) -> GanyuResult<String> {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                crate::error::GanyuError::Forbidden("未找到 git 命令，请确认已安装 Git".into())
            } else {
                crate::error::GanyuError::Io(e)
            }
        })?;
    if !output.status.success() {
        let msg = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(crate::error::GanyuError::Forbidden(format!(
            "git {} 失败: {}",
            args.join(" "),
            msg
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn print_stats(diff: &str) {
    let lines: Vec<&str> = diff.split('\n').collect();
    let added = lines.iter().filter(|l| l.starts_with('+') && !l.starts_with("+++")).count();
    let removed = lines.iter().filter(|l| l.starts_with('-') && !l.starts_with("---")).count();
    let files: Vec<&str> = lines
        .iter()
        .filter(|l| l.starts_with("+++ b/"))
        .map(|l| &l[6..])
        .collect();
    println!("变更统计: {} 个文件，+{} 行，-{} 行", files.len(), added, removed);
    if !files.is_empty() {
        println!("变更文件:");
        for f in files {
            println!("   - {f}");
        }
    }
}

/// `ganyu tool git-diff [--staged] [--branch <b>] [--commits <c>] [--file <f>] [--path <p>] [--stat]`
pub fn run_local(args: &[String]) -> GanyuResult<()> {
    let mut staged = false;
    let mut branch: Option<String> = None;
    let mut commits: Option<String> = None;
    let mut file: Option<String> = None;
    let mut path = ".".to_string();
    let mut stat = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--staged" => staged = true,
            "--branch" => {
                branch = args.get(i + 1).cloned();
                i += 1;
            }
            "--commits" => {
                commits = args.get(i + 1).cloned();
                i += 1;
            }
            "--file" => {
                file = args.get(i + 1).cloned();
                i += 1;
            }
            "--path" => {
                path = args.get(i + 1).cloned().unwrap_or_else(|| ".".to_string());
                i += 1;
            }
            "--stat" => stat = true,
            "--help" | "-h" => {
                println!("用法: ganyu tool git-diff [--staged] [--branch <b>] [--commits <c>] [--file <f>] [--path <p>] [--stat]");
                return Ok(());
            }
            other => {
                return Err(crate::error::GanyuError::Forbidden(format!(
                    "未知参数：{other}"
                )))
            }
        }
        i += 1;
    }

    // 仓库存在性校验（失败闭环：非仓库直接报错，不静默继续）。
    run_git(&["rev-parse", "--git-dir"], &path)?;

    let cur_branch = run_git(&["rev-parse", "--abbrev-ref", "HEAD"], &path)?.trim().to_string();
    let last_commit = run_git(&["log", "-1", "--format=%h %s", "--"], &path)?.trim().to_string();
    println!("仓库路径: {path}");
    println!("当前分支: {cur_branch}");
    println!("最新提交: {last_commit}");
    println!("{}", "-".repeat(60));

    let mut git_args: Vec<&str> = vec!["diff"];
    if stat {
        git_args.push("--stat");
    } else {
        git_args.push("--unified=3");
    }

    if staged {
        git_args.push("--cached");
        println!("模式: 已暂存的变更（staged）");
    } else if let Some(b) = &branch {
        git_args.push(b);
        git_args.push("HEAD");
        println!("模式: 与 {b} 分支的差异");
    } else if let Some(c) = &commits {
        git_args.push(c);
        git_args.push("HEAD");
        println!("模式: {c} 到 HEAD 的变更");
    } else {
        git_args.push("HEAD~1");
        git_args.push("HEAD");
        println!("模式: 最近一次提交的变更");
    }

    if let Some(f) = &file {
        git_args.push("--");
        git_args.push(f);
        println!("文件过滤: {f}");
    }

    println!("{}", "-".repeat(60));
    let out = run_git(&git_args, &path)?;
    if out.trim().is_empty() {
        println!("没有发现变更");
        return Ok(());
    }
    print_stats(&out);
    println!("{}", "-".repeat(60));
    println!("{out}");
    Ok(())
}

#[cfg(feature = "network")]
pub async fn run_remote(args: &[String]) -> GanyuResult<()> {
    use crate::error::GanyuError;

    let mut provider: Option<String> = None;
    let mut pr: Option<String> = None;
    let mut project: Option<String> = None;
    let mut url: Option<String> = None;
    let mut token: Option<String> = None;
    let mut base_url = "https://gitlab.com".to_string();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str()  {
            "--provider" => {
                provider = args.get(i + 1).cloned();
                i += 1;
            }
            "--pr" => {
                pr = args.get(i + 1).cloned();
                i += 1;
            }
            "--project" => {
                project = args.get(i + 1).cloned();
                i += 1;
            }
            "--url" => {
                url = args.get(i + 1).cloned();
                i += 1;
            }
            "--token" => {
                token = args.get(i + 1).cloned();
                i += 1;
            }
            "--base-url" => {
                base_url = args.get(i + 1).cloned().unwrap_or_else(|| "https://gitlab.com".to_string());
                i += 1;
            }
            "--help" | "-h" => {
                println!("用法: ganyu tool pr-diff --url <url> | (--provider github|gitlab --pr <n> [--project <p>] [--base-url <u>]) [--token <t>]");
                return Ok(());
            }
            other => {
                return Err(GanyuError::Forbidden(format!("未知参数：{other}")));
            }
        }
        i += 1;
    }

    let client = reqwest::Client::builder()
        .user_agent("ganyu-agent")
        .build()
        .map_err(|e| GanyuError::Http(e.to_string()))?;

    if let Some(u) = &url {
        if let Some((owner, repo, num)) = parse_github_url(u) {
            let tok = token.clone().or_else(|| std::env::var("GITHUB_TOKEN").ok());
            println!("GitHub PR: {owner}/{repo}#{num}");
            println!("{}", "-".repeat(60));
            let diff = get_github_diff(&client, &owner, &repo, &num, tok.as_deref()).await?;
            println!("{diff}");
            return Ok(());
        }
        if let Some((host, proj, num)) = parse_gitlab_url(u) {
            let tok = token.clone().or_else(|| std::env::var("GITLAB_TOKEN").ok());
            println!("GitLab MR: {proj}!{num}");
            println!("{}", "-".repeat(60));
            let diff = get_gitlab_diff(&client, &host, &proj, &num, tok.as_deref()).await?;
            println!("{diff}");
            return Ok(());
        }
        return Err(GanyuError::Forbidden(
            "无法解析 URL，请使用完整的 GitHub/GitLab PR/MR URL".into(),
        ));
    }

    match (provider.as_deref(), pr.clone()) {
        (Some("github"), Some(num)) => {
            let tok = token.clone().or_else(|| std::env::var("GITHUB_TOKEN").ok());
            println!("GitHub PR: #{num}");
            println!("{}", "-".repeat(60));
            // owner/repo 从当前仓库 origin 推断（可选）。
            let (owner, repo) = repo_from_remote()?;
            let diff = get_github_diff(&client, &owner, &repo, &num, tok.as_deref()).await?;
            println!("{diff}");
            Ok(())
        }
        (Some("gitlab"), Some(num)) => {
            let proj = project.clone().ok_or_else(|| {
                GanyuError::Forbidden("GitLab 请指定 --project group/project 或 --url".into())
            })?;
            let tok = token.clone().or_else(|| std::env::var("GITLAB_TOKEN").ok());
            println!("GitLab MR: {proj}!{num}");
            println!("{}", "-".repeat(60));
            let diff = get_gitlab_diff(&client, &base_url, &proj, &num, tok.as_deref()).await?;
            println!("{diff}");
            Ok(())
        }
        _ => Err(GanyuError::Forbidden(
            "请指定 --url 或 (--provider github|gitlab --pr <n>)".into(),
        )),
    }
}

#[cfg(feature = "network")]
async fn get_github_diff(
    client: &reqwest::Client,
    owner: &str,
    repo: &str,
    pr: &str,
    token: Option<&str>,
) -> GanyuResult<String> {
    use crate::error::GanyuError;
    let url = format!("https://api.github.com/repos/{owner}/{repo}/pulls/{pr}");
    let mut req = client.get(&url).header("Accept", "application/vnd.github.v3.diff");
    if let Some(t) = token {
        req = req.header("Authorization", format!("token {t}"));
    }
    let resp = req.send().await.map_err(|e| GanyuError::Http(e.to_string()))?;
    if !resp.status().is_success() {
        let code = resp.status().as_u16();
        let msg = match code {
            404 => "PR 不存在或无权限访问".to_string(),
            401 => "GitHub Token 无效或未设置".to_string(),
            _ => format!("HTTP {code}"),
        };
        return Err(GanyuError::Http(msg));
    }
    Ok(resp.text().await.map_err(|e| GanyuError::Http(e.to_string()))?)
}

#[cfg(feature = "network")]
async fn get_gitlab_diff(
    client: &reqwest::Client,
    base_url: &str,
    project: &str,
    mr: &str,
    token: Option<&str>,
) -> GanyuResult<String> {
    use crate::error::GanyuError;
    let proj_encoded: String = project.replace('/', "%2F");
    let url = format!("{base_url}/api/v4/projects/{proj_encoded}/merge_requests/{mr}/diffs");
    let mut req = client.get(&url);
    if let Some(t) = token {
        req = req.header("PRIVATE-TOKEN", t);
    }
    let resp = req.send().await.map_err(|e| GanyuError::Http(e.to_string()))?;
    if !resp.status().is_success() {
        let code = resp.status().as_u16();
        let msg = match code {
            404 => "MR 不存在或无权限访问".to_string(),
            401 => "GitLab Token 无效或未设置".to_string(),
            _ => format!("HTTP {code}"),
        };
        return Err(GanyuError::Http(msg));
    }
    let arr: serde_json::Value = resp.json().await.map_err(|e| GanyuError::Http(e.to_string()))?;
    let mut out = String::new();
    if let Some(items) = arr.as_array() {
        for d in items {
            if let Some(diff) = d.get("diff").and_then(|v| v.as_str()) {
                out.push_str(diff);
                out.push('\n');
            }
        }
    }
    Ok(out)
}

#[cfg(feature = "network")]
fn parse_github_url(u: &str) -> Option<(String, String, String)> {
    let re = regex::Regex::new(r"https://github\.com/([^/]+)/([^/]+)/pull/(\d+)").ok()?;
    let caps = re.captures(u)?;
    Some((
        caps.get(1)?.as_str().to_string(),
        caps.get(2)?.as_str().to_string(),
        caps.get(3)?.as_str().to_string(),
    ))
}

#[cfg(feature = "network")]
fn parse_gitlab_url(u: &str) -> Option<(String, String, String)> {
    let re =
        regex::Regex::new(r"https://([^/]+)/([^/]+)/([^/]+)/-/merge_requests/(\d+)").ok()?;
    let caps = re.captures(u)?;
    Some((
        format!("https://{}", caps.get(1)?.as_str()),
        format!("{}/{}", caps.get(2)?.as_str(), caps.get(3)?.as_str()),
        caps.get(4)?.as_str().to_string(),
    ))
}

#[cfg(feature = "network")]
fn repo_from_remote() -> GanyuResult<(String, String)> {
    use crate::error::GanyuError;
    let out = run_git(&["remote", "get-url", "origin"], ".").unwrap_or_default();
    let out = out.trim();
    // git@github.com:owner/repo.git 或 https://github.com/owner/repo.git
    let re = regex::Regex::new(r"(?:git@github\.com:|https://github\.com/)([^/]+)/([^/]+?)(?:\.git)?$")
        .map_err(|e| GanyuError::Regex(e))?;
    if let Some(c) = re.captures(out) {
        return Ok((
            c.get(1)
                .ok_or_else(|| GanyuError::Forbidden("无法解析仓库 owner".into()))?
                .as_str()
                .to_string(),
            c.get(2)
                .ok_or_else(|| GanyuError::Forbidden("无法解析仓库名".into()))?
                .as_str()
                .to_string(),
        ));
    }
    Err(GanyuError::Forbidden(
        "无法从当前目录获取 GitHub 仓库信息，请使用 --url 参数".into(),
    ))
}

#[cfg(not(feature = "network"))]
pub async fn run_remote(_args: &[String]) -> GanyuResult<()> {
    Err(crate::error::GanyuError::Forbidden(
        "pr-diff 需要 network 特性，请用 --features network/hardened 编译。".into(),
    ))
}
