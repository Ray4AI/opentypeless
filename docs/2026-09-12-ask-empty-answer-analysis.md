# Ask 模式 "returned an empty answer" 根因分析（OpenRouter / BYOK）

> 现象：Windows 上按 Ask 快捷键（默认 `Ctrl+.`）说话提问，**只要问题稍微长一点（≈5 秒以上）就必定报
> `Ask returned an empty answer. Please try again.`**；同一套 OpenRouter 配置下，**语音转录和 AI 润色完全正常**。
>
> 结论先行：**不是超时、不是网络、不是语音识别问题。Ask 的请求体把输出预算写死成 80 tokens，
> 而 OpenRouter 的 reasoning（思考）tokens 与答案共享同一个 `max_tokens` 预算 —— 思考模型在 80 tokens 内
> 根本写不出正文，于是返回 HTTP 200 + `content: ""`，被 Ask 的校验判为"空答案"。**

分析基于 commit `213d4c4`（`v0.1.42`），文件路径均相对仓库根目录。

---

## 1. 报错的唯一来源

全仓库只有两处能产生这条消息，且都不是超时：

| 位置 | 函数 | 触发条件 |
|---|---|---|
| `src-tauri/src/commands/ask.rs:442` | `validate_ask_answer()` | 拿到的答案 `trim()` 后为空 |
| 同上，调用方 `ask.rs:902` | `ask_via_byok()` | **BYOK（自带 OpenRouter key）路径命中此分支** |

```rust
// src-tauri/src/commands/ask.rs:439
fn validate_ask_answer(answer: &str) -> Result<String, String> {
    let trimmed = answer.trim().to_string();
    if trimmed.is_empty() {
        return Err("Ask returned an empty answer. Please try again.".to_string()); // ← 442
    }
    Ok(trimmed)
}
```

云端（`ask_via_cloud()`，`ask.rs:945`）走的是 `body["answer"]`，与本问题无关。

---

## 2. 完整失败链路

```
按下 Ask 快捷键（Ctrl+.）
  → start_ask_dictation()            ask.rs:985 / 1000
  → 说话录音，STT 流式返回 transcript（正常，无报错）
  → stop_ask_dictation()             ask.rs:1349
       ├─ validate_ask_question()    ask.rs:1387 → 通过（说明转录没问题）
       ├─ 路由判断                   ask.rs:1434  Search / DraftInsert / 普通 Ask
       └─ answer_question()          ask.rs:735
             ├─ should_use_byok()    ask.rs:699 → true（provider=openrouter + 有 key）
             └─ ask_via_byok()       ask.rs:858
                   body = build_byok_ask_body_for_config()   ask.rs:567
                          └─ "max_tokens": ASK_OUTPUT_TOKEN_LIMIT = 80   ★ ask.rs:24 ★
                   ↓ HTTP 200
                   protocol::response_text()  ask.rs:901 → llm/protocol.rs:242
                   ↓ ""
                   validate_ask_answer("") → "Ask returned an empty answer"
```

**关键点**：`transcript` 已经校验通过才发 LLM 请求，所以报错文本虽然是"answer"，
但真正被"掏空"的是 LLM 输出，跟录音、跟 STT 无关。

---

## 3. 为什么润色正常、Ask 不正常（决定性对比）

同一个模型、同一个 key、同一份 `AppConfig`，**唯一的差别是输出预算**：

| 功能 | 代码位置 | max_tokens | 传输方式 | 空 content 兜底 | 结果 |
|---|---|---|---|---|---|
| AI 润色 polish | `src-tauri/src/pipeline.rs:2157` | **4096** | **流式** | 有（`llm/openai.rs:246`） | ✅ 正常 |
| 翻译 | 复用 polish 的 `LlmConfig` | **4096** | 流式 | 有 | ✅ 正常 |
| Ask 草稿意图（DraftInsert） | `pipeline.rs:2553` `run_ask_draft` | **4096** | 流式 | 有 | ✅ 正常 |
| **Ask 普通问答** | **`commands/ask.rs:24` + `:549`** | **80** | **非流式** | 几乎无效 | ❌ 空答案 |

