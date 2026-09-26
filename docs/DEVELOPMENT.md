# 开发指南（面向后续开发会话）

本文档描述 OpenTypeless 个人精简版的内部结构、约定与常见改动入口，供后续 AI/人工开发会话快速上手。

## 技术栈

- **前端**：React 19 + TypeScript + Vite + Tailwind CSS 4 + zustand + i18next + framer-motion
- **后端**：Rust + Tauri 2（`tauri = "=2.11.2"`，版本锁定）
- **测试**：Vitest（前端，jsdom）+ `cargo test --lib`（Rust）
- **存储**：SQLite（rusqlite bundled）存历史/词典；AppConfig 存 JSON 文件（tauri-plugin-store 不用于配置）

## 命令速查

```bash
npm ci                        # 安装依赖
npm run tauri dev             # 开发运行（前端热更新）
npm run tauri build           # 打包
npm test                      # Vitest
npm run lint                  # eslint
npx tsc --noEmit              # 类型检查（CI 必过）
npx prettier --write src/     # 格式化前端
cd src-tauri && cargo test --lib   # Rust 测试
cd src-tauri && cargo fmt          # 格式化 Rust
```

CI（`.github/workflows/ci.yml`）只做四件事：tsc、eslint、vitest、cargo test（Windows + Linux 两个 runner）。

## 核心数据流（听写）

1. 全局快捷键（`hotkey.rs` / `native_hotkey.rs`）触发 `pipeline.rs::start()`
2. `audio/capture.rs` 采集 PCM → `stt/` 提供商流式转写
3. 语音意图路由 `voice_intent/` 判断：直接听写 / 改写选中 / 翻译 / 搜索 / Ask
4. 需要润色时走 `llm/`（BYOK，`openai.rs` + `protocol.rs` 适配不同 API 形态）
5. `output/` 把结果注入目标应用：Windows SendInput / enigo 键盘直输，或剪贴板粘贴
6. `storage/` 保存历史记录；`commands/history.rs` 支持失败录音重转写

Ask Anything（`commands/ask.rs`）是独立的轻量流程：录音 → 转写 → 一次性 LLM 问答 → 弹出小窗。

## 关键约定

### 前后端契约
- 所有 IPC 调用都封装在 `src/lib/tauri.ts`，**不要在组件里直接 `invoke()`**（AskPanel 等个别处除外）
- 后端命令集中在 `src-tauri/src/commands/`，在 `lib.rs` 的 `generate_handler![]` 注册
- 事件（后端 → 前端）在 `src/hooks/useTauriEvents.ts` 统一监听

### 配置（AppConfig）
- 类型定义在两个地方，**改字段必须两边同步**：
  - Rust：`src-tauri/src/storage/mod.rs` 的 `AppConfig`（`#[serde(default)]`，向后兼容）
  - TS：`src/stores/appStore.ts` 的 `AppConfig` 接口 + 默认值 `defaultConfig`
- 如果字段需要参与 WebDAV 同步/备份：加入 `src/lib/backup-settings.ts` 的 `SafeScalarKey` 白名单（三个位置：类型、`createBackupSettings`、`SAFE_SCALAR_KEYS` 数组）
- **API Key 类字段绝不能进备份白名单**（Key 存系统凭据库，见下）

### 凭据（API Key / 密码）
- 一律存系统凭据库：Windows Credential Manager / Linux libsecret，封装在 `src-tauri/src/credentials.rs`
- 命名空间约定：`stt.<provider>`、`llm.<provider>`、`sync.webdav`（在 `commands/credentials.rs::validate_credential_target` 校验）
- 旧式明文 Key 字段（`stt_api_key` 等）仍留在配置里做迁移兼容，新代码读 Key 走 `resolve_*_secret`

### 平台代码
- 只支持 Windows / Linux。平台分发用 `#[cfg(target_os = "...")]`，Windows 专属文件在 `output/windows_*.rs`
- Linux Wayland 有专门的键盘输出降级路径（wtype/kwtype，见 `output/keyboard.rs`）
- 新增平台相关逻辑时保持 `platform.rs::capabilities()` 的语义（前端据此调整 UI 提示）

