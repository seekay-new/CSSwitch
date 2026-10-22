# 发布到 GitHub（源码 + Windows 便携包）

本文只讲**怎么把当前这份工作发出去**。许可证与归属先读 [NOTICE.md](../../NOTICE.md)：
本仓库是 [SuperJJ007/CSSwitch](https://github.com/SuperJJ007/CSSwitch) 的非官方 Windows 移植（MIT），
分发必须保留 `LICENSE` 的原始版权声明并说明改动。

---

## 0. 发布前检查清单

已在仓库内就绪的：

- [x] 无密钥入库：跟踪文件里只有测试用的假 key（`sk-aaaa…`）；`.gitignore` 覆盖 `dist/`、`target/`、
      `desktop/src-tauri/binaries/`、`.sandbox/`、`*token*`、`*secret*`
- [x] 本地绝对路径已泛化：`build.rs` 与 `test/win-oneclick-e2e.ps1` 改为 `WINDRES` /
      `CSSWITCH_MINGW_BIN` / `W64DEVKIT_HOME` / PATH 发现
- [x] `NOTICE.md`：上游归属、WebView2Loader.dll 再分发说明、与 Anthropic 无关联声明
- [x] `README.md` 的「功能现状（Windows x64）」把已验证 / 未验收 / 不支持分开写

需要你自己确认的：

- [ ] 你的使用方式与分发方式符合 Claude Science 与各家模型服务的条款
- [ ] **不要把 Claude Science 的 157 MB 安装程序上传**（不在仓库内，也不建议放进 Releases）
- [ ] Release 说明里写清「未签名」：Windows 首次运行会出现 SmartScreen 提示，
      用户需「更多信息 → 仍要运行」

---

## 1. 仓库怎么建（二选一）

### A. Fork 上游（推荐，归属最清楚）

1. 在 GitHub 打开 `https://github.com/SuperJJ007/CSSwitch` → **Fork**；
2. 本地把我们的提交推到你 fork 的分支上：

```powershell
cd C:\software\Claudescience\CSSwitch-0.8.4
git remote add origin https://github.com/<你的用户名>/CSSwitch.git
git remote add upstream https://github.com/SuperJJ007/CSSwitch.git
git checkout -b windows-x64
git push -u origin windows-x64
```

> 本仓库的历史是从上游快照重新起点的（8 个提交，无上游历史），所以推到 fork 时是新分支，
> 不要试图 `git push` 到 fork 的 `main`（会被拒绝）。要提 PR 给上游时，用这个分支即可。

### B. 新建独立仓库（最简单）

1. GitHub → **New repository**（建议命名 `CSSwitch-Windows`，描述里写明 “unofficial Windows port of SuperJJ007/CSSwitch”）；
2. 推送：

```powershell
cd C:\software\Claudescience\CSSwitch-0.8.4
git remote add origin https://github.com/<你的用户名>/CSSwitch-Windows.git
git branch -M main
git push -u origin main
```

首次推送前确认 `git status` 是干净的、`dist/` 没被跟踪：

```powershell
git status --short
git ls-files | Select-String -Pattern '^(dist/|target/|desktop/src-tauri/binaries/)'
```

---

## 2. 打 tag + 建 Release

```powershell
cd C:\software\Claudescience\CSSwitch-0.8.4

# 1) 重新打包（会读取 tauri.conf.json 的版本号）
powershell -ExecutionPolicy Bypass -File scripts\package-windows.ps1

# 2) 打 tag
git tag -a v0.8.4-win.1 -m "CSSwitch 0.8.4 Windows x64 portable (unofficial port)"
git push origin v0.8.4-win.1
```

上传资产（`gh` CLI 最省事；没有的话用网页 Releases → Draft a new release → 手动上传）：

```powershell
gh release create v0.8.4-win.1 `
  "dist\CSSwitch_0.8.4_x64-win-portable.zip" `
  --title "CSSwitch 0.8.4 · Windows x64（非官方移植）" `
  --notes-file docs\operations\release-notes-windows.md
```

Release 说明里至少写清：

- 适用：Windows 10 1809+ / 11 x64，需已安装 WebView2 Runtime（Win11 自带）
- 前置：自备 Claude Science + 模型 API Key；本包不含、不下载 Science
- 未签名：首次运行需 SmartScreen「仍要运行」
- 能力边界：指向 README 的「功能现状（Windows x64）」
- 校验：附 zip 的 SHA-256（`Get-FileHash dist\*.zip -Algorithm SHA256`）

---

## 3. 可选：GitHub Actions 自动构建

与本地验证环境保持一致（`x86_64-pc-windows-gnu`）。放进 `.github/workflows/windows.yml`：

```yaml
name: windows-portable
on:
  push:
    tags: ["v*"]
  workflow_dispatch:
jobs:
  build:
    runs-on: windows-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: x86_64-pc-windows-gnu
          toolchain: stable-x86_64-pc-windows-gnu
      - name: Install MinGW toolchain
        run: |
          choco install -y mingw --no-progress
          echo "C:\ProgramData\chocolatey\bin" >> $env:GITHUB_PATH
      - name: Build + package
        shell: pwsh
        env:
          CC: gcc
          AR: ar
        run: |
          cd desktop/src-tauri
          cargo build --release
          cd ../..
          powershell -ExecutionPolicy Bypass -File scripts/package-windows.ps1 -SkipBuild
      - uses: actions/upload-artifact@v4
        with:
          name: CSSwitch-win-x64
          path: dist/*.zip
```

注意：runner 上 `windres` 来自 choco 的 mingw；若找不到，加一步设置 `WINDRES` 环境变量指向它。
这条 CI 未在本机验证过（本机用的是 w64devkit），首次跑通后建议把结果写进 README 徽章。

---

## 4. 之后每次发版

1. 改 `desktop/src-tauri/tauri.conf.json` 的 `version` + `CHANGELOG.md`；
2. `git tag -a vX.Y.Z-win.N`；
3. 跑一次 `test\win-e2e.ps1` 与 `cargo test --lib`，把结果写进 Release 说明（本仓库的惯例是
   「通过 / 失败 / 未执行」分开记录，不要把没跑的说成通过）；
4. `scripts\package-windows.ps1` → `gh release create`。
