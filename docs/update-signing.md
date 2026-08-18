# 自更新签名（R-1 供应链强校验）

`ganyu-agent` 的 `self-update` 从 GitHub Release 下载三平台 `tar.gz` 并落地。
为防止「发布服务器/账号被接管」后投毒，下载的资产在 sha256 比对**之前**先用
固定 ed25519 公钥验签（`src/main.rs` 的 `verify_update_signature`，基于 `ring::ED25519`）。

- 验签端：Rust `ring::signature::ED25519`（标准 RFC 8032 Ed25519），挂在 `network` 特性下。
- 签名端：`scripts/sign-release.py`（Python `cryptography`），CI 在发版时对每个资产签名。
- 契约：公钥 = 32 字节原始公钥（hex，来自 `GANYU_UPDATE_PUBKEY`）；
  签名 = `<资产名>.sig`，**原始 64 字节**；校验对象 = 下载的 `tar.gz` 原始字节。

---

## 1. 官方发布公钥（请核对后写入环境）

```
GANYU_UPDATE_PUBKEY=d2de2259cce226840e7acb743b89b98cf603d2781e7b1b5456855efe8bf02cec
```

> ✅ 这是 2026-08-18 轮换后的**生产公钥**（由 `scripts/sign-release.py gen` 生成）。
> 配套私钥种子 `GANYU_UPDATE_SIGN_KEY` **仅存在于 CI Secret**，绝不入库。
> 早期加固阶段生成的演示公钥 `3875bdb99b8fea88084baa75335660083903775f52969ff289efbbdf0c5afbd1`
> 已**作废**（种子曾在对话历史中明文出现，视为泄露），请勿使用。

用户启用强校验只需：

```bash
export GANYU_UPDATE_PUBKEY=d2de2259cce226840e7acb743b89b98cf603d2781e7b1b5456855efe8bf02cec
ganyu-agent update        # 自动下载 <url>.sig 并验签；缺失/不符则拒绝更新
```

未设置该变量时，`update` 仅做同源 sha256（防御纵深但**不能**防恶意发布方），并显式告警。

> **信任锚点**：不仅运行时 `ganyu update` 验签，**首次安装的 `install.sh` / `install.ps1`
> 也会在解包前下载 `<asset>.sig` 并用上面的官方公钥验签**（缺失或失败直接拒绝安装，
> fail-closed）。官方公钥已硬编码进两个安装脚本（可用 `GANYU_UPDATE_PUBKEY` 覆盖）。
> 这样整条供应链——从首次安装到后续自更新——都锚定在同一把公钥上。

---

## 2. 维护者：发版签名（CI 自动）

`.github/workflows/release.yml` 在每个平台 `build` job 里，于 `Archive`/`Checksum`
之后新增 `Sign` 步骤：

```bash
GANYU_UPDATE_SIGN_KEY=<32字节种子hex> python scripts/sign-release.py sign <资产>.tar.gz
# → 生成 <资产>.tar.gz.sig（原始 64 字节），随 .sha256 一起上传并发布
```

**前置：在仓库 `Settings → Secrets` 配置 `GANYU_UPDATE_SIGN_KEY`**
（= `scripts/sign-release.py gen` 输出的种子 hex）。

- 未配置该 Secret 时，`Sign` 步骤以 `::error::` 失败、**拒绝发布未签名资产**（fail-closed）。
- `release` job 通过 `softprops/action-gh-release` 把 `dist/*/*`（含 `.sig`）发布到 Release。
- 每次 `build` 的 `Test` 步骤（`cargo test --features hardened`）会跑
  `update_sig_interop_tests`，固定向量回归证明「Python 签名」能被「Rust 验签」接受。

---

## 3. 维护者：首次生成 / 轮换密钥

```bash
python -m pip install cryptography
python scripts/sign-release.py gen
# GANYU_UPDATE_SIGN_KEY=<种子hex>   # 机密：仅存 CI Secret
# GANYU_UPDATE_PUBKEY =<公钥hex>    # 公开：写进本文件 + 用户指引
```

1. 把新 `GANYU_UPDATE_SIGN_KEY` 写入仓库 Secret（替换旧的）。
2. 把新 `GANYU_UPDATE_PUBKEY` 更新到本文件 §1、用户指引，以及
   `install.sh` 的 `GANYU_OFFICIAL_PUBKEY` 与 `install.ps1` 的 `$OfficialPubKey`
   （两个安装脚本都已硬编码官方公钥作为信任锚点，轮换时必须同步改这 3 处）。
3. 打新 tag 发版：新资产用新密钥签名，旧资产仍在旧发布里（旧公钥已失效的话，旧发布应重新签名或归档）。

> 轮换窗口：新旧公钥并存期尽量短。若必须同时支持两把钥匙，可在 `verify_update_signature`
> 调用处做「多密钥试验」，但默认只信任一把——本实现当前为单固定公钥。

---

## 4. 本地复核签名（不依赖 Rust）

```bash
# 下载资产与签名
curl -LO https://github.com/<you>/ganyu-agent/releases/download/v0.1.0/ganyu-agent-linux-x86_64.tar.gz
curl -LO https://github.com/<you>/ganyu-agent/releases/download/v0.1.0/ganyu-agent-linux-x86_64.tar.gz.sig

python scripts/sign-release.py verify ganyu-agent-linux-x86_64.tar.gz \
  d2de2259cce226840e7acb743b89b98cf603d2781e7b1b5456855efe8bf02cec
# → [verify] 签名有效 ✅
```

---

## 5. 逃生舱口（不推荐）

环境变量 `GANYU_UPDATE_ALLOW_NOCHECK=1` 仅在**连 sha256 校验文件都拿不到**时，
允许跳过校验继续更新（用于完全离线/自托管场景）。它**不**绕过 ed25519 验签——
只要 `GANYU_UPDATE_PUBKEY` 已设置，签名校验失败仍会拒绝。请勿用于规避签名机制。