### i18n
- 只有 `src/i18n/locales/en.json` 与 `zh.json`，两者的键集合必须一致（`localeParity.test.ts` 守护）
- 新增文案两个文件都要加；Rust 侧返回的是错误码/键名，翻译在前端完成
- 语音指令语法（`src-tauri/src/voice_intent/grammar/`）保留 en / zh_hans / zh_hant 三套

### STT 提供商
- OpenAI 兼容族（OpenAI Whisper / Groq / SiliconFlow / GLM-ASR / 自定义）统一走 `stt/whisper_compat.rs`，端点配置在 `stt/config.rs`
- 流式提供商（Deepgram / AssemblyAI / 火山 / 千问）各有独立实现
- 新增提供商：`stt/mod.rs::create_provider` 注册 + `stt/config.rs` 补元数据 + 前端 `lib/constants.ts` 的 `STT_PROVIDERS` + i18n 键 + `stt/capabilities.rs` 的录音时长上限

### WebDAV 同步
- 后端命令：`src-tauri/src/commands/webdav.rs`（test / upload / download，HTTP PUT/GET + Basic Auth）
- 前端编排：`src/lib/webdav-sync.ts`（组包/恢复/自动同步）
- 备份包格式：`{ format: "opentypeless-backup", version: 1, settings, history, dictionary }`
- 恢复走 `commands/backup.rs::restore_backup_data` + `mergeBackupSettings` 白名单合并
- 密码在 `sync.webdav` 凭据命名空间，永不进备份包

## 测试策略

- 前端：组件测试用 `vi.mock('../../../lib/tauri')` 隔离 IPC；状态测试直接操作 `useAppStore`
- Rust：纯逻辑单测内联在各模块 `#[cfg(test)]`；`storage/mod.rs` 有 SQLite 集成测试（临时库）
- 改动同步/备份逻辑时补 `src/lib/__tests__/webdav-sync.test.ts` 与 `backup-settings.test.ts` 风格的用例
- **删除功能时同步删除对应测试**，不要留跳过的空壳

## 性能相关现状

- Rust release 配置已调优：`opt-level = "s"`、`lto = "thin"`、`strip = true`、`codegen-units = 1`
- 前端无代码分割（单窗口应用，bundle 已经很小）；framer-motion 保留用于交互动画
- HTTP 客户端全局复用连接池（`lib.rs::build_shared_http_client`），STT/LLM 请求前有 TLS 预热（`pipeline.rs::pre_warm`）
- 大文件：`pipeline.rs`（流水线）、`storage/mod.rs`（持久化）、`commands/ask.rs`（Ask 流程）——改动前先用 `ast_grep_outline` 摸结构

## 已知遗留 / 可改进点

1. `storage/mod.rs` 与 `commands/ask.rs` 单文件偏大，可拆分
2. `AppConfig` 双端定义靠手工同步，可考虑用 JSON Schema 或共享类型生成
3. WebDAV 目前是手动/保存后自动上传，没有冲突检测（多机同时改会覆盖）；可加时间戳协商
4. Wayland 下全局快捷键依赖合成器支持，hold 模式体验不如 X11；可探索各合成器专用接口
5. 历史记录目前也进同步包，数据量大会拖慢同步；可加"仅同步设置"开关
6. 前端 `i18n` 键在 600+ 数量级，其中部分设置文案仍以英文为锚（en.json 与 zh.json 同步维护即可）

## 修改守则

- **先跑测试再改**：`npm test` + `cargo test --lib` 必须全绿后再动手
- **每步可编译**：删代码后立即 `tsc --noEmit` + `cargo check`，避免连锁编译错误堆积
- **git 提交粒度**：一个功能/一次清理一个 commit，方便回滚
- **不引入新云服务**：本 fork 定位是纯本地 + 用户自有服务（BYOK / WebDAV）
