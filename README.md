# OpenTypeless（个人精简版）

开源 AI 语音输入桌面工具：按住快捷键说话，松开后把语音转成干净的文字，直接输入到当前应用中。

本仓库是 [tover0314-w/opentypeless](https://github.com/tover0314-w/opentypeless) 的**个人精简 fork**，在保留核心语音功能的前提下做了大幅瘦身，并新增 WebDAV 配置同步。

<p align="center">
  <img src="src-tauri/icons/128x128@2x.png" width="96" height="96" alt="OpenTypeless Logo" />
</p>

## 功能

- **语音输入**：全局快捷键唤起录音胶囊（Ctrl+/），说话松开自动转文字并键入当前光标处
- **Ask Anything**：另一个快捷键（Ctrl+.）语音提问，一次性回答（弹出小窗，无聊天历史）
- **AI 润色**：可选 LLM 对转写结果做清理/结构化/翻译（支持选中文字改写、翻译等语音意图）
- **本地词典 + 纠错规则**：人名、术语、常错词持久化修正，支持导入/导出
- **多 STT 提供商**（自带 API Key）：Deepgram、AssemblyAI、火山引擎豆包、阿里千问 ASR、智谱 GLM-ASR、OpenAI Whisper、Groq、SiliconFlow、自定义 OpenAI 兼容 Whisper
- **多 LLM 提供商**（自带 API Key）：智谱、DeepSeek、SiliconFlow、OpenAI、Gemini、Moonshot、豆包、通义、Groq、Claude、Ollama、OpenRouter
- **应用感知写作**：本地检测前台应用类别（邮件/聊天/文档/代码等），自动适配写作风格
- **WebDAV 配置同步**：设置、词典、历史记录可上传到任意 WebDAV 服务（Nextcloud、坚果云等），在另一台电脑一键拉取恢复

## 与上游的差异

| 移除 | 说明 |
| --- | --- |
| 官方付费/账号体系 | Stripe/Creem 结账、订阅、套餐配额、AppSumo、官方云备份全部移除 |
| 托管云 STT/LLM | 只保留自带 API Key（BYOK）模式 |
| 官方自动更新 / 深度链接登录 | updater、`opentypeless://` 回调移除 |
| 多语言 | 只保留中文、英文（界面、翻译目标、语音指令均支持中英） |
| macOS 支持 | Apple Speech、CGEventTap 快捷键、无障碍授权等全部移除，只支持 **Windows / Linux** |
| 官方营销文档 | 18 个翻译 README、发布签名流水线、社区治理文件等全部移除 |

| 新增 | 说明 |
| --- | --- |
| WebDAV 配置同步 | 设置 → 同步面板；密码只存系统凭据库，永不进同步包 |

## 平台支持

| 平台 | 状态 |
| --- | --- |
| Windows 10/11 x64 | ✅ 完整支持（原生单键快捷键如 RightAlt 可用） |
| Linux x64（X11 / Wayland） | ✅ 支持；Wayland 下全局快捷键受合成器限制，键盘直输推荐 X11 |
| macOS | ❌ 已移除 |

## 构建与运行

```bash
# 依赖：Node.js 20+、Rust 1.82+
npm ci

# 开发模式
npm run tauri dev

# 构建安装包（Windows: MSI/NSIS；Linux: AppImage/deb）
npm run tauri build

# 测试
npm test                 # 前端 Vitest（约 300+ 用例）
cd src-tauri && cargo test --lib   # Rust 单元测试（约 530 用例）
```

## 使用

1. 启动后进入引导流程：选择 STT 提供商并填入 API Key → 选择 LLM 提供商并填入 API Key → 完成
2. 任意界面按 `Ctrl+/` 开始听写（默认 hold 模式：按住说话，松开输出）
3. 按 `Ctrl+.` 打开 Ask Anything：语音提问，松开后弹出回答
4. 设置中可自定义快捷键、输出方式（键盘直输/剪贴板粘贴）、词典、写作风格、语音指令开关
5. **多机同步**：设置 → 同步，填入 WebDAV 地址/用户名/密码，点"测试连接"后可上传/下载；开启"自动同步"后每次保存设置自动上传

### WebDAV 同步内容

- 全部应用设置（含快捷键、场景、词典、纠错规则）
- 历史记录与词典数据
- **不含**：任何 API Key（Key 存在各自电脑的系统凭据库中，需要在新机器上重新填写）
- WebDAV 密码本身也只存本机系统凭据库，不会进入同步包

## 架构速览

```
src/                      # React + TypeScript 前端（Vite + Tailwind）
  components/             # UI：Capsule 录音胶囊、Settings、History、AskPanel、Onboarding
  stores/appStore.ts      # 全局状态（zustand），AppConfig 定义在此
  lib/tauri.ts            # 所有 Tauri invoke 封装（前后端契约）
  lib/backup-settings.ts  # 备份/恢复的字段白名单（同步安全）
  lib/webdav-sync.ts      # WebDAV 同步编排
  i18n/locales/           # 仅 en.json / zh.json

src-tauri/src/            # Rust 后端（Tauri 2）
  pipeline.rs             # 核心流水线：录音 → STT → LLM 润色 → 键盘注入
  commands/               # 全部 #[tauri::command]（前端可调用的 IPC）
  stt/                    # STT 提供商实现（whisper_compat 为 OpenAI 兼容族）
  llm/                    # LLM 提供商实现（openai.rs 为主，protocol.rs 做协议适配）
  voice_intent/           # 语音意图路由（听写/改写/翻译/搜索，中英语法）
  output/                 # 键盘/剪贴板输出（Windows SendInput / enigo）
  storage/mod.rs          # SQLite（历史/词典）+ AppConfig 持久化
  credentials.rs          # 系统凭据库（Windows Credential Manager / libsecret）
```

详细开发指南见 [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md)。

## 许可

[MIT](LICENSE) © OpenTypeless Contributors

第三方组件声明见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。