```rust
// src-tauri/src/commands/ask.rs:22-25
pub const ASK_MAX_QUESTION_CHARS: usize = 500;
pub const ASK_MAX_SELECTED_TEXT_CHARS: usize = 4_000;
pub const ASK_OUTPUT_TOKEN_LIMIT: u32 = 80;   // ← 真凶
const ASK_STT_FINALIZE_TIMEOUT_SECS: u64 = 12;
```

而润色侧：

```rust
// src-tauri/src/pipeline.rs:2152
let llm_config = LlmConfig {
    provider: config.llm_provider.clone(),
    api_key: llm_api_key,
    model: config.llm_model.clone(),
    base_url: config.llm_base_url.clone(),
    max_tokens: 4096,          // ← 50 倍于 Ask
    temperature: 0.3,
};
```

---

## 4. 为什么 80 tokens 必然变成"0 字"

OpenRouter 统一了各家思考模型的思维链计费：**reasoning tokens 属于 output tokens，占用同一个
`max_tokens` 额度**（见 OpenRouter 文档 *Reasoning Tokens*：
"Reasoning tokens are considered output tokens and charged accordingly"）。

对思考模型（Gemini 2.5/3 thinking、GPT-5/o 系列、DeepSeek-R1/V4、Kimi、Qwen3、GLM 思考模式、
带 thinking 的 Claude……）来说，80 tokens 连思考都烧不完，直接 length 截断：

```jsonc
{
  "choices": [{
    "finish_reason": "length",                    // ← Ask 完全不读这个字段
    "message": {
      "content": "",                              // ← 空 → 触发报错
      "reasoning_content": "让我分析一下这个问题……" // ← 全文都在这里
    }
  }],
  "usage": {
    "completion_tokens": 80,
    "completion_tokens_details": { "reasoning_tokens": 80 }  // ← Ask 也不读
  }
}
```

HTTP 是 200，所以：不报错、不重试、不告警 —— 只剩一句用户完全无法理解的"empty answer"。

Google 直连 API 也有同类长期问题（`googleapis/python-genai#782`：
`thinking_budget` 被忽略、`thoughts_token_count` 会一路膨胀吃掉整个 `max_output_tokens`、
此时 `text` 为空）。业界同类修复都是"给 reasoning 单独留预算 / 提高 max_tokens / 用非思考模型"。

---

## 5. 为什么"提问 5 秒以上"才复现

这是本 bug 的典型指纹，说明它是**预算饥饿**而不是网络抖动：

| 提问 | 模型思考量 | 80 token 下的结果 |
|---|---|---|
| "什么是 Ask 模式"（短、事实性） | 有概率 <80 就收住 | 偶尔能返回 |
| "帮我对比 A 和 B"、"这段话什么意思"+选中文本 | 思考链轻松破 80 | **100% 空** |

问题越长 → 思考链越长 → 必然吃满 80 → 正文为空。

**另一个自相矛盾的设定**（`ask.rs:518`）：

```rust
fn ask_system_prompt(has_selected_text: bool) -> &'static str {
    // "Keep the answer under 40 words unless the user asks for a rewrite or translation."
    // ...
}
```

要求模型输出 40 词，却只给 80 tokens（40 英文词≈50–60 token，中文≈60–70 token），
再扣掉思考内容，**留给正文的实际余量恒等于 0**。

---

## 6. Ask 路径的其它叠加缺陷

### 6.1 非流式兜底字段名不对（OpenRouter 上基本失效）

```rust
// src-tauri/src/llm/protocol.rs:242
LlmApiKind::OpenAiCompatible => {
    let message = &body["choices"][0]["message"];
    message["content"]
        .as_str().filter(|c| !c.is_empty())
        .or_else(|| message["reasoning_content"].as_str())  // ← 只有这一个
        .unwrap_or("").to_string()
}
```

OpenRouter 把思维链放在 **`reasoning`** 与 **`reasoning_details[]`**（`{type:"reasoning.summary"…}`）里，
`reasoning_content` 只是 DeepSeek / vLLM 系的字段。所以这层兜底在 OpenRouter 上不生效。

> ⚠️ 修复建议：**不要**把 `reasoning` 当答案返回 —— 那是思维链（CoT），用户会看到一坨内部推理。
> 正确做法是提高预算 / 关闭过度思考，而不是改兜底字段。
> 顺带一提：流式路径 `llm/openai.rs:246` 会把整个 `reasoning_text` 当正文吐进输入框，
> 这个行为对"润色"场景其实是有污染的，也值得单独修。

### 6.2 Ask 无任何重试、无日志

| | 润色 `llm/openai.rs` | Ask `commands/ask.rs:858` |
|---|---|---|
| 5xx 重试 | ✅ 3 次指数退避（`:150`） | ❌ 无 |
| 连接超时重试 | ✅ 3 次（`:166`） | ❌ 无 |
| 空内容日志 | ✅ `tracing::warn!` 打印完整响应体（`:269`） | ❌ 无 |

### 6.3 超时偏紧（次要症状，报错文案不同）

```rust
// src-tauri/src/llm/protocol.rs:128
pub fn request_timeout(provider: &str, base_url: &str, model: &str) -> Duration {
    if detect_api_kind(...) == LlmApiKind::AnthropicMessages
        || is_reasoning_model_without_sampling_controls(model)  // 只认 gpt-5* / o1* / o3* / o4*
    { Duration::from_secs(60) } else { Duration::from_secs(30) }
}
```

`ask_via_byok()`（`ask.rs:882`）用这个 30s；OpenRouter 冷启动/排队 >30s 时报的是
`error decoding response body` 之类，而**不是** empty answer。如果你看到的确实是
"Ask returned an empty answer"，那就是 §3–§4 的预算问题，不是超时。

### 6.4 没有独立的 Ask 模型设置

Ask 与润色共用同一个 `config.llm_model`，**无法只给 Ask 换一个不思考的模型**。
而模型下拉是直接从 OpenRouter `/api/v1/models` 拉的（`llm/protocol.rs:96`、`LlmPane.tsx:185`），
前排绝大多数是思考模型，默认值 `google/gemini-2.5-flash` 也是 thinking 系。
这是产品设计层面的根因。

### 6.5 `ASK_OUTPUT_TOKEN_LIMIT = 80` 的来历（别误解成手误）

```
docs/2026-06-29-ask-anything-reliability-spec.md:127
  "Cloud ask output stays capped at `ASK_OUTPUT_TOKEN_LIMIT = 80`."
docs/2026-06-29-ask-anything-reliability-spec.md:146
  "talkmore/src/app/api/proxy/ask/route.ts caps output at 80 tokens."
```

它**本来是为了省官方云端额度**而定的，但代码里被无差别地同时套到了 BYOK 上 ——
BYOK 花的是用户自己的钱，抠这 80 token 毫无收益，只有 bug。

---

## 7. 不修改代码的验证 / 临时规避

1. 把问题缩到一句极短的事实性问题 → 大概率能返回。
2. 换成非思考模型（如 `google/gemini-2.5-flash` 的 instant 变体、`openai/gpt-4o-mini`、
   任意不带 reasoning 的模型）→ 立刻不报错。
3. 走 Ask 的 Search / DraftInsert 意图（例如让它在光标处起草，而不是弹答案气泡）
   → 走的是 4096 预算的润色链路，正常。
4. 换用官方 Cloud Ask（需登录）→ 走 `/api/proxy/ask`，但**同样是 80 上限**，只是服务端行为不可控。

---

## 8. 修复方案（本次已实施的部分标 ✅）

| # | 修改 | 文件 | 说明 |
|---|---|---|---|
| 1 ✅ | Ask 输出预算 80 → 可配置，默认 **4096**（与润色一致） | `commands/ask.rs`、`storage/mod.rs` | BYOK 不再饥饿；`ASK_OUTPUT_TOKEN_LIMIT = 80` 仅保留给云端额度契约 |
| 2 ✅ | 高级设置页暴露 prompt / token 预算 / 请求参数 | `src/components/Settings/AdvancedPane.tsx` | 新增 "Advanced / 高级设置" 面板 |
| 3 ✅ | 空答案给出可诊断原因（`finish_reason` + `reasoning_tokens`） | `commands/ask.rs` | 不再只说 "empty answer" |
| 4 ✅ | Ask 复用润色的重试策略（3 次指数退避）+ 空响应告警日志 | `commands/ask.rs` | 之前 0 重试、0 日志 |
| 5 ✅ | 请求 timeout 可配（Ask 独立 120s、润色 120s） | `storage/mod.rs`、`commands/ask.rs`、`llm/openai.rs` | |
| 6 ✅ | 新增 `get_ask_prompt_defaults` 命令，UI 可显示真实内置 prompt | `commands/ask.rs`、`lib.rs`、`src/lib/tauri.ts` | 避免前端复制一份会漂移的常量 |
| 7 | 流式路径不要把整段 `reasoning_text` 当正文输出 | `llm/openai.rs:246` | **未改动**，留给上游单独处理，避免污染润色输出的行为影响现有语义 |

### 8.1 新增配置字段（`AppConfig`，`#[serde(default)]` 已启用，向后兼容旧配置文件）

| 字段 | 类型 | 默认值 | 作用 |
|---|---|---|---|
| `ask_max_tokens` | u32 | **4096** | Ask 输出预算（与润色一致） |
| `ask_temperature` | f64 | 0.2 | Ask 采样温度 |
| `ask_system_prompt` | String | `""` | 非空时**整体替换**内置 Ask prompt |
| `ask_request_timeout_secs` | u64 | 120 | Ask 请求超时 |
| `polish_max_tokens` | u32 | **4096** | 润色/翻译输出预算 |
| `polish_temperature` | f64 | 0.3 | 润色采样温度 |
| `polish_system_prompt_append` | String | `""` | **追加**到润色 prompt 末尾 |
| `llm_request_timeout_secs` | u64 | 120 | 润色/翻译请求超时 |
| `ask_request_extra_params` | JSON String | `""` | merge 进 Ask 请求体 |
| `polish_request_extra_params` | JSON String | `""` | merge 进润色请求体 |

> **未实现（有意为之）**：`stt_request_extra_params`。转录请求是 multipart 文件上传
> （`src-tauri/src/stt/whisper_compat.rs:144`），没有可 merge 的 JSON 请求体，
> 强加一个字段只会带来"看起来能填但完全不生效"的 bug。因此 STT 侧不开放参数覆盖。

### 8.2 请求参数 merge 语义（刻意保持"不 user friendly"的通用性）

- 内容必须是 **JSON 对象**，按**顶层 key merge** 追加/覆盖到该请求自己的 body 上。
- 非法 JSON / 非对象（数组、标量）→ 设置界面即时红字提示；**请求层直接忽略**，
  不会让请求失败（`advanced.rs::parse_request_override` 返回 `None`）。
- 在 GLM `thinking` 特判**之后**应用，因此可以覆盖内置行为
  （例如对 OpenRouter 思考模型写 `{"reasoning":{"effort":"none"}}`）。
- `model` / `messages` 被**强制保护**（`PROTECTED_REQUEST_KEYS`），覆盖会被丢弃并写 warn 日志。
- 统一实现于 `src-tauri/src/llm/advanced.rs`，Ask 与润色共用同一套 merge/校验/夹取逻辑；
  不给任何单一服务商加特判，因此不增加 per-provider 维护成本。
- 数值夹取：`max_tokens` 16–128000、`temperature` 0–2、timeout 5–600 秒。

---

## 9. 复现用的最小请求（可直接 curl 验证）

```bash
# 复现 bug：max_tokens=80，思考模型 → content 为空、reasoning 吃掉全部额度
curl -s https://openrouter.ai/api/v1/chat/completions \
  -H "Authorization: Bearer $OPENROUTER_API_KEY" -H "Content-Type: application/json" \
  -d '{"model":"google/gemini-2.5-flash","max_tokens":80,"temperature":0.2,"stream":false,
       "messages":[{"role":"system","content":"Answer clearly and directly in the same language as the user. Keep the answer under 40 words."},
                   {"role":"user","content":"帮我对比一下 WebSocket 和 SSE 在语音识别场景下的优劣，并说明各自的延迟特征"}]}' \
  | jq '{finish: .choices[0].finish_reason, content: .choices[0].message.content, reasoning_tokens: .usage.completion_tokens_details.reasoning_tokens}'

# 验证修复：只改 max_tokens → 立刻有正文
#   "max_tokens": 4096
```

预期：第一条 `content` 为 `""` / `finish: "length"`；第二条正常返回中文答案。
