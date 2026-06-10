//! Agent implementation
//!
//! Core Agent struct and task execution logic.
//!
//! 🆕 PLANNING FIX: Integrated planning module for autonomous task planning and
//! execution.

use std::collections::HashMap;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::Engine as _;
use tokio::sync::RwLock;
use tracing::{error, info, warn};

use crate::device::{AppLifecycle, Device, DeviceAutomation};
use crate::error::AgentError;
use crate::evolution::skill_distiller::{DistillTrigger, DistillerConfig, SkillDistiller};
use crate::planning::{
    ExecutionResult, Plan, PlanContext, PlanExecutor, PlanId, PlanStatus, PlanStep, PlanStrategy,
    PlanningEngine, RePlanner, StepType,
};
use crate::skills::composition::{InputMapping, PipelineStep, SkillPipeline};
use crate::skills::loader::{LoadedSkill, SkillManifest};
use crate::skills::registry::Version;
use crate::task::{Artifact, Task, TaskResult, TaskType};
use crate::{
    a2a, communication, events, mcp, queue, skills, state_manager, types, wallet, AgentConfig,
};

#[derive(Debug, Clone)]
struct WebSearchResult {
    title: String,
    url: String,
    snippet: String,
}

#[derive(Debug, Clone)]
struct WebSearchFailure {
    provider: &'static str,
    reason: String,
}

#[derive(Debug, Clone)]
struct McpToolTarget {
    server_name: String,
    tool_name: String,
}

pub struct Agent {
    pub(crate) config: AgentConfig,
    pub(crate) a2a_client: Option<a2a::A2AClient>,
    pub(crate) mcp_manager: Option<Arc<mcp::MCPManager>>,
    pub(crate) outbound_router: Option<Arc<communication::OutboundMessageRouter>>,
    pub(crate) message_rx: Option<tokio::sync::mpsc::Receiver<communication::UserMessageContext>>,
    pub(crate) queue_manager: Option<Arc<queue::QueueManager>>,
    pub(crate) skill_registry: Option<Arc<skills::SkillRegistry>>,
    pub(crate) llm_interface: Option<Arc<dyn communication::LLMCallInterface>>,
    // 🔒 P0 FIX: Wallet integration for on-chain transactions
    pub(crate) wallet: Option<Arc<wallet::AgentWallet>>,
    // 🟢 P1 FIX: 统一事件总线 - 复用 core::EventBus
    pub(crate) event_bus: Option<events::AgentEventBus>,
    // 🔒 P0 FIX: Kernel integration for WASM sandbox execution
    pub(crate) kernel: Option<Arc<beebotos_kernel::Kernel>>,
    // 🔒 P0 FIX: Agent state (from state_manager) — Mutex allows state updates
    // without &mut self so the kernel task never needs to hold a write-lock
    // across an await point.
    pub(crate) state: Mutex<state_manager::AgentState>,
    // 🆕 PLANNING FIX: Planning module integration
    pub(crate) planning_engine: Option<Arc<PlanningEngine>>,
    pub(crate) plan_executor: Option<Arc<PlanExecutor>>,
    pub(crate) replanner: Option<Arc<dyn RePlanner>>,
    // 🆕 PLANNING FIX: Active plans tracking
    pub(crate) active_plans: Arc<RwLock<HashMap<PlanId, Plan>>>,
    // 🆕 DEVICE FIX: Device automation integration
    pub(crate) device: Option<Device>,
    // 🟢 P1 FIX: Memory system for long-term memory retrieval
    pub(crate) memory_system: Option<Arc<dyn crate::memory::MemorySearch>>,
    // 🟢 P2 FIX: LLM response cache to reduce latency for repeated queries
    pub(crate) llm_response_cache: Arc<RwLock<HashMap<String, (String, Instant)>>>,
    // 🆕 FIX: Hold the current plan's original user goal so skill matching can use
    // domain keywords (e.g. "旅游") even when step descriptions are generic English.
    pub(crate) current_plan_goal: Arc<RwLock<Option<String>>>,
    // 🆕 FIX: Global skill catalog injected into LLM system prompt
    pub(crate) skill_catalog: Option<String>,
    // 🟢 P1 FIX: Workflow registry for workflow execution tasks
    pub(crate) workflow_registry: Option<Arc<crate::workflow::WorkflowRegistry>>,
    // 🆕 OPTIMIZATION PHASE 1: Intent engine for pre-LLM classification
    pub(crate) intent_engine: Option<crate::intent::IntentEngine>,
    // 🆕 SKILL MATCHING V2: Pure LLM-driven intent analyzer
    pub(crate) llm_intent_analyzer: Option<Arc<crate::skill_matching::LLMIntentAnalyzer>>,
    // 🆕 SKILL MATCHING V2: Pure LLM-driven skill selector
    pub(crate) skill_selector: Option<Arc<crate::skill_matching::SkillSelector>>,
    // 🆕 SKILL MATCHING V2: Activation trace store for observability
    pub(crate) trace_store: Option<Arc<dyn crate::skill_matching::TraceStore>>,
    // 🆕 OPTIMIZATION PHASE 1: Approval gate for destructive operations
    pub(crate) approval_gate: Option<crate::security::ApprovalGate>,
    // 🆕 FIX (Plan C): Pending approvals for multi-step user confirmation
    pub(crate) pending_approvals: Arc<RwLock<HashMap<String, crate::security::ApprovalRequest>>>,
    // 🆕 MCP PARAMETER EXTRACTION: Pending interactive parameter forms
    pub(crate) pending_parameter_forms:
        Arc<RwLock<HashMap<String, crate::skills::PendingParameterForm>>>,
    // 🆕 FIX (Plan C): Temporary flag to skip approval for confirmed operations
    pub(crate) skip_approval: std::sync::atomic::AtomicBool,
    // 🆕 OPTIMIZATION PHASE 2: Prompt cache for repeated prompt assembly
    pub(crate) prompt_cache: Option<Arc<crate::prompt::PromptCache>>,
    // 🆕 OPTIMIZATION PHASE 4: Max rounds limit to prevent infinite loops
    pub(crate) max_rounds: u32,
    // 🆕 OPTIMIZATION PHASE 3: Skill feedback collector for self-improvement
    pub(crate) skill_feedback_collector: Option<crate::skills::feedback::SkillImprovementEngine>,
    // 🆕 PHASE 5: Evolution scheduler for three-layer co-evolution orchestration
    pub(crate) evolution_scheduler: Option<crate::evolution::scheduler::EvolutionScheduler>,
    // 🆕 System information provider for querying Gateway-layer data (cron jobs, etc.)
    pub(crate) system_info_provider: Option<Arc<dyn crate::system_info::SystemInfoProvider>>,
    // 🆕 Tool working directory for sandboxed file operations
    pub(crate) tool_work_dir: std::path::PathBuf,
    // 🆕 Direct LLM client for native tool calling ( bypasses LLMCallInterface stub )
    pub(crate) llm_client: Option<Arc<crate::llm::LLMClient>>,
    // OpenClaw-style ReAct: optional sink for real-time tool loop feedback.
    pub(crate) react_trace_sink: Option<Arc<dyn crate::react_trace::ReActTraceSink>>,
    // OpenClaw-style ReAct: session-scoped skill context activated by the LLM.
    pub(crate) active_skill_contexts: Arc<RwLock<HashMap<String, Vec<String>>>>,
    // 🆕 Foreign runtime manager for Python/Node.js execution
    pub(crate) foreign_rt_manager: Option<Arc<beebotos_foreign_rt::DefaultForeignRuntimeManager>>,
}

struct TextSkillObservation {
    skill_id: String,
    arguments: String,
    output: String,
    success: bool,
}

fn configure_shell_command(cmd: &mut tokio::process::Command, command: &str) {
    #[cfg(windows)]
    {
        let encoded_command = encode_powershell_command(command);
        cmd.arg("-NoLogo")
            .arg("-NoProfile")
            .arg("-NonInteractive")
            .arg("-ExecutionPolicy")
            .arg("Bypass")
            .arg("-EncodedCommand")
            .arg(encoded_command);
    }
    #[cfg(not(windows))]
    {
        cmd.arg("-c").arg(command);
    }
}

fn decode_command_output(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return String::new();
    }

    if bytes.starts_with(&[0xff, 0xfe]) {
        return decode_utf16_le(&bytes[2..]);
    }
    if bytes.starts_with(&[0xfe, 0xff]) {
        return decode_utf16_be(&bytes[2..]);
    }
    if let Ok(text) = std::str::from_utf8(bytes) {
        return text.to_string();
    }

    let even_len = bytes.len() / 2 * 2;
    if even_len >= 2 {
        let nul_odd = bytes[..even_len]
            .chunks_exact(2)
            .filter(|chunk| chunk[1] == 0)
            .count();
        let pairs = even_len / 2;
        if pairs > 0 && nul_odd * 100 / pairs >= 30 {
            return decode_utf16_le(&bytes[..even_len]);
        }
    }

    String::from_utf8_lossy(bytes).to_string()
}

fn decode_utf16_le(bytes: &[u8]) -> String {
    String::from_utf16_lossy(
        &bytes
            .chunks_exact(2)
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
            .collect::<Vec<_>>(),
    )
}

fn decode_utf16_be(bytes: &[u8]) -> String {
    String::from_utf16_lossy(
        &bytes
            .chunks_exact(2)
            .map(|chunk| u16::from_be_bytes([chunk[0], chunk[1]]))
            .collect::<Vec<_>>(),
    )
}

fn configure_command_environment(cmd: &mut tokio::process::Command, workspace: Option<&Path>) {
    crate::command_env::configure_host_user_cli_environment(cmd, workspace);
}

#[cfg(windows)]
fn encode_powershell_command(command: &str) -> String {
    let wrapper = format!(
        r#"
$utf8NoBom = New-Object System.Text.UTF8Encoding -ArgumentList $false
[Console]::InputEncoding = $utf8NoBom
[Console]::OutputEncoding = $utf8NoBom
$OutputEncoding = $utf8NoBom
{command}
"#
    );
    let bytes: Vec<u8> = wrapper
        .encode_utf16()
        .flat_map(|code_unit| code_unit.to_le_bytes())
        .collect();
    encode_base64(&bytes)
}

#[cfg(windows)]
fn encode_base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut encoded = String::with_capacity(((bytes.len() + 2) / 3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0];
        let b1 = chunk.get(1).copied().unwrap_or(0);
        let b2 = chunk.get(2).copied().unwrap_or(0);

        encoded.push(TABLE[(b0 >> 2) as usize] as char);
        encoded.push(TABLE[(((b0 & 0b0000_0011) << 4) | (b1 >> 4)) as usize] as char);
        if chunk.len() > 1 {
            encoded.push(TABLE[(((b1 & 0b0000_1111) << 2) | (b2 >> 6)) as usize] as char);
        } else {
            encoded.push('=');
        }
        if chunk.len() > 2 {
            encoded.push(TABLE[(b2 & 0b0011_1111) as usize] as char);
        } else {
            encoded.push('=');
        }
    }
    encoded
}

fn extract_usd_notional(text: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    let chars: Vec<char> = text.chars().collect();
    for (idx, ch) in chars.iter().enumerate() {
        if !ch.is_ascii_digit() {
            continue;
        }
        let mut end = idx;
        while end < chars.len() && (chars[end].is_ascii_digit() || chars[end] == '.') {
            end += 1;
        }
        let value: String = chars[idx..end].iter().collect();
        let tail: String = chars[end..chars.len().min(end + 12)].iter().collect();
        let tail_lower = tail.to_ascii_lowercase();
        let prefix_start = idx.saturating_sub(2);
        let prefix: String = chars[prefix_start..idx].iter().collect();
        if tail_lower.contains("usd")
            || tail.contains("美元")
            || tail.contains("美金")
            || prefix.contains('$')
            || lower.contains(&format!("${}", value))
        {
            return Some(value);
        }
    }
    None
}

fn is_pending_approval_adjustment(message_text: &str) -> bool {
    let text = message_text.trim();
    if text.is_empty() || text.len() > 80 {
        return false;
    }
    let lower = text.to_ascii_lowercase();
    let has_amount = text.chars().any(|c| c.is_ascii_digit())
        && (text.contains("美元")
            || text.contains("美金")
            || text.contains("usd")
            || lower.contains("usd")
            || text.contains('$'));
    has_amount
        && (text.contains("调整")
            || text.contains("改")
            || text.contains("金额")
            || text.contains("换成")
            || text.contains("设为")
            || text.contains("改为")
            || lower.contains("amount"))
}

fn is_confirmation_message(message_text: &str) -> bool {
    let text = message_text.trim();
    let lower = text.to_ascii_lowercase();
    matches!(
        text,
        "确认" | "同意" | "可以" | "执行" | "是" | "好" | "好的" | "确认执行"
    ) || matches!(lower.as_str(), "yes" | "y" | "ok" | "okay" | "confirm")
}

fn has_side_effect_execution_intent(message_text: &str) -> bool {
    let lower = message_text.to_ascii_lowercase();
    let side_effect_terms = [
        "执行",
        "操作",
        "提交",
        "创建",
        "修改",
        "删除",
        "移除",
        "取消",
        "发送",
        "转账",
        "支付",
        "提现",
        "下单",
        "开单",
        "买",
        "买入",
        "卖",
        "卖出",
        "购买",
        "交易",
        "execute",
        "run",
        "submit",
        "create",
        "modify",
        "update",
        "delete",
        "remove",
        "cancel",
        "send",
        "transfer",
        "withdraw",
        "pay",
        "buy",
        "sell",
        "order",
        "place order",
    ];
    side_effect_terms.iter().any(|term| lower.contains(term))
}

fn looks_like_unbacked_approval_confirmation(response: &str) -> bool {
    let lower = response.to_ascii_lowercase();
    let asks_confirmation = response.contains("确认")
        || response.contains("请回复")
        || lower.contains("confirm")
        || lower.contains("please reply");
    let side_effect_context = response.contains("执行")
        || response.contains("操作")
        || response.contains("提交")
        || response.contains("创建")
        || response.contains("修改")
        || response.contains("删除")
        || response.contains("移除")
        || response.contains("取消")
        || response.contains("发送")
        || response.contains("转账")
        || response.contains("支付")
        || response.contains("提现")
        || response.contains("无法撤销")
        || response.contains("真实")
        || response.contains("高风险")
        || response.contains("下单")
        || response.contains("市价")
        || response.contains("买入")
        || response.contains("卖出")
        || response.contains("手数")
        || lower.contains("execute")
        || lower.contains("submit")
        || lower.contains("create")
        || lower.contains("modify")
        || lower.contains("update")
        || lower.contains("delete")
        || lower.contains("remove")
        || lower.contains("cancel")
        || lower.contains("send")
        || lower.contains("transfer")
        || lower.contains("withdraw")
        || lower.contains("payment")
        || lower.contains("irreversible")
        || lower.contains("risk")
        || lower.contains("order")
        || lower.contains("buy")
        || lower.contains("sell")
        || lower.contains("market");
    asks_confirmation && side_effect_context
}

impl Agent {
    fn runtime_context_prompt() -> String {
        let now = chrono::Local::now();
        let utc_now = chrono::Utc::now();
        format!(
            "## Runtime Context\nCurrent local date and time: {}.\nCurrent UTC date and time: \
             {}.\nWhen the user says today, now, current, latest, this morning, tonight, or uses \
             another relative time expression, resolve it against the current local date above. \
             Do not invent or reuse an old date. For real-time or recently-changing facts such as \
             weather, prices, news, schedules, availability, or account state, use the \
             appropriate tool instead of relying on model memory. When searching the web for a \
             relative-time request, keep the user's relative wording or use the current local \
             date above; never substitute a stale date.",
            now.format("%Y-%m-%d %H:%M:%S %:z"),
            utc_now.format("%Y-%m-%d %H:%M:%S UTC")
        )
    }

    fn workspace_dir(&self) -> PathBuf {
        #[cfg(test)]
        if let Ok(path) = std::env::var("BEEBOTOS_WORKSPACE") {
            let path = PathBuf::from(path);
            if !path.as_os_str().is_empty() {
                return path;
            }
        }

        let current = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        if let Some(root) = Self::find_workspace_root(&current) {
            return root.join("data").join("workspace");
        }

        current.join("data").join("workspace")
    }

    fn workspace_dir_display(&self) -> String {
        self.workspace_dir().display().to_string()
    }

    fn find_workspace_root(start: &Path) -> Option<PathBuf> {
        for dir in start.ancestors() {
            if dir.join("Cargo.toml").is_file()
                && dir.join("crates").is_dir()
                && dir.join("apps").is_dir()
            {
                return Some(dir.to_path_buf());
            }
        }
        None
    }

    fn is_builtin_workspace_tool(name: &str) -> bool {
        matches!(
            name,
            "read_file"
                | "write_file"
                | "edit_file"
                | "list_dir"
                | "glob"
                | "grep"
                | "exec"
                | "file_read"
                | "file_write"
                | "file_list"
                | "bash_shell"
                | "process_exec"
                | "create_cron_job"
        )
    }

    fn is_mcp_search_tool(name: &str) -> bool {
        name == "mcp_tool_search"
    }

    fn is_mcp_dynamic_tool_name(name: &str) -> bool {
        name.starts_with("mcp__")
    }

    fn is_controlled_workspace_tool(name: &str) -> bool {
        matches!(
            name,
            "write_file" | "edit_file" | "exec" | "file_write" | "bash_shell" | "process_exec"
        )
    }

    fn controlled_workspace_tools_enabled(&self) -> bool {
        std::env::var("BEEBOTOS_ENABLE_CONTROLLED_WORKSPACE_TOOLS")
            .map(|v| !matches!(v.as_str(), "0" | "false" | "FALSE" | "no" | "NO"))
            .unwrap_or(true)
    }

    fn approval_env() -> HashMap<String, String> {
        let mut env = HashMap::new();

        if let Ok(value) = std::env::var("ALPACA_PAPER_TRADE") {
            let normalized = value.trim().to_ascii_lowercase();
            if !normalized.is_empty() {
                env.insert("ALPACA_PAPER_TRADE".to_string(), normalized);
            }
        }

        env
    }

    fn builtin_workspace_tools(&self) -> Vec<communication::ToolDefinition> {
        let mut tools = vec![
            communication::ToolDefinition {
                name: "read_file".to_string(),
                description: "Read a UTF-8 text file from the current workspace. Parameters: path \
                              (string), max_chars (integer, optional)."
                    .to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "File path, relative to the workspace unless absolute" },
                        "max_chars": { "type": "integer", "description": "Maximum characters to return", "default": 12000 }
                    },
                    "required": ["path"]
                }),
            },
            communication::ToolDefinition {
                name: "list_dir".to_string(),
                description: "List files and directories in the current workspace. Parameters: \
                              path (string, optional)."
                    .to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Directory path, relative to the workspace unless absolute", "default": "." }
                    },
                    "required": []
                }),
            },
            communication::ToolDefinition {
                name: "glob".to_string(),
                description: "Find workspace files whose relative paths match a glob pattern. \
                              Parameters: pattern (string), path (string, optional), max_results \
                              (integer, optional)."
                    .to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "pattern": { "type": "string", "description": "Glob pattern such as **/*.rs or Cargo.toml" },
                        "path": { "type": "string", "description": "Directory to search, relative to the workspace", "default": "." },
                        "max_results": { "type": "integer", "description": "Maximum matches to return", "default": 200 }
                    },
                    "required": ["pattern"]
                }),
            },
            communication::ToolDefinition {
                name: "grep".to_string(),
                description: "Search workspace text files with a regular expression. Parameters: \
                              pattern (string), path (string, optional), include (glob, \
                              optional), case_sensitive (boolean, optional), max_matches \
                              (integer, optional)."
                    .to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "pattern": { "type": "string", "description": "Regex pattern to search for" },
                        "path": { "type": "string", "description": "File or directory to search, relative to the workspace", "default": "." },
                        "include": { "type": "string", "description": "Optional glob filter such as **/*.rs" },
                        "case_sensitive": { "type": "boolean", "description": "Whether matching is case-sensitive", "default": true },
                        "max_matches": { "type": "integer", "description": "Maximum matches to return", "default": 200 }
                    },
                    "required": ["pattern"]
                }),
            },
            communication::ToolDefinition {
                name: "create_cron_job".to_string(),
                description: "Create a Gateway-managed scheduled Agent task. Use this when the \
                              user asks to remind, schedule, periodically run, or create a cron \
                              task. Parameters include name, schedule_type (at/every/cron), \
                              schedule_expr, prompt, timezone, delivery_channel, and max_runs."
                    .to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "name": { "type": "string", "description": "Short human-readable task name" },
                        "description": { "type": "string", "description": "Optional task description" },
                        "schedule_type": { "type": "string", "enum": ["at", "every", "cron"], "description": "at=one-shot ISO time, every=interval like 30m/1h/1d, cron=5-field cron expression" },
                        "schedule_expr": { "type": "string", "description": "ISO 8601 time, interval, or cron expression matching schedule_type" },
                        "timezone": { "type": "string", "description": "IANA timezone, default Asia/Shanghai" },
                        "prompt": { "type": "string", "description": "Instruction the agent should execute when the schedule fires" },
                        "enabled": { "type": "boolean", "description": "Whether the job starts enabled", "default": true },
                        "context_mode": { "type": "string", "enum": ["isolated", "main"], "description": "isolated is recommended for scheduled tasks", "default": "isolated" },
                        "delivery_channel": { "type": "string", "description": "Optional result delivery channel, e.g. webchat or webhook" },
                        "delivery_target": { "type": "string", "description": "Optional delivery target such as a webchat channel/session or webhook URL" },
                        "max_runs": { "type": "integer", "description": "Optional maximum number of executions" }
                    },
                    "required": ["name", "schedule_type", "schedule_expr", "prompt"]
                }),
            },
        ];
        if self.controlled_workspace_tools_enabled() {
            tools.extend(self.controlled_workspace_tool_definitions());
        }
        tools
    }

    fn web_tools_enabled(&self) -> bool {
        std::env::var("BEEBOTOS_WEB_TOOLS_ENABLED")
            .map(|v| !matches!(v.as_str(), "0" | "false" | "FALSE" | "no" | "NO"))
            .unwrap_or(true)
    }

    fn builtin_react_tools(&self) -> Vec<communication::ToolDefinition> {
        let mut tools = self.builtin_workspace_tools();
        tools.push(communication::ToolDefinition {
            name: "activate_skill".to_string(),
            description: "Activate a BeeBotOS skill as additional context for this conversation. \
                          Use this when the skill index suggests a skill is relevant. This does \
                          not execute the skill; it injects its instructions into future turns."
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "skill_id": { "type": "string", "description": "The exact skill id from the skill index" }
                },
                "required": ["skill_id"]
            }),
        });

        if self.system_info_provider.is_some() {
            tools.push(communication::ToolDefinition {
                name: "session_search".to_string(),
                description: "Search previous persisted chat messages for the current user. Use \
                              this only when the answer needs context from older conversations."
                    .to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "Search query for older conversations" },
                        "limit": { "type": "integer", "description": "Maximum results to return", "default": 5 }
                    },
                    "required": ["query"]
                }),
            });
        }

        if self.web_tools_enabled() {
            tools.push(communication::ToolDefinition {
                name: "web_fetch".to_string(),
                description: "Fetch a public HTTP/HTTPS URL and return readable text. Local, \
                              private, and non-web URLs are blocked."
                    .to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "url": { "type": "string", "description": "Public HTTP or HTTPS URL to fetch" },
                        "max_chars": { "type": "integer", "description": "Maximum characters to return", "default": 12000 }
                    },
                    "required": ["url"]
                }),
            });
            tools.push(communication::ToolDefinition {
                name: "web_search".to_string(),
                description: "Search the web for current information. Use this for real-time or \
                              recently-changing facts such as weather, prices, news, schedules, \
                              and availability. For relative dates such as today or now, resolve \
                              them against the runtime date in the system prompt; never invent a \
                              stale date. Defaults to keyless DuckDuckGo search when no \
                              configured provider is available."
                    .to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "Search query" },
                        "count": { "type": "integer", "description": "Number of results to return", "default": 5 },
                        "country": { "type": "string", "description": "Optional region/country hint" },
                        "language": { "type": "string", "description": "Optional language hint" },
                        "freshness": { "type": "string", "description": "Optional freshness hint: day, week, month, year" }
                    },
                    "required": ["query"]
                }),
            });
        }

        tools
    }

    fn mcp_tool_search_definition() -> communication::ToolDefinition {
        communication::ToolDefinition {
            name: "mcp_tool_search".to_string(),
            description: "Load schema details for a connected MCP tool. Prefer passing the exact \
                          catalog name in tool_name, for example mcp:server/tool. You may also \
                          search by query when you only know the intent. This returns lightweight \
                          matches and dynamically exposes selected MCP tool schemas for the next \
                          tool call."
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "tool_name": { "type": "string", "description": "Exact MCP catalog name in the form mcp:server/tool" },
                    "query": { "type": "string", "description": "Natural-language task or capability to search for when the exact tool_name is unknown" },
                    "server": { "type": "string", "description": "Optional MCP server name to restrict search" },
                    "limit": { "type": "integer", "description": "Maximum tools to return", "default": 5 }
                }
            }),
        }
    }

    fn add_mcp_tool_search_if_available(&self, tools: &mut Vec<communication::ToolDefinition>) {
        if self.mcp_manager.is_some() && !tools.iter().any(|tool| tool.name == "mcp_tool_search") {
            tools.push(Self::mcp_tool_search_definition());
        }
    }

    fn controlled_workspace_tool_definitions(&self) -> Vec<communication::ToolDefinition> {
        vec![
            communication::ToolDefinition {
                name: "write_file".to_string(),
                description: "Write a UTF-8 text file. Relative paths are resolved from the \
                              current workspace; absolute paths are allowed. Controlled tool: \
                              only available when runtime policy enables file writes."
                    .to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "File path, relative to the workspace unless absolute" },
                        "content": { "type": "string", "description": "Full file content to write" }
                    },
                    "required": ["path", "content"]
                }),
            },
            communication::ToolDefinition {
                name: "edit_file".to_string(),
                description: "Replace text in a UTF-8 file. Relative paths are resolved from the \
                              current workspace; absolute paths are allowed. Controlled tool: \
                              only available when runtime policy enables file writes."
                    .to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "File path, relative to the workspace unless absolute" },
                        "old_text": { "type": "string", "description": "Exact text to replace" },
                        "new_text": { "type": "string", "description": "Replacement text" },
                        "replace_all": { "type": "boolean", "description": "Replace all matches instead of requiring exactly one match", "default": false }
                    },
                    "required": ["path", "old_text", "new_text"]
                }),
            },
            communication::ToolDefinition {
                name: "exec".to_string(),
                description: "Execute a shell command. By default it runs in the current \
                              workspace; working_dir may be relative to the workspace or \
                              absolute. Controlled tool: only available when runtime policy \
                              enables command execution."
                    .to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "command": { "type": "string", "description": "Shell command to run" },
                        "working_dir": { "type": "string", "description": "Optional working directory, relative to the workspace unless absolute", "default": "." },
                        "timeout_ms": { "type": "integer", "description": "Timeout in milliseconds", "default": 180000 }
                    },
                    "required": ["command"]
                }),
            },
        ]
    }

    fn resolve_tool_path(&self, path: &str) -> Result<PathBuf, AgentError> {
        let workspace = self.workspace_dir();
        let candidate = PathBuf::from(path);
        let joined = if candidate.is_absolute() {
            candidate
        } else {
            workspace.join(candidate)
        };
        Ok(Self::normalize_path_without_fs(&joined))
    }

    fn normalize_path_without_fs(path: &Path) -> PathBuf {
        let mut out = PathBuf::new();
        for component in path.components() {
            match component {
                std::path::Component::CurDir => {}
                std::path::Component::ParentDir => {
                    out.pop();
                }
                other => out.push(other.as_os_str()),
            }
        }
        out
    }

    async fn execute_builtin_workspace_tool(
        &self,
        tool_name: &str,
        params: Option<&HashMap<String, String>>,
        fallback_input: &str,
    ) -> Result<String, AgentError> {
        let canonical_name = match tool_name {
            "file_read" => "read_file",
            "file_write" => "write_file",
            "file_list" => "list_dir",
            "bash_shell" | "process_exec" => "exec",
            other => other,
        };
        if Self::is_controlled_workspace_tool(canonical_name)
            && !self.controlled_workspace_tools_enabled()
        {
            return Err(AgentError::Execution(format!(
                "Workspace tool '{}' is controlled and disabled by default",
                canonical_name
            )));
        }
        let get = |key: &str| params.and_then(|p| p.get(key)).map(|s| s.as_str());

        match canonical_name {
            "read_file" => {
                let path = get("path").unwrap_or(fallback_input).trim();
                if path.is_empty() {
                    return Err(AgentError::Execution(
                        "read_file requires a non-empty path".to_string(),
                    ));
                }
                let max_chars = get("max_chars")
                    .and_then(|s| s.parse::<usize>().ok())
                    .unwrap_or(12000);
                let resolved = self.resolve_tool_path(path)?;
                let content = tokio::fs::read_to_string(&resolved).await.map_err(|e| {
                    AgentError::Execution(format!("Failed to read '{}': {}", resolved.display(), e))
                })?;
                let truncated: String = content.chars().take(max_chars).collect();
                let suffix = if truncated.len() < content.len() {
                    format!(
                        "\n\n...[truncated, total chars: {}]",
                        content.chars().count()
                    )
                } else {
                    String::new()
                };
                Ok(format!(
                    "File: {}\n\n{}{}",
                    resolved.display(),
                    truncated,
                    suffix
                ))
            }
            "list_dir" => {
                let path = get("path").unwrap_or(".").trim();
                let resolved = self.resolve_tool_path(if path.is_empty() { "." } else { path })?;
                let mut entries = tokio::fs::read_dir(&resolved).await.map_err(|e| {
                    AgentError::Execution(format!("Failed to list '{}': {}", resolved.display(), e))
                })?;
                let mut lines = vec![format!("Directory: {}", resolved.display())];
                while let Ok(Some(entry)) = entries.next_entry().await {
                    let name = entry.file_name().to_string_lossy().to_string();
                    let kind = match entry.file_type().await {
                        Ok(ft) if ft.is_dir() => "dir",
                        Ok(ft) if ft.is_symlink() => "link",
                        _ => "file",
                    };
                    lines.push(format!("[{}] {}", kind, name));
                }
                lines.sort();
                Ok(lines.join("\n"))
            }
            "glob" => {
                let pattern = get("pattern")
                    .ok_or_else(|| AgentError::Execution("glob requires pattern".to_string()))?
                    .trim();
                if pattern.is_empty() {
                    return Err(AgentError::Execution(
                        "glob requires a non-empty pattern".to_string(),
                    ));
                }
                let path = get("path").unwrap_or(".").trim();
                let max_results = get("max_results")
                    .and_then(|s| s.parse::<usize>().ok())
                    .unwrap_or(200)
                    .clamp(1, 1000);
                let root = self.resolve_tool_path(if path.is_empty() { "." } else { path })?;
                let workspace = Self::normalize_path_without_fs(&self.workspace_dir());
                let pattern = pattern.to_string();
                tokio::task::spawn_blocking(move || {
                    Self::execute_workspace_glob_sync(&root, &workspace, &pattern, max_results)
                })
                .await
                .map_err(|e| AgentError::Execution(format!("Glob blocking task panicked: {}", e)))?
            }
            "grep" => {
                let pattern = get("pattern")
                    .ok_or_else(|| AgentError::Execution("grep requires pattern".to_string()))?
                    .trim();
                if pattern.is_empty() {
                    return Err(AgentError::Execution(
                        "grep requires a non-empty pattern".to_string(),
                    ));
                }
                let path = get("path").unwrap_or(".").trim();
                let include = get("include").map(str::trim).filter(|s| !s.is_empty());
                let case_sensitive = get("case_sensitive")
                    .map(|s| !matches!(s, "false" | "0" | "FALSE" | "no" | "NO"))
                    .unwrap_or(true);
                let max_matches = get("max_matches")
                    .and_then(|s| s.parse::<usize>().ok())
                    .unwrap_or(200)
                    .clamp(1, 1000);
                let root = self.resolve_tool_path(if path.is_empty() { "." } else { path })?;
                let workspace = Self::normalize_path_without_fs(&self.workspace_dir());
                let pattern = pattern.to_string();
                let include = include.map(|s| s.to_string());
                tokio::task::spawn_blocking(move || {
                    Self::execute_workspace_grep_sync(
                        &root,
                        &workspace,
                        &pattern,
                        include.as_deref(),
                        case_sensitive,
                        max_matches,
                    )
                })
                .await
                .map_err(|e| AgentError::Execution(format!("Grep blocking task panicked: {}", e)))?
            }
            "create_cron_job" => {
                let provider = self.system_info_provider.as_ref().ok_or_else(|| {
                    AgentError::Execution(
                        "create_cron_job is unavailable: system info provider is not configured"
                            .to_string(),
                    )
                })?;
                let name = get("name")
                    .ok_or_else(|| {
                        AgentError::Execution("create_cron_job requires name".to_string())
                    })?
                    .trim();
                let schedule_type = get("schedule_type")
                    .ok_or_else(|| {
                        AgentError::Execution("create_cron_job requires schedule_type".to_string())
                    })?
                    .trim()
                    .to_ascii_lowercase();
                let schedule_expr = get("schedule_expr")
                    .ok_or_else(|| {
                        AgentError::Execution("create_cron_job requires schedule_expr".to_string())
                    })?
                    .trim();
                let prompt = get("prompt").unwrap_or(fallback_input).trim();
                if name.is_empty() || schedule_expr.is_empty() || prompt.is_empty() {
                    return Err(AgentError::Execution(
                        "create_cron_job requires non-empty name, schedule_expr, and prompt"
                            .to_string(),
                    ));
                }
                let request = crate::system_info::CreateGatewayCronJobRequest {
                    name: name.to_string(),
                    description: get("description")
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(ToOwned::to_owned),
                    schedule_type,
                    schedule_expr: schedule_expr.to_string(),
                    timezone: get("timezone")
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(ToOwned::to_owned),
                    prompt: prompt.to_string(),
                    enabled: get("enabled")
                        .map(|s| !matches!(s, "false" | "0" | "FALSE" | "no" | "NO")),
                    context_mode: get("context_mode")
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(ToOwned::to_owned),
                    delivery_channel: get("delivery_channel")
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(ToOwned::to_owned),
                    delivery_target: get("delivery_target")
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(ToOwned::to_owned),
                    max_runs: get("max_runs").and_then(|s| s.parse::<i64>().ok()),
                    created_by: Some(self.config.id.clone()),
                };
                let job = provider
                    .create_gateway_cron_job(request)
                    .await
                    .map_err(|e| {
                        AgentError::Execution(format!("Failed to create cron job: {}", e))
                    })?;
                Ok(format!(
                    "Created cron job '{}' (id: {}, type: {}, schedule: {}, timezone: {}, \
                     enabled: {}).",
                    job.name,
                    job.id,
                    job.schedule_type,
                    job.schedule_expr,
                    job.timezone,
                    job.enabled
                ))
            }
            "write_file" => {
                let path = get("path")
                    .ok_or_else(|| AgentError::Execution("write_file requires path".to_string()))?;
                let content = get("content").ok_or_else(|| {
                    AgentError::Execution("write_file requires content".to_string())
                })?;
                let resolved = self.resolve_tool_path(path)?;
                if let Some(parent) = resolved.parent() {
                    tokio::fs::create_dir_all(parent).await.map_err(|e| {
                        AgentError::Execution(format!(
                            "Failed to create '{}': {}",
                            parent.display(),
                            e
                        ))
                    })?;
                }
                tokio::fs::write(&resolved, content).await.map_err(|e| {
                    AgentError::Execution(format!(
                        "Failed to write '{}': {}",
                        resolved.display(),
                        e
                    ))
                })?;
                Ok(format!("Wrote file: {}", resolved.display()))
            }
            "edit_file" => {
                let path = get("path")
                    .ok_or_else(|| AgentError::Execution("edit_file requires path".to_string()))?;
                let old_text = get("old_text").ok_or_else(|| {
                    AgentError::Execution("edit_file requires old_text".to_string())
                })?;
                let new_text = get("new_text").ok_or_else(|| {
                    AgentError::Execution("edit_file requires new_text".to_string())
                })?;
                if old_text.is_empty() {
                    return Err(AgentError::Execution(
                        "edit_file old_text cannot be empty".to_string(),
                    ));
                }
                let replace_all = get("replace_all")
                    .map(|s| s == "true" || s == "1")
                    .unwrap_or(false);
                let resolved = self.resolve_tool_path(path)?;
                let content = tokio::fs::read_to_string(&resolved).await.map_err(|e| {
                    AgentError::Execution(format!("Failed to read '{}': {}", resolved.display(), e))
                })?;
                let count = content.matches(old_text).count();
                if count == 0 {
                    return Err(AgentError::Execution(format!(
                        "edit_file could not find old_text in '{}'",
                        resolved.display()
                    )));
                }
                if !replace_all && count != 1 {
                    return Err(AgentError::Execution(format!(
                        "edit_file found {} matches in '{}'; set replace_all=true or provide a \
                         more specific old_text",
                        count,
                        resolved.display()
                    )));
                }
                let updated = if replace_all {
                    content.replace(old_text, new_text)
                } else {
                    content.replacen(old_text, new_text, 1)
                };
                tokio::fs::write(&resolved, updated).await.map_err(|e| {
                    AgentError::Execution(format!(
                        "Failed to write '{}': {}",
                        resolved.display(),
                        e
                    ))
                })?;
                Ok(format!(
                    "Edited file: {} ({} replacement(s))",
                    resolved.display(),
                    if replace_all { count } else { 1 }
                ))
            }
            "exec" => {
                let command = get("command").unwrap_or(fallback_input).trim();
                if command.is_empty() {
                    return Err(AgentError::Execution(
                        "exec requires a non-empty command".to_string(),
                    ));
                }
                self.validate_workspace_command(command)?;
                let working_dir = match get("working_dir") {
                    Some(dir) if !dir.trim().is_empty() => self.resolve_tool_path(dir)?,
                    _ => self.workspace_dir(),
                };
                let workspace = self.workspace_dir();
                let timeout_ms = get("timeout_ms")
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or(180000);
                let mut cmd = tokio::process::Command::new(if cfg!(windows) {
                    "powershell.exe"
                } else {
                    "sh"
                });
                configure_shell_command(&mut cmd, command);
                cmd.current_dir(&working_dir);
                cmd.kill_on_drop(true);
                configure_command_environment(&mut cmd, Some(&workspace));
                let output = tokio::time::timeout(
                    std::time::Duration::from_millis(timeout_ms),
                    cmd.output(),
                )
                .await
                .map_err(|_| {
                    AgentError::Execution(format!(
                        "Command timed out after {}ms: {}",
                        timeout_ms, command
                    ))
                })?
                .map_err(|e| AgentError::Execution(format!("Failed to execute command: {}", e)))?;
                let stdout = decode_command_output(&output.stdout);
                let stderr = decode_command_output(&output.stderr);
                Ok(format!(
                    "Exit code: {}\nWorking directory: {}\n\nSTDOUT:\n{}\nSTDERR:\n{}",
                    output.status.code().unwrap_or(-1),
                    working_dir.display(),
                    stdout,
                    stderr
                ))
            }
            _ => Err(AgentError::Execution(format!(
                "Unknown builtin workspace tool: {}",
                tool_name
            ))),
        }
    }

    fn validate_workspace_command(&self, command: &str) -> Result<(), AgentError> {
        let lower = command.to_lowercase();
        let blocked = [
            "rm -rf /",
            "rm -rf /*",
            ":(){ :|:& };:",
            "> /dev/sda",
            "dd if=/dev/zero",
            "mkfs.",
        ];
        for pattern in blocked {
            if lower.contains(pattern) {
                return Err(AgentError::Execution(format!(
                    "Dangerous command pattern blocked: {}",
                    pattern
                )));
            }
        }
        Ok(())
    }

    async fn execute_native_tool_call(
        &self,
        tool_call: &crate::llm::ToolCall,
        tool_name_map: &HashMap<String, String>,
        input_text: &str,
        weather_data: &Option<String>,
    ) -> Result<(String, bool), AgentError> {
        let tool_name = tool_call.function.name.as_str();
        let mut params = Self::parse_tool_arguments(&tool_call.function.arguments)?;

        if Self::is_mcp_search_tool(tool_name) {
            return self.execute_mcp_tool_search(params.as_ref()).await;
        }

        if Self::is_mcp_dynamic_tool_name(tool_name) {
            return self
                .execute_mcp_dynamic_tool(tool_name, params.as_ref(), input_text)
                .await;
        }

        if Self::is_builtin_workspace_tool(tool_name) {
            return self
                .execute_builtin_workspace_tool(tool_name, params.as_ref(), input_text)
                .await
                .map(|output| (output, true));
        }

        let registered = self.resolve_native_skill(tool_name, tool_name_map).await?;
        let registry = self
            .skill_registry
            .as_ref()
            .ok_or_else(|| AgentError::SkillNotFound(tool_name.to_string()))?;
        let resolved_id = registered.skill.id.clone();
        let enriched_input = self.enriched_skill_input(input_text, weather_data);
        Self::apply_react_skill_param_defaults(
            &resolved_id,
            &enriched_input,
            Some(input_text),
            &mut params,
        );
        let params_are_empty = params.as_ref().map_or(true, |p| p.is_empty());
        let skill_input = if params_are_empty {
            enriched_input.as_str()
        } else {
            input_text
        };
        let result = self
            .execute_registered_skill(&registered, skill_input, params)
            .await;

        let result = match result {
            Ok(result) => result,
            Err(e) => {
                return Err(e);
            }
        };
        let _ = registry.record_usage(&resolved_id).await;
        let output = if result.success {
            self.synthesize_skill_output(input_text, &result.output, &resolved_id)
        } else {
            Self::recoverable_skill_observation(&resolved_id, &result.output)
        };
        Ok((output, result.success))
    }

    fn mcp_dynamic_tool_name(server_name: &str, tool_name: &str) -> String {
        let raw = format!("{}:{}", server_name, tool_name);
        format!(
            "mcp__{}",
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(raw)
        )
    }

    fn mcp_display_tool_name(server_name: &str, tool_name: &str) -> String {
        format!("mcp:{}/{}", server_name, tool_name)
    }

    fn parse_mcp_display_tool_name(name: &str) -> Result<McpToolTarget, AgentError> {
        let raw = name.trim().strip_prefix("mcp:").ok_or_else(|| {
            AgentError::Execution(format!(
                "Invalid MCP tool name '{}'. Expected format mcp:server/tool",
                name
            ))
        })?;
        let (server_name, tool_name) = raw.split_once('/').ok_or_else(|| {
            AgentError::Execution(format!(
                "Invalid MCP tool name '{}'. Expected format mcp:server/tool",
                name
            ))
        })?;
        if server_name.is_empty() || tool_name.is_empty() {
            return Err(AgentError::Execution(format!(
                "Invalid MCP tool name '{}'. Expected format mcp:server/tool",
                name
            )));
        }
        Ok(McpToolTarget {
            server_name: server_name.to_string(),
            tool_name: tool_name.to_string(),
        })
    }

    fn parse_mcp_dynamic_tool_name(name: &str) -> Result<McpToolTarget, AgentError> {
        let encoded = name.strip_prefix("mcp__").ok_or_else(|| {
            AgentError::Execution(format!("Invalid MCP dynamic tool name: {}", name))
        })?;
        let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(encoded)
            .map_err(|e| AgentError::Execution(format!("Invalid MCP dynamic tool id: {}", e)))?;
        let decoded = String::from_utf8(decoded)
            .map_err(|e| AgentError::Execution(format!("Invalid MCP dynamic tool id: {}", e)))?;
        let (server_name, tool_name) = decoded.split_once(':').ok_or_else(|| {
            AgentError::Execution(format!("Invalid MCP dynamic tool target: {}", decoded))
        })?;
        if server_name.is_empty() || tool_name.is_empty() {
            return Err(AgentError::Execution(format!(
                "Invalid MCP dynamic tool target: {}",
                decoded
            )));
        }
        Ok(McpToolTarget {
            server_name: server_name.to_string(),
            tool_name: tool_name.to_string(),
        })
    }

    async fn mcp_tool_definition(
        &self,
        target: &McpToolTarget,
    ) -> Result<communication::ToolDefinition, AgentError> {
        let manager = self
            .mcp_manager
            .as_ref()
            .ok_or_else(|| AgentError::InvalidConfig("MCP manager not configured".into()))?;
        let tool = manager
            .get_tool_schema(&target.server_name, &target.tool_name)
            .await
            .map_err(|e| AgentError::Execution(format!("Failed to load MCP tool schema: {}", e)))?;
        Ok(communication::ToolDefinition {
            name: Self::mcp_dynamic_tool_name(&target.server_name, &target.tool_name),
            description: format!(
                "MCP tool {}/{}: {}",
                target.server_name,
                target.tool_name,
                tool.description.unwrap_or_default()
            ),
            parameters: tool.input_schema,
        })
    }

    fn mcp_search_score(
        query: &str,
        server_name: &str,
        tool_name: &str,
        description: &str,
    ) -> usize {
        let haystack = format!("{} {} {}", server_name, tool_name, description).to_lowercase();
        let query_lower = query.to_lowercase();
        let mut score = if haystack.contains(&query_lower) {
            10
        } else {
            0
        };
        for term in query_lower
            .split(|c: char| !c.is_alphanumeric() && c != '_' && c != '-')
            .filter(|term| term.chars().count() >= 2)
        {
            if haystack.contains(term) {
                score += 1;
            }
        }
        score
    }

    async fn execute_mcp_tool_search(
        &self,
        params: Option<&HashMap<String, String>>,
    ) -> Result<(String, bool), AgentError> {
        let exact_tool_name = Self::tool_arg_ref(params, "tool_name").map(str::trim);
        let query = Self::tool_arg_ref(params, "query").map(str::trim);
        if exact_tool_name.map_or(true, str::is_empty) && query.map_or(true, str::is_empty) {
            return Err(AgentError::Execution(
                "mcp_tool_search requires tool_name (mcp:server/tool) or query".to_string(),
            ));
        }
        let limit = Self::tool_arg_ref(params, "limit")
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(5)
            .clamp(1, 10);
        let server_filter = Self::tool_arg_ref(params, "server").map(|s| s.to_lowercase());
        let manager = self
            .mcp_manager
            .as_ref()
            .ok_or_else(|| AgentError::InvalidConfig("MCP manager not configured".into()))?;
        let summaries = manager
            .list_tool_summaries()
            .await
            .map_err(|e| AgentError::Execution(format!("MCP tool search failed: {}", e)))?;

        let selected = if let Some(tool_name) = exact_tool_name.filter(|value| !value.is_empty()) {
            let target = Self::parse_mcp_display_tool_name(tool_name)?;
            summaries
                .into_iter()
                .filter(|summary| {
                    summary.server_name == target.server_name
                        && summary.tool_name == target.tool_name
                })
                .map(|summary| {
                    let desc = summary.description.clone().unwrap_or_default();
                    (usize::MAX, summary, desc)
                })
                .collect::<Vec<_>>()
        } else {
            let query = query.unwrap_or_default();
            let mut scored = summaries
                .into_iter()
                .filter(|summary| {
                    server_filter
                        .as_ref()
                        .map_or(true, |server| summary.server_name.to_lowercase() == *server)
                })
                .map(|summary| {
                    let desc = summary.description.clone().unwrap_or_default();
                    let score = Self::mcp_search_score(
                        query,
                        &summary.server_name,
                        &summary.tool_name,
                        &desc,
                    );
                    (score, summary, desc)
                })
                .collect::<Vec<_>>();

            scored.sort_by(|a, b| b.0.cmp(&a.0));
            scored
                .into_iter()
                .filter(|(score, _, _)| *score > 0)
                .take(limit)
                .collect::<Vec<_>>()
        };

        if selected.is_empty() {
            return Ok((
                exact_tool_name
                    .filter(|value| !value.is_empty())
                    .map(|tool_name| {
                        format!(
                            "No MCP tool named '{}' was found. Use one of the catalog names in \
                             the form mcp:server/tool.",
                            tool_name
                        )
                    })
                    .unwrap_or_else(|| {
                        format!(
                            "No matching MCP tools found for query '{}'. Try a broader search \
                             query.",
                            query.unwrap_or_default()
                        )
                    }),
                false,
            ));
        }

        let mut lines = vec![
            "MCP schema lookup results. The matching tool schemas will be exposed dynamically for \
             the next tool call; call the dynamic tool names below in the next step."
                .to_string(),
        ];
        for (idx, (_, summary, desc)) in selected.iter().enumerate() {
            let display_name =
                Self::mcp_display_tool_name(&summary.server_name, &summary.tool_name);
            let dynamic_name =
                Self::mcp_dynamic_tool_name(&summary.server_name, &summary.tool_name);
            lines.push(format!(
                "{}. {} -> {} - {}",
                idx + 1,
                display_name,
                dynamic_name,
                desc.chars().take(240).collect::<String>()
            ));
        }
        Ok((lines.join("\n"), true))
    }

    async fn mcp_targets_from_search_arguments(
        &self,
        arguments: &str,
    ) -> Option<Vec<McpToolTarget>> {
        let params = Self::parse_tool_arguments(arguments).ok()?;
        let tool_name = Self::tool_arg_ref(params.as_ref(), "tool_name");
        let query = Self::tool_arg_ref(params.as_ref(), "query");
        let limit = Self::tool_arg_ref(params.as_ref(), "limit")
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(5)
            .clamp(1, 10);
        let server_filter = Self::tool_arg_ref(params.as_ref(), "server").map(|s| s.to_lowercase());
        let manager = self.mcp_manager.as_ref()?;
        let summaries = manager.list_tool_summaries().await.ok()?;

        if let Some(tool_name) = tool_name.filter(|value| !value.trim().is_empty()) {
            let target = Self::parse_mcp_display_tool_name(tool_name).ok()?;
            let exists = summaries.iter().any(|summary| {
                summary.server_name == target.server_name && summary.tool_name == target.tool_name
            });
            return if exists { Some(vec![target]) } else { None };
        }

        let query = query?;

        let mut scored = summaries
            .into_iter()
            .filter(|summary| {
                server_filter
                    .as_ref()
                    .map_or(true, |server| summary.server_name.to_lowercase() == *server)
            })
            .map(|summary| {
                let desc = summary.description.clone().unwrap_or_default();
                let score =
                    Self::mcp_search_score(query, &summary.server_name, &summary.tool_name, &desc);
                (score, summary)
            })
            .filter(|(score, _)| *score > 0)
            .collect::<Vec<_>>();
        scored.sort_by(|a, b| b.0.cmp(&a.0));

        let targets = scored
            .into_iter()
            .take(limit)
            .map(|(_, summary)| McpToolTarget {
                server_name: summary.server_name,
                tool_name: summary.tool_name,
            })
            .collect::<Vec<_>>();
        if targets.is_empty() {
            None
        } else {
            Some(targets)
        }
    }

    async fn execute_mcp_dynamic_tool(
        &self,
        tool_name: &str,
        params: Option<&HashMap<String, String>>,
        input_text: &str,
    ) -> Result<(String, bool), AgentError> {
        let target = Self::parse_mcp_dynamic_tool_name(tool_name)?;
        let manager = self
            .mcp_manager
            .as_ref()
            .ok_or_else(|| AgentError::InvalidConfig("MCP manager not configured".into()))?;
        let client = manager
            .get_client(&target.server_name)
            .await
            .ok_or_else(|| {
                AgentError::InvalidConfig(format!("MCP client '{}' not found", target.server_name))
            })?;

        let mut arguments = serde_json::Map::new();
        if let Some(params) = params {
            for (key, value) in params {
                let parsed = serde_json::from_str::<serde_json::Value>(value)
                    .unwrap_or_else(|_| serde_json::Value::String(value.clone()));
                arguments.insert(key.clone(), parsed);
            }
        }

        let skill_id = format!("mcp:{}/{}", target.server_name, target.tool_name);
        Self::apply_mcp_default_params(&skill_id, &mut arguments);

        let tool_schema = manager
            .get_tool_schema(&target.server_name, &target.tool_name)
            .await
            .map_err(|e| AgentError::Execution(format!("Failed to load MCP tool schema: {}", e)))?
            .input_schema;
        if let Err(e) = crate::mcp::skill_bridge::validate_tool_arguments(&tool_schema, &arguments)
        {
            return Ok((
                format!(
                    "MCP parameter validation failed.\nTool: {}/{}\nArguments: {}\nValidation \
                     error: {}\n\nRetry with corrected arguments or ask one concise clarification \
                     question if the missing information cannot be inferred.",
                    target.server_name,
                    target.tool_name,
                    serde_json::Value::Object(arguments),
                    e
                ),
                false,
            ));
        }

        if Self::is_high_risk_mcp_skill(&skill_id)
            && !self.skip_approval.load(std::sync::atomic::Ordering::SeqCst)
        {
            let params_json = serde_json::Value::Object(arguments.clone());
            if let Some(ref gate) = self.approval_gate {
                match gate.evaluate(&skill_id, &params_json, &Self::approval_env()) {
                    crate::security::ApprovalResult::Approved
                    | crate::security::ApprovalResult::AutoApproved { .. } => {}
                    crate::security::ApprovalResult::Rejected { reason } => {
                        let request = gate.build_request(&skill_id, &params_json, input_text);
                        let req_id = request.request_id.clone();
                        {
                            let mut pending = self.pending_approvals.write().await;
                            pending.insert(req_id, request);
                        }
                        return Ok((
                            format!(
                                "{}\n\n{}\n\n⚠️ \
                                 这是一个高风险操作，需要您的确认后才能执行。\\\
                                 n请回复「确认」或「同意」来执行此操作。",
                                Self::generate_action_preview(
                                    &skill_id,
                                    &target.tool_name,
                                    &arguments
                                ),
                                reason
                            ),
                            false,
                        ));
                    }
                    _ => {
                        let request = gate.build_request(&skill_id, &params_json, input_text);
                        let req_id = request.request_id.clone();
                        {
                            let mut pending = self.pending_approvals.write().await;
                            pending.insert(req_id, request);
                        }
                        return Ok((
                            format!(
                                "{}\n\n⚠️ 这是一个高风险操作，需要您的确认后才能执行。\\\
                                 n请回复「确认」或「同意」来执行此操作。",
                                Self::generate_action_preview(
                                    &skill_id,
                                    &target.tool_name,
                                    &arguments
                                )
                            ),
                            false,
                        ));
                    }
                }
            }
        }

        let result = client
            .call_tool(
                target.tool_name.clone(),
                if arguments.is_empty() {
                    None
                } else {
                    Some(arguments)
                },
            )
            .await
            .map_err(|e| AgentError::Execution(format!("MCP tool call failed: {}", e)))?;

        let output = result
            .content
            .iter()
            .filter_map(|content| match content {
                crate::mcp::types::ToolContent::Text { text } => Some(text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        if result.is_error {
            return Ok((
                format_mcp_call_error(
                    &skill_id,
                    if output.is_empty() {
                        "unknown error"
                    } else {
                        output.as_str()
                    },
                ),
                false,
            ));
        }

        Ok((Self::truncate_tool_output(&output, 4000), true))
    }

    async fn resolve_native_skill(
        &self,
        tool_name: &str,
        tool_name_map: &HashMap<String, String>,
    ) -> Result<skills::registry::RegisteredSkill, AgentError> {
        let registry = self
            .skill_registry
            .as_ref()
            .ok_or_else(|| AgentError::SkillNotFound(tool_name.to_string()))?;

        let mut resolved_skill = registry.get(tool_name).await;
        if resolved_skill.is_none() {
            if let Some(original_id) = tool_name_map.get(tool_name) {
                resolved_skill = registry.get(original_id).await;
            }
        }
        if resolved_skill.is_none() && !tool_name.contains(':') && !tool_name.contains('/') {
            let all_skills = registry.list_all().await;
            for skill in &all_skills {
                if skill.skill.id.ends_with(&format!("/{}", tool_name)) {
                    resolved_skill = Some(skill.clone());
                    break;
                }
            }
        }

        resolved_skill.ok_or_else(|| AgentError::SkillNotFound(tool_name.to_string()))
    }

    fn enriched_skill_input(&self, input_text: &str, weather_data: &Option<String>) -> String {
        let enriched_input = if let Some(weather) = weather_data {
            if !weather.is_empty() {
                format!(
                    "{}\n\n[参考数据] 实时天气：{}\n请基于以上数据回答。",
                    input_text, weather
                )
            } else {
                input_text.to_string()
            }
        } else {
            input_text.to_string()
        };
        enriched_input
    }

    fn parse_tool_arguments(
        arguments: &str,
    ) -> Result<Option<HashMap<String, String>>, AgentError> {
        if arguments.trim().is_empty() || arguments.trim() == "{}" {
            return Ok(None);
        }
        let map = serde_json::from_str::<HashMap<String, serde_json::Value>>(arguments)
            .map_err(|e| AgentError::Execution(format!("Invalid tool arguments: {}", e)))?;
        let string_map = map
            .into_iter()
            .map(|(k, v)| {
                let value = match v {
                    serde_json::Value::String(s) => s,
                    serde_json::Value::Bool(b) => b.to_string(),
                    serde_json::Value::Number(n) => n.to_string(),
                    other => other.to_string(),
                };
                (k, value)
            })
            .collect();
        Ok(Some(string_map))
    }

    fn apply_react_skill_param_defaults(
        skill_id: &str,
        input: &str,
        user_request: Option<&str>,
        params: &mut Option<HashMap<String, String>>,
    ) {
        let context = format!("{}\n{}", user_request.unwrap_or_default(), input);
        let lower = context.to_ascii_lowercase();
        let mentions_btc = lower.contains("btc") || lower.contains("bitcoin");
        let mentions_eth = lower.contains("eth") || lower.contains("ethereum");
        let default_crypto_symbol = if mentions_eth {
            Some("ETH/USD")
        } else if mentions_btc || skill_id.contains("crypto") || skill_id.contains("alpaca") {
            Some("BTC/USD")
        } else {
            None
        };

        if matches!(
            skill_id,
            "mcp:alpaca/get_crypto_latest_quote" | "mcp:alpaca/get_crypto_snapshot"
        ) {
            if let Some(symbol) = default_crypto_symbol {
                let params = params.get_or_insert_with(HashMap::new);
                params
                    .entry("symbols".to_string())
                    .or_insert(symbol.to_string());
                params.entry("loc".to_string()).or_insert("us".to_string());
            }
        }

        if skill_id == "mcp:alpaca/place_crypto_order" {
            let params = params.get_or_insert_with(HashMap::new);
            if let Some(symbol) = default_crypto_symbol {
                params
                    .entry("symbol".to_string())
                    .or_insert(symbol.to_string());
            }
            if lower.contains("卖") || lower.contains("sell") || lower.contains("做空") {
                params
                    .entry("side".to_string())
                    .or_insert("sell".to_string());
            } else if lower.contains("买") || lower.contains("buy") || lower.contains("做多") {
                params
                    .entry("side".to_string())
                    .or_insert("buy".to_string());
            }
            if lower.contains("市场") || lower.contains("市价") || lower.contains("market") {
                params
                    .entry("type".to_string())
                    .or_insert("market".to_string());
            }
            if !params.contains_key("notional") && !params.contains_key("qty") {
                if let Some(notional) = extract_usd_notional(&context) {
                    params.insert("notional".to_string(), notional);
                }
            }
        }
    }

    fn apply_mcp_default_params(
        skill_id: &str,
        params: &mut serde_json::Map<String, serde_json::Value>,
    ) {
        match skill_id {
            "mcp:alpaca/place_crypto_order" => {
                params
                    .entry("type".to_string())
                    .or_insert_with(|| serde_json::Value::String("market".to_string()));
                params
                    .entry("time_in_force".to_string())
                    .or_insert_with(|| serde_json::Value::String("gtc".to_string()));
            }
            "mcp:alpaca/get_crypto_latest_quote" | "mcp:alpaca/get_crypto_snapshot" => {
                params
                    .entry("loc".to_string())
                    .or_insert_with(|| serde_json::Value::String("us".to_string()));
            }
            _ => {}
        }
    }

    fn tool_arg<'a>(params: &'a Option<HashMap<String, String>>, key: &str) -> Option<&'a str> {
        params.as_ref().and_then(|p| p.get(key)).map(|s| s.as_str())
    }

    fn tool_arg_ref<'a>(params: Option<&'a HashMap<String, String>>, key: &str) -> Option<&'a str> {
        params.and_then(|p| p.get(key)).map(|s| s.as_str())
    }

    fn should_skip_walk_entry(path: &Path) -> bool {
        path.file_name()
            .and_then(|n| n.to_str())
            .map(|name| {
                matches!(
                    name,
                    ".git"
                        | "target"
                        | "node_modules"
                        | ".next"
                        | "dist"
                        | "build"
                        | ".cache"
                        | ".turbo"
                )
            })
            .unwrap_or(false)
    }

    fn is_probably_binary(bytes: &[u8]) -> bool {
        bytes.iter().take(1024).any(|b| *b == 0)
    }

    fn glob_pattern_matches(pattern: &str, text: &str) -> bool {
        fn inner(p: &[u8], t: &[u8]) -> bool {
            if p.is_empty() {
                return t.is_empty();
            }
            match p[0] {
                b'*' => {
                    if p.get(1) == Some(&b'*') {
                        let rest = &p[2..];
                        inner(rest, t) || (!t.is_empty() && inner(p, &t[1..]))
                    } else {
                        inner(&p[1..], t) || (!t.is_empty() && t[0] != b'/' && inner(p, &t[1..]))
                    }
                }
                b'?' => !t.is_empty() && t[0] != b'/' && inner(&p[1..], &t[1..]),
                byte => !t.is_empty() && byte == t[0] && inner(&p[1..], &t[1..]),
            }
        }
        inner(pattern.as_bytes(), text.as_bytes())
    }

    fn collect_workspace_files_sync(root: &Path, limit: usize) -> Result<Vec<PathBuf>, AgentError> {
        let mut files = Vec::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(path) = stack.pop() {
            if Self::should_skip_walk_entry(&path) {
                continue;
            }
            if files.len() >= limit {
                break;
            }
            if path.is_file() {
                files.push(path);
                continue;
            }
            if !path.is_dir() {
                continue;
            }
            let entries = std::fs::read_dir(&path).map_err(|e| {
                AgentError::Execution(format!("Failed to read '{}': {}", path.display(), e))
            })?;
            for entry in entries.flatten() {
                stack.push(entry.path());
            }
        }
        files.sort();
        Ok(files)
    }

    fn execute_workspace_glob_sync(
        root: &Path,
        workspace: &Path,
        pattern: &str,
        max_results: usize,
    ) -> Result<String, AgentError> {
        let files =
            Self::collect_workspace_files_sync(root, max_results.saturating_mul(20).max(200))?;
        let mut matches = Vec::new();
        for file in files {
            let rel = file
                .strip_prefix(workspace)
                .unwrap_or(&file)
                .to_string_lossy()
                .replace('\\', "/");
            if Self::glob_pattern_matches(pattern, &rel)
                || file
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(|name| Self::glob_pattern_matches(pattern, name))
                    .unwrap_or(false)
            {
                matches.push(rel);
                if matches.len() >= max_results {
                    break;
                }
            }
        }

        if matches.is_empty() {
            Ok(format!(
                "No files matched pattern '{}' under {}",
                pattern,
                root.display()
            ))
        } else {
            Ok(format!(
                "Glob: {}\nRoot: {}\nMatches: {}\n\n{}",
                pattern,
                root.display(),
                matches.len(),
                matches.join("\n")
            ))
        }
    }

    fn execute_workspace_grep_sync(
        root: &Path,
        workspace: &Path,
        pattern: &str,
        include: Option<&str>,
        case_sensitive: bool,
        max_matches: usize,
    ) -> Result<String, AgentError> {
        let regex_pattern = if case_sensitive {
            pattern.to_string()
        } else {
            format!("(?i){}", pattern)
        };
        let re = regex::Regex::new(&regex_pattern)
            .map_err(|e| AgentError::Execution(format!("Invalid grep regex: {}", e)))?;
        let files = if root.is_file() {
            vec![root.to_path_buf()]
        } else {
            Self::collect_workspace_files_sync(root, max_matches.saturating_mul(50).max(500))?
        };

        let mut lines = Vec::new();
        for file in files {
            let rel = file
                .strip_prefix(workspace)
                .unwrap_or(&file)
                .to_string_lossy()
                .replace('\\', "/");
            if let Some(include_pattern) = include {
                if !Self::glob_pattern_matches(include_pattern, &rel)
                    && !file
                        .file_name()
                        .and_then(|n| n.to_str())
                        .map(|name| Self::glob_pattern_matches(include_pattern, name))
                        .unwrap_or(false)
                {
                    continue;
                }
            }
            let bytes = match std::fs::read(&file) {
                Ok(bytes) => bytes,
                Err(_) => continue,
            };
            if Self::is_probably_binary(&bytes) {
                continue;
            }
            let content = String::from_utf8_lossy(&bytes);
            for (idx, line) in content.lines().enumerate() {
                if re.is_match(line) {
                    lines.push(format!("{}:{}:{}", rel, idx + 1, line.trim_end()));
                    if lines.len() >= max_matches {
                        break;
                    }
                }
            }
            if lines.len() >= max_matches {
                break;
            }
        }

        if lines.is_empty() {
            Ok(format!(
                "No matches for pattern '{}' under {}",
                pattern,
                root.display()
            ))
        } else {
            Ok(format!(
                "Grep: {}\nRoot: {}\nMatches: {}\n\n{}",
                pattern,
                root.display(),
                lines.len(),
                lines.join("\n")
            ))
        }
    }

    fn execute_workspace_glob(
        &self,
        pattern: &str,
        path: &str,
        max_results: usize,
    ) -> Result<String, AgentError> {
        let root = self.resolve_tool_path(if path.is_empty() { "." } else { path })?;
        let workspace = Self::normalize_path_without_fs(&self.workspace_dir());
        Self::execute_workspace_glob_sync(&root, &workspace, pattern, max_results)
    }

    fn execute_workspace_grep(
        &self,
        pattern: &str,
        path: &str,
        include: Option<&str>,
        case_sensitive: bool,
        max_matches: usize,
    ) -> Result<String, AgentError> {
        let root = self.resolve_tool_path(if path.is_empty() { "." } else { path })?;
        let workspace = Self::normalize_path_without_fs(&self.workspace_dir());
        Self::execute_workspace_grep_sync(
            &root,
            &workspace,
            pattern,
            include,
            case_sensitive,
            max_matches,
        )
    }

    fn is_blocked_web_host(host: &str) -> bool {
        let lower = host.trim_matches('.').to_lowercase();
        if matches!(lower.as_str(), "localhost" | "0.0.0.0")
            || lower.ends_with(".localhost")
            || lower.ends_with(".local")
        {
            return true;
        }
        if let Ok(ip) = lower.parse::<IpAddr>() {
            return match ip {
                IpAddr::V4(v4) => {
                    v4.is_private()
                        || v4.is_loopback()
                        || v4.is_link_local()
                        || v4.is_broadcast()
                        || v4.is_documentation()
                        || v4.octets()[0] == 0
                }
                IpAddr::V6(v6) => {
                    v6.is_loopback()
                        || v6.is_unspecified()
                        || v6.is_unique_local()
                        || v6.is_unicast_link_local()
                }
            };
        }
        false
    }

    fn validate_public_web_url(raw_url: &str) -> Result<url::Url, AgentError> {
        let parsed = url::Url::parse(raw_url)
            .map_err(|e| AgentError::Execution(format!("Invalid URL '{}': {}", raw_url, e)))?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err(AgentError::Execution(format!(
                "Blocked non-HTTP URL scheme: {}",
                parsed.scheme()
            )));
        }
        let host = parsed
            .host_str()
            .ok_or_else(|| AgentError::Execution("URL must include a host".to_string()))?;
        if Self::is_blocked_web_host(host) {
            return Err(AgentError::Execution(format!(
                "Blocked local or private web host: {}",
                host
            )));
        }
        Ok(parsed)
    }

    fn strip_html_to_text(html: &str) -> String {
        let mut without_scripts = html.to_string();
        for tag in ["script", "style", "noscript"] {
            let pattern = format!(r"(?is)<{0}[^>]*>.*?</{0}>", tag);
            if let Ok(re) = regex::Regex::new(&pattern) {
                without_scripts = re.replace_all(&without_scripts, " ").into_owned();
            }
        }
        let without_tags = regex::Regex::new("(?is)<[^>]+>")
            .map(|re| re.replace_all(&without_scripts, " ").into_owned())
            .unwrap_or(without_scripts);
        let decoded = without_tags
            .replace("&amp;", "&")
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&#39;", "'");
        regex::Regex::new(r"\s+")
            .map(|re| re.replace_all(&decoded, " ").trim().to_string())
            .unwrap_or_else(|_| decoded.trim().to_string())
    }

    async fn execute_web_fetch(
        &self,
        raw_url: &str,
        max_chars: usize,
    ) -> Result<String, AgentError> {
        if !self.web_tools_enabled() {
            return Err(AgentError::Execution("web tools are disabled".to_string()));
        }
        let url = Self::validate_public_web_url(raw_url)?;
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(20))
            .redirect(reqwest::redirect::Policy::limited(5))
            .user_agent("BeeBotOS/1.0 (+https://github.com/linc77/beebotos)")
            .build()
            .map_err(|e| AgentError::Execution(format!("Failed to build HTTP client: {}", e)))?;
        let response = client
            .get(url.clone())
            .send()
            .await
            .map_err(|e| AgentError::Execution(format!("web_fetch failed: {}", e)))?;
        let final_url = response.url().clone();
        Self::validate_public_web_url(final_url.as_str())?;
        let status = response.status();
        if !status.is_success() {
            return Err(AgentError::Execution(format!(
                "web_fetch returned HTTP {} for {}",
                status, final_url
            )));
        }
        let text = response
            .text()
            .await
            .map_err(|e| AgentError::Execution(format!("Failed to read response body: {}", e)))?;
        let cleaned = Self::strip_html_to_text(&text);
        let truncated: String = cleaned.chars().take(max_chars).collect();
        let suffix = if cleaned.chars().count() > max_chars {
            format!(
                "\n\n...[truncated, total chars: {}]",
                cleaned.chars().count()
            )
        } else {
            String::new()
        };
        Ok(format!(
            "URL: {}\nStatus: {}\n\n{}{}",
            final_url, status, truncated, suffix
        ))
    }

    fn decode_html_entities(text: &str) -> String {
        text.replace("&amp;", "&")
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&#39;", "'")
    }

    fn decode_bing_url(raw: &str) -> String {
        let decoded = Self::decode_html_entities(raw);
        if let Ok(parsed) = url::Url::parse(&decoded) {
            if parsed
                .domain()
                .map(|domain| domain.ends_with("bing.com"))
                .unwrap_or(false)
                && parsed.path() == "/ck/a"
            {
                for (key, value) in parsed.query_pairs() {
                    if key == "u" || key == "url" {
                        return value.to_string();
                    }
                }
            }
        }
        decoded
    }

    fn decode_duckduckgo_url(raw: &str) -> String {
        if let Ok(parsed) = url::Url::parse(raw) {
            if parsed.domain() == Some("duckduckgo.com") {
                for (key, value) in parsed.query_pairs() {
                    if key == "uddg" {
                        return value.to_string();
                    }
                }
            }
        }
        raw.to_string()
    }

    fn parse_bing_results(html: &str, count: usize) -> Vec<WebSearchResult> {
        let mut results = Vec::new();
        let item_re = match regex::Regex::new(
            r#"(?is)<li[^>]+class=["'][^"']*b_algo[^"']*["'][^>]*>(.*?)</li>"#,
        ) {
            Ok(re) => re,
            Err(_) => return results,
        };
        let link_re = match regex::Regex::new(
            r#"(?is)<h2[^>]*>\s*<a[^>]+href=["']([^"']+)["'][^>]*>(.*?)</a>"#,
        ) {
            Ok(re) => re,
            Err(_) => return results,
        };
        let snippet_re = match regex::Regex::new(
            r#"(?is)<p[^>]*>(.*?)</p>|<div[^>]+class=["'][^"']*b_caption[^"']*["'][^>]*>.*?<p[^>]*>(.*?)</p>"#,
        ) {
            Ok(re) => re,
            Err(_) => return results,
        };
        for item in item_re.captures_iter(html) {
            let item_html = item.get(1).map(|m| m.as_str()).unwrap_or("");
            let Some(link_caps) = link_re.captures(item_html) else {
                continue;
            };
            let raw_url = link_caps.get(1).map(|m| m.as_str()).unwrap_or("");
            let title_html = link_caps.get(2).map(|m| m.as_str()).unwrap_or("");
            let url = Self::decode_bing_url(raw_url);
            let title = Self::strip_html_to_text(title_html);
            let snippet = snippet_re
                .captures(item_html)
                .and_then(|caps| caps.get(1).or_else(|| caps.get(2)))
                .map(|m| Self::strip_html_to_text(m.as_str()))
                .unwrap_or_default();
            if title.is_empty() || url.is_empty() {
                continue;
            }
            results.push(WebSearchResult {
                title,
                url,
                snippet,
            });
            if results.len() >= count {
                break;
            }
        }
        results
    }

    fn parse_duckduckgo_results(html: &str, count: usize) -> Vec<WebSearchResult> {
        let mut results = Vec::new();
        let link_re = match regex::Regex::new(
            r#"(?is)<a[^>]+class=["'][^"']*result__a[^"']*["'][^>]*href=["']([^"']+)["'][^>]*>(.*?)</a>"#,
        ) {
            Ok(re) => re,
            Err(_) => return results,
        };
        let snippet_re = match regex::Regex::new(
            r#"(?is)<a[^>]+class=["'][^"']*result__snippet[^"']*["'][^>]*>(.*?)</a>|<div[^>]+class=["'][^"']*result__snippet[^"']*["'][^>]*>(.*?)</div>"#,
        ) {
            Ok(re) => re,
            Err(_) => return results,
        };
        for caps in link_re.captures_iter(html) {
            let raw_url = caps.get(1).map(|m| m.as_str()).unwrap_or("");
            let title_html = caps.get(2).map(|m| m.as_str()).unwrap_or("");
            let url = Self::decode_duckduckgo_url(&Self::decode_html_entities(raw_url));
            let title = Self::strip_html_to_text(title_html);
            if title.is_empty() || url.is_empty() {
                continue;
            }
            let snippet = snippet_re
                .captures(html)
                .and_then(|caps| caps.get(1).or_else(|| caps.get(2)))
                .map(|m| Self::strip_html_to_text(m.as_str()))
                .unwrap_or_default();
            results.push(WebSearchResult {
                title,
                url,
                snippet,
            });
            if results.len() >= count {
                break;
            }
        }
        results
    }

    async fn fetch_bing_search_results(
        client: &reqwest::Client,
        query: &str,
        count: usize,
    ) -> Result<Vec<WebSearchResult>, WebSearchFailure> {
        let url = format!(
            "https://cn.bing.com/search?q={}&ensearch=0",
            urlencoding::encode(query)
        );
        let parsed_url = Self::validate_public_web_url(&url).map_err(|e| WebSearchFailure {
            provider: "bing",
            reason: e.to_string(),
        })?;
        let response = client
            .get(parsed_url)
            .send()
            .await
            .map_err(|e| WebSearchFailure {
                provider: "bing",
                reason: format!("request failed: {}", e),
            })?;
        let final_url = response.url().clone();
        Self::validate_public_web_url(final_url.as_str()).map_err(|e| WebSearchFailure {
            provider: "bing",
            reason: e.to_string(),
        })?;
        let status = response.status();
        if !status.is_success() {
            return Err(WebSearchFailure {
                provider: "bing",
                reason: format!("HTTP {}", status),
            });
        }
        let html = response.text().await.map_err(|e| WebSearchFailure {
            provider: "bing",
            reason: format!("failed to read response: {}", e),
        })?;
        let results = Self::parse_bing_results(&html, count);
        if results.is_empty() {
            return Err(WebSearchFailure {
                provider: "bing",
                reason: "no parseable results".to_string(),
            });
        }
        Ok(results)
    }

    async fn fetch_duckduckgo_search_results(
        client: &reqwest::Client,
        query: &str,
        count: usize,
    ) -> Result<Vec<WebSearchResult>, WebSearchFailure> {
        let url = format!(
            "https://html.duckduckgo.com/html/?q={}",
            urlencoding::encode(query)
        );
        let parsed_url = Self::validate_public_web_url(&url).map_err(|e| WebSearchFailure {
            provider: "duckduckgo",
            reason: e.to_string(),
        })?;
        let response = client
            .get(parsed_url)
            .send()
            .await
            .map_err(|e| WebSearchFailure {
                provider: "duckduckgo",
                reason: format!("request failed: {}", e),
            })?;
        let final_url = response.url().clone();
        Self::validate_public_web_url(final_url.as_str()).map_err(|e| WebSearchFailure {
            provider: "duckduckgo",
            reason: e.to_string(),
        })?;
        let status = response.status();
        if !status.is_success() {
            return Err(WebSearchFailure {
                provider: "duckduckgo",
                reason: format!("HTTP {}", status),
            });
        }
        let html = response.text().await.map_err(|e| WebSearchFailure {
            provider: "duckduckgo",
            reason: format!("failed to read response: {}", e),
        })?;
        let results = Self::parse_duckduckgo_results(&html, count);
        if results.is_empty() {
            return Err(WebSearchFailure {
                provider: "duckduckgo",
                reason: "no parseable results; it may be blocking automated search".to_string(),
            });
        }
        Ok(results)
    }

    async fn execute_web_search(
        &self,
        query: &str,
        count: usize,
        _country: Option<&str>,
        _language: Option<&str>,
        _freshness: Option<&str>,
    ) -> Result<String, AgentError> {
        if !self.web_tools_enabled() {
            return Err(AgentError::Execution("web tools are disabled".to_string()));
        }
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(20))
            .redirect(reqwest::redirect::Policy::limited(5))
            .user_agent("BeeBotOS/1.0 (+https://github.com/linc77/beebotos)")
            .build()
            .map_err(|e| AgentError::Execution(format!("Failed to build HTTP client: {}", e)))?;

        let mut failures = Vec::new();
        let (provider, results) = match Self::fetch_bing_search_results(&client, query, count).await
        {
            Ok(results) => ("bing", results),
            Err(bing_error) => {
                failures.push(bing_error);
                match Self::fetch_duckduckgo_search_results(&client, query, count).await {
                    Ok(results) => ("duckduckgo", results),
                    Err(duckduckgo_error) => {
                        failures.push(duckduckgo_error);
                        let reasons = failures
                            .into_iter()
                            .map(|failure| format!("{}: {}", failure.provider, failure.reason))
                            .collect::<Vec<_>>()
                            .join("; ");
                        return Err(AgentError::Execution(format!(
                            "web_search failed after trying Bing and DuckDuckGo: {}",
                            reasons
                        )));
                    }
                }
            }
        };

        let mut lines = vec![format!(
            "Provider: {}\nQuery: {}\nResults: {}",
            provider,
            query,
            results.len()
        )];
        for (idx, result) in results.into_iter().enumerate() {
            if result.snippet.is_empty() {
                lines.push(format!("{}. {}\n   {}", idx + 1, result.title, result.url));
            } else {
                lines.push(format!(
                    "{}. {}\n   {}\n   {}",
                    idx + 1,
                    result.title,
                    result.url,
                    result.snippet
                ));
            }
        }
        Ok(lines.join("\n"))
    }

    async fn activate_skill_context(
        &self,
        session_key: &str,
        skill_id: &str,
    ) -> Result<String, AgentError> {
        let registry = self
            .skill_registry
            .as_ref()
            .ok_or_else(|| AgentError::InvalidConfig("Skill registry not configured".into()))?;
        let skill = registry
            .get(skill_id)
            .await
            .ok_or_else(|| AgentError::SkillNotFound(skill_id.to_string()))?;
        if !skill.enabled {
            return Err(AgentError::Execution(format!(
                "Skill '{}' is disabled",
                skill_id
            )));
        }
        let doc = registry
            .get_skill_description(skill_id, crate::skills::registry::SkillDisclosureLevel::L3)
            .await
            .unwrap_or_else(|| skill.skill.manifest.prompt_template.clone());
        let context = format!(
            "## Active Skill: {} (`{}`)\n{}\n\nCapabilities: {}",
            skill.skill.name,
            skill.skill.id,
            if doc.trim().is_empty() {
                skill.skill.manifest.description.clone()
            } else {
                doc
            },
            skill.skill.manifest.capabilities.join(", ")
        );
        {
            let mut active = self.active_skill_contexts.write().await;
            let entry = active.entry(session_key.to_string()).or_default();
            if !entry
                .iter()
                .any(|item| item.contains(&format!("`{}`", skill_id)))
            {
                entry.push(context);
            }
        }
        let _ = registry.record_usage(skill_id).await;
        Ok(format!(
            "Activated skill '{}' for session '{}'. Future turns will include its context.",
            skill_id, session_key
        ))
    }

    async fn build_skill_index_context(&self) -> String {
        let Some(registry) = &self.skill_registry else {
            return "No skill registry is configured.".to_string();
        };
        let mut skills = registry.list_enabled().await;
        skills.retain(|skill| !skill.skill.id.starts_with("mcp:"));
        skills.sort_by(|a, b| a.skill.id.cmp(&b.skill.id));
        if skills.is_empty() {
            return "No enabled skills are available.".to_string();
        }
        let mut lines = vec![
            "Available skills are context packs, not executable tools. If one is relevant, call \
             activate_skill with its exact id before using its guidance."
                .to_string(),
        ];
        for skill in skills.iter().take(80) {
            let l2 = registry
                .get_skill_description(
                    &skill.skill.id,
                    crate::skills::registry::SkillDisclosureLevel::L2,
                )
                .await
                .unwrap_or_else(|| skill.skill.manifest.description.clone());
            lines.push(format!(
                "- `{}`: {}",
                skill.skill.id,
                l2.chars().take(220).collect::<String>()
            ));
        }
        if skills.len() > 80 {
            lines.push(format!("...{} more skills omitted.", skills.len() - 80));
        }
        lines.join("\n")
    }

    async fn build_mcp_tool_index_context(&self) -> Option<String> {
        let Some(manager) = &self.mcp_manager else {
            return None;
        };

        let mut summaries = match manager.list_tool_summaries().await {
            Ok(summaries) => summaries,
            Err(e) => {
                warn!("Failed to build MCP tool catalog: {}", e);
                return Some(
                    "MCP manager is configured, but the MCP tool catalog could not be loaded. \
                     Retry mcp_tool_search when an MCP capability is needed."
                        .to_string(),
                );
            }
        };
        if summaries.is_empty() {
            return Some("No MCP tools are currently connected.".to_string());
        }

        summaries
            .sort_by(|a, b| (&a.server_name, &a.tool_name).cmp(&(&b.server_name, &b.tool_name)));
        let total = summaries.len();
        let mut lines = vec![
            "Available MCP tools are listed by catalog name only. To use one, first call \
             mcp_tool_search with tool_name set to the exact `mcp:server/tool` name; the runtime \
             will then load that tool's schema and expose a dynamic callable tool for the next \
             step."
                .to_string(),
        ];
        for summary in summaries.iter().take(200) {
            let display_name =
                Self::mcp_display_tool_name(&summary.server_name, &summary.tool_name);
            let desc = summary
                .description
                .as_deref()
                .unwrap_or("")
                .chars()
                .take(180)
                .collect::<String>();
            if desc.trim().is_empty() {
                lines.push(format!("- `{}`", display_name));
            } else {
                lines.push(format!("- `{}`: {}", display_name, desc));
            }
        }
        if total > 200 {
            lines.push(format!("...{} more MCP tools omitted.", total - 200));
        }
        Some(lines.join("\n"))
    }

    async fn active_skill_context_for_session(&self, session_key: &str) -> Option<String> {
        let active = self.active_skill_contexts.read().await;
        let contexts = active.get(session_key)?;
        if contexts.is_empty() {
            None
        } else {
            Some(contexts.join("\n\n"))
        }
    }

    async fn execute_react_tool_call(
        &self,
        tool_call: &crate::llm::ToolCall,
        session_key: &str,
        input_text: &str,
        context_params: &HashMap<String, String>,
    ) -> Result<String, AgentError> {
        let tool_name = tool_call.function.name.as_str();
        let params = Self::parse_tool_arguments(&tool_call.function.arguments)?;
        match tool_name {
            "mcp_tool_search" => self
                .execute_mcp_tool_search(params.as_ref())
                .await
                .map(|(output, _)| output),
            _ if Self::is_mcp_dynamic_tool_name(tool_name) => self
                .execute_mcp_dynamic_tool(tool_name, params.as_ref(), input_text)
                .await
                .map(|(output, _)| output),
            "activate_skill" => {
                let skill_id = Self::tool_arg(&params, "skill_id")
                    .ok_or_else(|| {
                        AgentError::Execution("activate_skill requires skill_id".to_string())
                    })?
                    .trim();
                self.activate_skill_context(session_key, skill_id).await
            }
            "web_fetch" => {
                let url = Self::tool_arg(&params, "url")
                    .ok_or_else(|| AgentError::Execution("web_fetch requires url".to_string()))?;
                let max_chars = Self::tool_arg(&params, "max_chars")
                    .and_then(|s| s.parse::<usize>().ok())
                    .unwrap_or(12000)
                    .clamp(1, 50000);
                self.execute_web_fetch(url, max_chars).await
            }
            "web_search" => {
                let query = Self::tool_arg(&params, "query").ok_or_else(|| {
                    AgentError::Execution("web_search requires query".to_string())
                })?;
                let count = Self::tool_arg(&params, "count")
                    .and_then(|s| s.parse::<usize>().ok())
                    .unwrap_or(5)
                    .clamp(1, 10);
                self.execute_web_search(
                    query,
                    count,
                    Self::tool_arg(&params, "country"),
                    Self::tool_arg(&params, "language"),
                    Self::tool_arg(&params, "freshness"),
                )
                .await
            }
            "session_search" => {
                let query = Self::tool_arg(&params, "query").ok_or_else(|| {
                    AgentError::Execution("session_search requires query".to_string())
                })?;
                let limit = Self::tool_arg(&params, "limit")
                    .and_then(|s| s.parse::<usize>().ok())
                    .unwrap_or(5)
                    .clamp(1, 10);
                let user_id = context_params
                    .get("user_id")
                    .map(|s| s.as_str())
                    .ok_or_else(|| {
                        AgentError::Execution(
                            "session_search unavailable: current user_id is missing".to_string(),
                        )
                    })?;
                let exclude_session_id = context_params
                    .get("db_session_id")
                    .or_else(|| context_params.get("session_id"))
                    .map(|s| s.as_str());
                self.execute_session_search(query, user_id, exclude_session_id, limit)
                    .await
            }
            _ if Self::is_builtin_workspace_tool(tool_name) => {
                self.execute_builtin_workspace_tool(tool_name, params.as_ref(), input_text)
                    .await
            }
            _ => Err(AgentError::Execution(format!(
                "Unknown ReAct tool '{}'. Skills must be activated with activate_skill, not \
                 executed as tools.",
                tool_name
            ))),
        }
    }

    async fn execute_session_search(
        &self,
        query: &str,
        user_id: &str,
        exclude_session_id: Option<&str>,
        limit: usize,
    ) -> Result<String, AgentError> {
        let provider = self.system_info_provider.as_ref().ok_or_else(|| {
            AgentError::Execution("session_search is unavailable in this runtime".to_string())
        })?;
        let hits = provider
            .search_sessions(query, user_id, limit.clamp(1, 10), exclude_session_id)
            .await
            .map_err(|e| AgentError::Execution(format!("session_search failed: {}", e)))?;
        if hits.is_empty() {
            return Ok("Session search found no previous matching messages.".to_string());
        }

        let mut lines = vec!["Session search results:".to_string()];
        for (idx, hit) in hits.iter().enumerate() {
            let mut snippet: String = hit.content.chars().take(300).collect();
            if hit.content.chars().count() > 300 {
                snippet.push_str("...");
            }
            lines.push(format!(
                "{}. score={:.2} session={} title={} role={} at={} message={}\n{}",
                idx + 1,
                hit.score,
                hit.session_id,
                hit.session_title,
                hit.role,
                hit.created_at,
                hit.message_id,
                snippet
            ));
        }
        Ok(lines.join("\n"))
    }

    fn assistant_tool_call_message(
        tool_calls: &[crate::llm::ToolCall],
        reasoning_content: Option<&str>,
    ) -> communication::Message {
        let mut metadata = HashMap::new();
        metadata.insert("role".to_string(), "assistant".to_string());
        if let Ok(json) = serde_json::to_string(tool_calls) {
            metadata.insert("tool_calls_json".to_string(), json);
        }
        if let Some(reasoning_content) = reasoning_content {
            metadata.insert(
                "reasoning_content".to_string(),
                reasoning_content.to_string(),
            );
        }
        communication::Message::with_metadata(
            uuid::Uuid::new_v4(),
            communication::PlatformType::Custom,
            "",
            metadata,
        )
    }

    fn tool_result_message(
        tool_call_id: &str,
        result: impl Into<String>,
    ) -> communication::Message {
        let mut metadata = HashMap::new();
        metadata.insert("role".to_string(), "tool".to_string());
        metadata.insert("tool_call_id".to_string(), tool_call_id.to_string());
        communication::Message::with_metadata(
            uuid::Uuid::new_v4(),
            communication::PlatformType::Custom,
            result,
            metadata,
        )
    }

    async fn emit_react_trace(&self, event: crate::react_trace::ReActTraceEvent) {
        if let Some(sink) = &self.react_trace_sink {
            if let Err(e) = sink.emit(event).await {
                warn!("Failed to emit ReAct trace event: {}", e);
            }
        }
    }

    fn react_run_id(task_id: &str) -> String {
        format!("react-{}-{}", task_id, uuid::Uuid::new_v4())
    }

    fn tool_arguments_preview(tool_call: &crate::llm::ToolCall) -> String {
        crate::react_trace::sanitize_preview(&tool_call.function.arguments)
    }

    fn recoverable_tool_error(tool_name: &str, error: impl std::fmt::Display) -> String {
        crate::react_trace::format_tool_error(tool_name, error)
    }

    fn recoverable_skill_observation(skill_id: &str, output: &str) -> String {
        format!(
            "Skill '{}' returned a recoverable failure.\nObservation: {}\n\nDo not treat this as \
             the final answer. Use this observation to retry with corrected parameters, choose a \
             more appropriate tool/skill, or ask the user one concise clarification question if \
             the missing information cannot be inferred.",
            skill_id,
            output.trim()
        )
    }

    async fn force_final_react_answer(
        &self,
        llm: &Arc<dyn communication::LLMCallInterface>,
        mut loop_messages: Vec<communication::Message>,
        tools: Vec<communication::ToolDefinition>,
        extra_params: HashMap<String, String>,
        max_tool_rounds: u32,
    ) -> Result<String, AgentError> {
        let mut metadata = HashMap::new();
        metadata.insert("role".to_string(), "system".to_string());
        loop_messages.push(communication::Message::with_metadata(
            uuid::Uuid::new_v4(),
            communication::PlatformType::Custom,
            format!(
                "The ReAct/tool loop has reached the maximum of {} rounds. Do not call any more \
                 tools. Produce the best final answer based on the observations already available.",
                max_tool_rounds
            ),
            metadata,
        ));

        let mut final_params = extra_params;
        final_params.insert("tool_choice".to_string(), "none".to_string());
        const FINAL_ANSWER_TIMEOUT_SECS: u64 = 180;
        let turn = tokio::time::timeout(
            std::time::Duration::from_secs(FINAL_ANSWER_TIMEOUT_SECS),
            llm.call_llm_tool_turn(loop_messages, tools, Some(final_params)),
        )
        .await
        .map_err(|_| {
            AgentError::Execution(format!(
                "Forced final answer timed out after {}s",
                FINAL_ANSWER_TIMEOUT_SECS
            ))
        })?
        .map_err(|e| AgentError::Execution(format!("Forced final answer failed: {}", e)))?;
        Ok(Self::cleanup_thinking_process(&turn.content))
    }

    /// Extract user input text from a task.
    /// If input is JSON with a "message" field, extracts the message.
    /// Otherwise returns the raw input.
    pub fn extract_user_input(task: &crate::Task) -> String {
        match serde_json::from_str::<serde_json::Value>(&task.input) {
            Ok(val) if val.get("message").is_some() => {
                val["message"].as_str().unwrap_or(&task.input).to_string()
            }
            _ => task.input.clone(),
        }
    }

    pub fn new(config: AgentConfig) -> Self {
        Self {
            config,
            a2a_client: None,
            mcp_manager: None,
            outbound_router: None,
            message_rx: None,
            queue_manager: None,
            skill_registry: None,
            llm_interface: None,
            state: Mutex::new(state_manager::AgentState::Registered),
            wallet: None,    // 🔒 P0 FIX: Initialize wallet as None
            event_bus: None, // 🟢 P1 FIX: Initialize event bus as None
            kernel: None,    // 🔒 P0 FIX: Initialize kernel as None
            // 🆕 PLANNING FIX: Initialize planning components as None
            planning_engine: None,
            plan_executor: None,
            replanner: None,
            active_plans: Arc::new(RwLock::new(HashMap::new())),
            // 🆕 DEVICE FIX: Initialize device as None
            device: None,
            // 🟢 P1 FIX: Initialize memory system as None
            memory_system: None,
            // 🟢 P2 FIX: Initialize LLM response cache
            llm_response_cache: Arc::new(RwLock::new(HashMap::new())),
            // 🆕 FIX: Initialize current plan goal
            current_plan_goal: Arc::new(RwLock::new(None)),
            // 🆕 FIX: Initialize skill catalog
            skill_catalog: None,
            // 🟢 P1 FIX: Initialize workflow registry as None
            workflow_registry: None,
            // 🆕 OPTIMIZATION: Initialize new components
            intent_engine: Some(crate::intent::IntentEngine::new()),
            // 🆕 SKILL MATCHING V2: Initialize as None — will be built lazily when LLM is available
            llm_intent_analyzer: None,
            skill_selector: None,
            trace_store: None,
            approval_gate: Some(crate::security::ApprovalGate::with_paper_trading_rules()),
            pending_approvals: Arc::new(RwLock::new(HashMap::new())),
            pending_parameter_forms: Arc::new(RwLock::new(HashMap::new())),
            skip_approval: std::sync::atomic::AtomicBool::new(false),
            prompt_cache: Some(Arc::new(crate::prompt::PromptCache::new())),
            max_rounds: crate::react_trace::DEFAULT_REACT_MAX_TOOL_ROUNDS,
            skill_feedback_collector: Some(crate::skills::feedback::SkillImprovementEngine::new()),
            evolution_scheduler: None,
            system_info_provider: None,
            tool_work_dir: std::path::PathBuf::from("data/workspace"),
            llm_client: None,
            react_trace_sink: None,
            active_skill_contexts: Arc::new(RwLock::new(HashMap::new())),
            foreign_rt_manager: None,
        }
    }

    /// Set the evolution scheduler
    pub fn with_evolution_scheduler(
        mut self,
        scheduler: crate::evolution::scheduler::EvolutionScheduler,
    ) -> Self {
        self.evolution_scheduler = Some(scheduler);
        self
    }

    /// Spawn a sub-agent with shared infrastructure.
    /// The sub-agent inherits kernel, LLM, skill registry, wallet, memory from
    /// the parent.
    pub fn spawn_sub_agent(&self, mut config: AgentConfig) -> Result<Agent, AgentError> {
        config.id = format!("{}-sub-{}", self.config.id, uuid::Uuid::new_v4());
        let mut child = Agent::new(config);
        // Share parent's infrastructure
        child.kernel = self.kernel.clone();
        child.llm_interface = self.llm_interface.clone();
        child.skill_registry = self.skill_registry.clone();
        child.wallet = self.wallet.clone();
        child.memory_system = self.memory_system.clone();
        child.event_bus = self.event_bus.clone();
        child.outbound_router = self.outbound_router.clone();
        child.queue_manager = self.queue_manager.clone();
        child.workflow_registry = self.workflow_registry.clone();
        child.system_info_provider = self.system_info_provider.clone();
        child.tool_work_dir = self.tool_work_dir.clone();
        child.llm_client = self.llm_client.clone();
        child.react_trace_sink = self.react_trace_sink.clone();
        info!(
            "Spawned sub-agent {} from parent {}",
            child.config.id, self.config.id
        );
        Ok(child)
    }

    /// Set the tool working directory for sandboxed file operations.
    pub fn with_tool_work_dir(mut self, dir: impl Into<std::path::PathBuf>) -> Self {
        self.tool_work_dir = dir.into();
        self
    }

    /// Attach a direct LLM client for native tool calling.
    pub fn with_llm_client(mut self, client: Arc<crate::llm::LLMClient>) -> Self {
        self.llm_client = Some(client);
        self
    }

    pub fn with_a2a(mut self, client: a2a::A2AClient) -> Self {
        self.a2a_client = Some(client);
        self
    }

    pub fn with_mcp(mut self, manager: Arc<mcp::MCPManager>) -> Self {
        self.mcp_manager = Some(manager);
        self
    }

    pub fn with_outbound_router(
        mut self,
        router: Arc<communication::OutboundMessageRouter>,
    ) -> Self {
        self.outbound_router = Some(router);
        self
    }

    pub fn with_message_rx(
        mut self,
        rx: tokio::sync::mpsc::Receiver<communication::UserMessageContext>,
    ) -> Self {
        self.message_rx = Some(rx);
        self
    }

    pub fn outbound_router(&self) -> Option<&Arc<communication::OutboundMessageRouter>> {
        self.outbound_router.as_ref()
    }

    pub fn has_outbound_router(&self) -> bool {
        self.outbound_router.is_some()
    }

    /// Takes ownership of the message receiver (can only be called once).
    pub fn take_message_rx(
        &mut self,
    ) -> Option<tokio::sync::mpsc::Receiver<communication::UserMessageContext>> {
        self.message_rx.take()
    }

    pub fn message_rx_ref(
        &self,
    ) -> Option<&tokio::sync::mpsc::Receiver<communication::UserMessageContext>> {
        self.message_rx.as_ref()
    }

    pub fn has_message_rx(&self) -> bool {
        self.message_rx.is_some()
    }

    pub fn with_queue_manager(mut self, manager: Arc<queue::QueueManager>) -> Self {
        self.queue_manager = Some(manager);
        self
    }

    pub fn with_skill_registry(mut self, registry: Arc<skills::SkillRegistry>) -> Self {
        self.skill_registry = Some(registry);
        self
    }

    pub fn with_llm_interface(
        mut self,
        interface: Arc<dyn communication::LLMCallInterface>,
    ) -> Self {
        self.llm_interface = Some(interface.clone());
        // 🆕 SKILL MATCHING V2: Auto-build LLM intent analyzer when LLM interface is
        // set 🆕 FIX: Timeout 5s → 20s to prevent frequent fallback to legacy
        // path.
        self.llm_intent_analyzer = Some(Arc::new(
            crate::skill_matching::LLMIntentAnalyzer::new(interface.clone())
                .with_timeout(std::time::Duration::from_secs(20)),
        ));
        self
    }

    /// 🆕 SKILL MATCHING V2: Set skill selector (auto-built if skill_registry
    /// is also set)
    pub fn with_skill_selector(
        mut self,
        selector: Arc<crate::skill_matching::SkillSelector>,
    ) -> Self {
        self.skill_selector = Some(selector);
        self
    }

    /// 🆕 SKILL MATCHING V2: Set trace store for activation observability
    pub fn with_trace_store(mut self, store: Arc<dyn crate::skill_matching::TraceStore>) -> Self {
        self.trace_store = Some(store);
        self
    }

    /// 🔒 P0 FIX: Set wallet for on-chain transactions
    pub fn with_wallet(mut self, wallet: Arc<wallet::AgentWallet>) -> Self {
        self.wallet = Some(wallet);
        info!("Wallet configured for agent {}", self.config.id);
        self
    }

    /// 🔒 P0 FIX: Get wallet reference
    pub fn wallet(&self) -> Option<&Arc<wallet::AgentWallet>> {
        self.wallet.as_ref()
    }

    /// 🔒 P0 FIX: Check if agent has wallet configured
    pub fn has_wallet(&self) -> bool {
        self.wallet.is_some()
    }

    /// 🟢 P1 FIX: Set event bus for unified event system
    pub fn with_event_bus(mut self, event_bus: events::AgentEventBus) -> Self {
        self.event_bus = Some(event_bus);
        info!("Event bus configured for agent {}", self.config.id);
        self
    }

    /// 🟢 P1 FIX: Get event bus reference
    pub fn event_bus(&self) -> Option<&events::AgentEventBus> {
        self.event_bus.as_ref()
    }

    /// 🟢 P1 FIX: Check if agent has event bus configured
    pub fn has_event_bus(&self) -> bool {
        self.event_bus.is_some()
    }

    /// 🔒 P0 FIX: Set kernel for WASM sandbox execution
    pub fn with_kernel(mut self, kernel: Arc<beebotos_kernel::Kernel>) -> Self {
        self.kernel = Some(kernel);
        info!("Kernel configured for agent {}", self.config.id);
        self
    }

    /// 🔒 P0 FIX: Get kernel reference
    pub fn kernel(&self) -> Option<&Arc<beebotos_kernel::Kernel>> {
        self.kernel.as_ref()
    }

    /// 🔒 P0 FIX: Check if agent has kernel configured
    pub fn has_kernel(&self) -> bool {
        self.kernel.is_some()
    }

    /// 🆕 PLANNING FIX: Set planning engine for autonomous planning
    pub fn with_planning_engine(mut self, engine: Arc<PlanningEngine>) -> Self {
        self.planning_engine = Some(engine);
        info!("Planning engine configured for agent {}", self.config.id);
        self
    }

    /// 🆕 PLANNING FIX: Get planning engine reference
    pub fn planning_engine(&self) -> Option<&Arc<PlanningEngine>> {
        self.planning_engine.as_ref()
    }

    /// 🆕 PLANNING FIX: Check if agent has planning engine configured
    pub fn has_planning_engine(&self) -> bool {
        self.planning_engine.is_some()
    }

    /// 🆕 PLANNING FIX: Set plan executor
    pub fn with_plan_executor(mut self, executor: Arc<PlanExecutor>) -> Self {
        self.plan_executor = Some(executor);
        info!("Plan executor configured for agent {}", self.config.id);
        self
    }

    /// 🆕 PLANNING FIX: Get plan executor reference
    pub fn plan_executor(&self) -> Option<&Arc<PlanExecutor>> {
        self.plan_executor.as_ref()
    }

    /// 🆕 PLANNING FIX: Check if agent has plan executor configured
    pub fn has_plan_executor(&self) -> bool {
        self.plan_executor.is_some()
    }

    /// 🆕 PLANNING FIX: Set replanner for dynamic replanning
    pub fn with_replanner(mut self, replanner: Arc<dyn RePlanner>) -> Self {
        self.replanner = Some(replanner);
        info!("RePlanner configured for agent {}", self.config.id);
        self
    }

    /// 🆕 PLANNING FIX: Get replanner reference
    pub fn replanner(&self) -> Option<&Arc<dyn RePlanner>> {
        self.replanner.as_ref()
    }

    /// 🆕 PLANNING FIX: Check if agent has replanner configured
    pub fn has_replanner(&self) -> bool {
        self.replanner.is_some()
    }

    /// 🆕 PLANNING FIX: Check if planning module is fully configured
    pub fn is_planning_ready(&self) -> bool {
        self.has_planning_engine() && self.has_plan_executor()
    }

    /// 🆕 P2 FIX: Auto-detect multi-step intent and build a SkillPipeline
    ///
    /// Scans the user message for sequencing keywords (先/再/然后/first/then)
    /// and explicit skill references. If 2+ known skills are found in sequence,
    /// returns a `SkillPipeline` that chains them with PassThrough mapping.
    pub async fn try_build_auto_pipeline(&self, message: &str) -> Option<SkillPipeline> {
        let registry = self.skill_registry.as_ref()?;
        let skills = registry.list_enabled().await;
        if skills.len() < 2 {
            return None;
        }

        let lower_msg = message.to_lowercase();

        // Sequencing keywords: Chinese and English
        let has_sequence_indicator =
            (lower_msg.contains("先") || lower_msg.contains("首先") || lower_msg.contains("first"))
                && (lower_msg.contains("再")
                    || lower_msg.contains("然后")
                    || lower_msg.contains("接着")
                    || lower_msg.contains("最后")
                    || lower_msg.contains("then")
                    || lower_msg.contains("next")
                    || lower_msg.contains("after"));

        let has_pipeline_keywords = lower_msg.contains("pipeline")
            || lower_msg.contains("chain")
            || lower_msg.contains("流水线")
            || lower_msg.contains("串联");

        if !has_sequence_indicator && !has_pipeline_keywords {
            return None;
        }

        // Find all skill references in the message, in order of appearance
        let mut matched_skills: Vec<(usize, String)> = Vec::new();

        // Deduplicate and sort by position in message
        matched_skills.sort_by_key(|(pos, _id)| *pos);
        matched_skills.dedup_by(|a, b| a.1 == b.1);

        if matched_skills.len() < 2 {
            return None;
        }

        let steps: Vec<PipelineStep> = matched_skills
            .into_iter()
            .map(|(_pos, skill_id)| PipelineStep {
                skill_id,
                input_mapping: InputMapping::PassThrough,
                output_schema: None,
            })
            .collect();

        info!(
            "Auto-pipeline built with {} skills for message: {}",
            steps.len(),
            message.chars().take(60).collect::<String>()
        );
        Some(SkillPipeline::new(steps))
    }

    /// 🆕 OPTIMIZATION PHASE 3: Attempt to parse and execute a tool chain from
    /// LLM response
    ///
    /// Detects format like:
    ///   STEP 1: get_stock_latest_quote|{"symbols":"AAPL"}
    ///   IF result.price > 180 THEN
    ///   STEP 2: place_stock_order|{"symbol":"AAPL","side":"buy","qty":"10"}
    async fn try_execute_tool_chain(
        &self,
        response: &str,
        _original_input: &str,
    ) -> Result<Option<String>, AgentError> {
        use crate::planning::{ToolChainParser, ToolChainStep};

        // Try to parse the response as a tool chain
        let chain = match ToolChainParser::parse(response) {
            Ok(chain) => chain,
            Err(_) => return Ok(None), // Not a tool chain format
        };

        if chain.steps.is_empty() {
            return Ok(None);
        }

        info!("Tool chain detected with {} steps", chain.steps.len());
        let mut step_results: Vec<String> = Vec::new();

        for (i, step) in chain.steps.iter().enumerate() {
            match step {
                ToolChainStep::Call(tool_call) => {
                    let tool_input = if tool_call.params.is_null()
                        || tool_call.params == serde_json::Value::Null
                    {
                        String::new()
                    } else {
                        tool_call.params.to_string()
                    };
                    let result = self
                        .execute_tool_by_name(&tool_call.name, &tool_input)
                        .await?;
                    step_results.push(format!("Step {} ({}): {}", i + 1, tool_call.name, result));
                }
                ToolChainStep::Conditional(cond) => {
                    // Evaluate condition against previous step results
                    let condition_met =
                        self.evaluate_tool_chain_condition(&cond.condition, &step_results);
                    let branch = if condition_met {
                        &cond.if_true
                    } else {
                        &cond.if_false
                    };
                    for (j, branch_step) in branch.iter().enumerate() {
                        if let ToolChainStep::Call(tool_call) = branch_step {
                            let tool_input = if tool_call.params.is_null()
                                || tool_call.params == serde_json::Value::Null
                            {
                                String::new()
                            } else {
                                tool_call.params.to_string()
                            };
                            let result = self
                                .execute_tool_by_name(&tool_call.name, &tool_input)
                                .await?;
                            step_results.push(format!(
                                "Step {}.{} ({}): {}",
                                i + 1,
                                j + 1,
                                tool_call.name,
                                result
                            ));
                        }
                    }
                }
                ToolChainStep::Wait { step_ref } => {
                    step_results.push(format!("Step {} (WAIT for {})", i + 1, step_ref));
                }
            }
        }

        let summary = format!(
            "已完成工具链执行（共 {} 步）：\n{}",
            step_results.len(),
            step_results.join("\n")
        );
        Ok(Some(summary))
    }

    /// Execute a non-MCP tool by name.
    async fn execute_tool_by_name(
        &self,
        tool_name: &str,
        input: &str,
    ) -> Result<String, AgentError> {
        // Try skill registry first
        if let Some(registry) = &self.skill_registry {
            if let Some(skill) = registry.get(tool_name).await {
                let result = self.execute_registered_skill(&skill, input, None).await?;
                return Ok(result.output);
            }
        }

        Err(AgentError::Execution(
            crate::react_trace::format_unknown_tool_error(tool_name, &[]),
        ))
    }

    /// Evaluate a simple tool chain condition against step results
    fn evaluate_tool_chain_condition(&self, condition: &str, step_results: &[String]) -> bool {
        let condition_lower = condition.to_lowercase();
        // Simple heuristic: if condition mentions success/ok/passed and last result
        // looks positive
        if condition_lower.contains("success") || condition_lower.contains("ok") {
            if let Some(last) = step_results.last() {
                return !last.to_lowercase().contains("error")
                    && !last.to_lowercase().contains("fail");
            }
        }
        // Default: assume condition is met (conservative for safety)
        true
    }

    // 🆕 DEVICE FIX: Device automation methods

    /// Set device for automation
    pub fn with_device(mut self, device: Device) -> Self {
        self.device = Some(device);
        info!("Device configured for agent {}", self.config.id);
        self
    }

    /// Get device reference
    pub fn device(&self) -> Option<&Device> {
        self.device.as_ref()
    }

    /// Check if agent has device configured
    pub fn has_device(&self) -> bool {
        self.device.is_some()
    }

    /// 🟢 P1 FIX: Set memory system for long-term memory retrieval
    pub fn with_memory_system(mut self, memory: Arc<dyn crate::memory::MemorySearch>) -> Self {
        self.memory_system = Some(memory);
        info!("Memory system configured for agent {}", self.config.id);
        self
    }

    /// 🟢 P1 FIX: Get memory system reference
    pub fn memory_system(&self) -> Option<&Arc<dyn crate::memory::MemorySearch>> {
        self.memory_system.as_ref()
    }

    /// 🟢 P1 FIX: Check if agent has memory system configured
    pub fn has_memory_system(&self) -> bool {
        self.memory_system.is_some()
    }

    /// 🆕 FIX: Set skill catalog for global LLM context injection
    pub fn with_skill_catalog(mut self, catalog: impl Into<String>) -> Self {
        self.skill_catalog = Some(catalog.into());
        self
    }

    /// 🟢 P1 FIX: Attach a workflow registry for workflow execution tasks
    pub fn with_workflow_registry(
        mut self,
        registry: Arc<crate::workflow::WorkflowRegistry>,
    ) -> Self {
        self.workflow_registry = Some(registry);
        self
    }

    /// 🆕 OPTIMIZATION PHASE 1: Set intent engine
    pub fn with_intent_engine(mut self, engine: crate::intent::IntentEngine) -> Self {
        self.intent_engine = Some(engine);
        info!("Intent engine configured for agent {}", self.config.id);
        self
    }

    /// 🆕 OPTIMIZATION PHASE 1: Set approval gate
    pub fn with_approval_gate(mut self, gate: crate::security::ApprovalGate) -> Self {
        self.approval_gate = Some(gate);
        info!("Approval gate configured for agent {}", self.config.id);
        self
    }

    /// 🆕 OPTIMIZATION PHASE 2: Set prompt cache
    pub fn with_prompt_cache(mut self, cache: Arc<crate::prompt::PromptCache>) -> Self {
        self.prompt_cache = Some(cache);
        info!("Prompt cache configured for agent {}", self.config.id);
        self
    }

    /// 🆕 OPTIMIZATION PHASE 4: Set max rounds limit
    pub fn with_max_rounds(mut self, max_rounds: u32) -> Self {
        self.max_rounds = crate::react_trace::clamp_react_max_tool_rounds(max_rounds);
        info!(
            "Max rounds set to {} for agent {}",
            self.max_rounds, self.config.id
        );
        self
    }

    pub fn with_react_trace_sink(
        mut self,
        sink: Arc<dyn crate::react_trace::ReActTraceSink>,
    ) -> Self {
        self.react_trace_sink = Some(sink);
        self
    }

    /// Set foreign runtime manager for Python/Node.js script execution
    pub fn with_foreign_rt_manager(
        mut self,
        manager: Arc<beebotos_foreign_rt::DefaultForeignRuntimeManager>,
    ) -> Self {
        self.foreign_rt_manager = Some(manager);
        self
    }

    /// Set system information provider for querying Gateway-layer data.
    pub fn with_system_info_provider(
        mut self,
        provider: Arc<dyn crate::system_info::SystemInfoProvider>,
    ) -> Self {
        self.system_info_provider = Some(provider);
        self
    }

    /// 🆕 OPTIMIZATION PHASE 3: Set skill feedback collector
    pub fn with_skill_feedback_collector(
        mut self,
        collector: crate::skills::feedback::SkillImprovementEngine,
    ) -> Self {
        self.skill_feedback_collector = Some(collector);
        info!(
            "Skill feedback collector configured for agent {}",
            self.config.id
        );
        self
    }

    /// 🆕 OPTIMIZATION PHASE 2: Build cached base system prompt
    ///
    /// Caches the base persona (name + description) which changes rarely,
    /// avoiding repeated token assembly for identical agent configurations.
    async fn build_system_prompt_cached(&self, intent: &crate::intent::UserIntent) -> String {
        let base_persona = format!(
            "你是 {} ({})。回答问题时请保持友好、专业、有帮助。\
             所有内部思考过程和工具调用理由必须使用中文。",
            self.config.name, self.config.description
        );
        if let Some(ref cache) = self.prompt_cache {
            let components = crate::prompt::PromptComponents {
                soul: Some(base_persona.clone()),
                user_profile: None,
                project_memory: None,
                memories: Vec::new(),
                skills: Vec::new(),
                tools: Vec::new(),
                model_instructions: None,
                context_files: Vec::new(),
                model: self.config.models.model.clone(),
            };

            cache
                .get_or_build(&components, |comps| {
                    let builder = crate::prompt::PromptBuilder::new()
                        .with_soul(comps.soul.clone().unwrap_or_default())
                        .with_model(&comps.model);
                    builder.build(intent)
                })
                .await
        } else {
            base_persona
        }
    }

    /// 🆕 OPTIMIZATION: Classify intent for a task input
    pub async fn classify_intent(&self, input: &str) -> crate::intent::IntentAnalysis {
        if self.intent_engine.is_some() {
            // 🆕 OPTIMIZATION PHASE 3: Dual-track intent classification
            // Heuristic first, LLM fallback if confidence is low
            let threshold = self.config.intent_confidence_threshold.unwrap_or(0.7);
            let (heuristic, llm_prompt) =
                crate::intent::IntentEngine::classify_dual_track(input, threshold);
            if let Some(prompt) = llm_prompt {
                tracing::debug!(
                    "Intent classification below threshold ({}), LLM prompt prepared: {}",
                    threshold,
                    prompt
                );
                // TODO: If an LLM provider is available, send the prompt for
                // higher-confidence classification
            }
            heuristic
        } else {
            crate::intent::IntentAnalysis::new(crate::intent::UserIntent::DirectAnswer, 0.5)
        }
    }

    /// 🆕 FIX: Inject skill catalog into message list if configured.
    /// Avoids duplicate injection if the first message already looks like a
    /// catalog.
    fn inject_skill_catalog(
        &self,
        messages: Vec<communication::Message>,
    ) -> Vec<communication::Message> {
        if let Some(ref catalog) = self.skill_catalog {
            // Check if catalog is already present (first message contains the catalog
            // header)
            if messages
                .first()
                .map(|m| {
                    m.content
                        .contains("You have access to the following skills")
                })
                .unwrap_or(false)
            {
                return messages;
            }
            let mut result = vec![communication::Message::new(
                uuid::Uuid::new_v4(),
                communication::PlatformType::Custom,
                format!(
                    "[System Context] 你可以使用以下技能。\n\n{}\n\n指令：\n1. \
                     这些技能是上下文包。MCP 工具不是技能；需要外部 MCP 能力时使用 \
                     mcp_tool_search。\n2. 当非 MCP 技能匹配用户请求时，仅回复 \
                     SKILL:<id>|{{\"key\":\"value\"}} 格式，使用真实值。\n3. \
                     如果没有匹配的技能，直接回答。\n4. \
                     不要分析、解释或大声思考。不要列出参数。不要使用 <id> 或 \
                     {{\"param\":\"value\"}} 等占位符。\n5. \
                     如果缺少信息，用一句话简短提问。\n\n示例：\n用户：北京天气怎么样？\n输出：\
                     SKILL:weather_assistant|{{\"city\":\"北京\"}}",
                    catalog
                ),
            )];
            result.extend(messages);
            result
        } else {
            messages
        }
    }

    /// 🆕 OPTIMIZATION PHASE 4: Smart tool output truncation to prevent context
    /// overflow
    fn truncate_tool_output(output: &str, max_chars: usize) -> String {
        if output.len() <= max_chars {
            return output.to_string();
        }

        // Try intelligent truncation for JSON
        let trimmed = output.trim();
        if trimmed.starts_with('{') || trimmed.starts_with('[') {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(trimmed) {
                return Self::truncate_json_value(&json, max_chars);
            }
        }

        // Simple truncation for non-JSON
        format!(
            "{}...[truncated, {} chars total]",
            &output[..max_chars],
            output.len()
        )
    }

    /// Recursively truncate JSON values, preserving critical fields
    fn truncate_json_value(value: &serde_json::Value, max_chars: usize) -> String {
        match value {
            serde_json::Value::Object(map) => {
                let mut truncated = serde_json::Map::new();
                let mut current_len = 2; // {}
                let critical_fields: std::collections::HashSet<&str> =
                    ["error", "status", "symbol", "price", "code"]
                        .iter()
                        .cloned()
                        .collect();

                for (key, val) in map {
                    if critical_fields.contains(key.as_str()) {
                        let val_str = Self::truncate_json_value(val, max_chars / 4);
                        let val_json: serde_json::Value =
                            serde_json::from_str(&val_str).unwrap_or_else(|_| val.clone());
                        truncated.insert(key.clone(), val_json);
                        current_len += key.len() + val_str.len();
                    } else if current_len < max_chars * 3 / 4 {
                        let val_str = Self::truncate_json_value(val, max_chars / 4);
                        let val_json: serde_json::Value =
                            serde_json::from_str(&val_str).unwrap_or_else(|_| val.clone());
                        if current_len + key.len() + val_str.len() < max_chars {
                            truncated.insert(key.clone(), val_json);
                            current_len += key.len() + val_str.len();
                        }
                    }
                }
                // Fallback: if nothing was preserved, keep the first key with a truncated value
                if truncated.is_empty() && !map.is_empty() {
                    if let Some((first_key, first_val)) = map.iter().next() {
                        let val_str = Self::truncate_json_value(
                            first_val,
                            max_chars.saturating_sub(first_key.len() + 5),
                        );
                        if let Ok(val_json) = serde_json::from_str(&val_str) {
                            truncated.insert(first_key.clone(), val_json);
                        }
                    }
                }
                serde_json::Value::Object(truncated).to_string()
            }
            serde_json::Value::Array(arr) => {
                if arr.len() > 5 {
                    let mut truncated: Vec<serde_json::Value> =
                        arr.iter().take(3).cloned().collect();
                    truncated.push(serde_json::json!(format!(
                        "... and {} more items",
                        arr.len() - 3
                    )));
                    serde_json::Value::Array(truncated).to_string()
                } else {
                    let mut result = Vec::new();
                    let mut current_len = 2; // []
                    for item in arr {
                        let item_str =
                            Self::truncate_json_value(item, max_chars / arr.len().max(1));
                        if let Ok(item_json) = serde_json::from_str(&item_str) {
                            current_len += item_str.len() + 1;
                            result.push(item_json);
                        }
                        if current_len >= max_chars {
                            break;
                        }
                    }
                    serde_json::Value::Array(result).to_string()
                }
            }
            serde_json::Value::String(s) => {
                if s.len() > max_chars {
                    serde_json::Value::String(format!(
                        "{}...[truncated, {} chars total]",
                        &s[..max_chars.saturating_sub(30)],
                        s.len()
                    ))
                    .to_string()
                } else {
                    value.to_string()
                }
            }
            _ => value.to_string(),
        }
    }

    // ── MCP Parameter Extraction Helpers ──

    /// Determine whether an MCP skill is high-risk (requires preview +
    /// approval). Uses precise suffix/prefix matching to avoid false
    /// positives on skill names like "buying_guide" or "knowledge_buy".
    fn is_high_risk_mcp_skill(skill_id: &str) -> bool {
        // Temporarily allow the model to execute MCP tools directly without
        // the approval/pending-confirmation gate. Set this env var when the
        // guard needs to be re-enabled during local testing.
        if std::env::var_os("BEEBOTOS_ENABLE_MCP_APPROVAL").is_none() {
            return false;
        }

        let id_lower = skill_id.to_lowercase();
        // 🆕 FIX: Removed "_trade" — get_crypto_latest_trade / get_crypto_latest_quote
        // are read-only queries and should NOT require approval.
        let high_risk_keywords = [
            "_order",
            "place_order",
            "cancel_order",
            "trading_",
            "_trading",
            "_transfer",
            "_withdraw",
            "_delete",
        ];
        high_risk_keywords.iter().any(|kw| id_lower.contains(kw))
            || id_lower.ends_with("_buy")
            || id_lower.ends_with("_sell")
            || id_lower.starts_with("buy_")
            || id_lower.starts_with("sell_")
    }

    /// Generate a human-readable action preview for high-risk MCP operations.
    fn generate_action_preview(
        _skill_id: &str,
        tool_name: &str,
        params: &serde_json::Map<String, serde_json::Value>,
    ) -> String {
        let mut lines = vec![
            "🔴 高风险操作确认".to_string(),
            String::new(),
            format!("操作: {}", tool_name),
        ];

        for (key, value) in params {
            let display_value = match value {
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Number(n) => n.to_string(),
                serde_json::Value::Bool(b) => b.to_string(),
                _ => value.to_string(),
            };
            let label = match key.as_str() {
                "symbol" => "交易品种",
                "side" => "交易方向",
                "notional" | "qty" | "quantity" => "交易数量/金额",
                "price" => "价格",
                "type" => "订单类型",
                _ => key.as_str(),
            };
            lines.push(format!("{}: {}", label, display_value));
        }

        lines.push(String::new());
        lines.push("⚠️ 此操作涉及真实资金或重要数据，确认后无法撤销。".to_string());
        lines.push("请仔细核对以上信息，回复「确认」执行，或直接取消。".to_string());
        lines.join("\n")
    }

    /// Render a parameter collection form for interactive parameter input.
    fn render_parameter_form(
        request_id: &str,
        missing_fields: &[crate::skills::FieldSchema],
        partial: &serde_json::Map<String, serde_json::Value>,
    ) -> String {
        let mut lines = vec!["📋 请补充以下信息".to_string(), String::new()];

        for field in missing_fields {
            let hint = match field.name.as_str() {
                "symbol" => "例如: BTC/USD, ETH/USD",
                "side" => "买入 或 卖出",
                "notional" => "例如: 100 (USD)",
                "qty" => "例如: 0.01",
                _ => &field.description,
            };
            lines.push(format!("• {} ({}): {}", field.name, field.param_type, hint));
        }

        if !partial.is_empty() {
            lines.push(String::new());
            lines.push("已识别的信息:".to_string());
            for (k, v) in partial {
                lines.push(format!("  • {}: {}", k, v));
            }
        }

        lines.push(String::new());
        lines.push("请直接回复您要填写的内容，例如：\"BTC/USD, 买入, 100\"".to_string());
        lines.push(format!("(表单 ID: {})", request_id));
        lines.join("\n")
    }

    /// Clean up LLM responses that contain thinking/process analysis instead of
    /// direct answers. 🆕 OPTIMIZATION PHASE 2: Supports
    /// <REASONING_SCRATCHPAD> extraction
    fn cleanup_thinking_process(response: &str) -> String {
        let response = response.trim();
        if response.is_empty() {
            return response.to_string();
        }

        // 🆕 OPTIMIZATION: Extract and strip REASONING_SCRATCHPAD if present
        let (cleaned, _reasoning) = Self::extract_reasoning_scratchpad(response);

        // 🆕 FIX: If response contains SKILL: marker anywhere, extract it directly
        // But truncate after the JSON parameters to avoid trailing thinking text
        if let Some(pos) = cleaned.find("SKILL:") {
            let after_skill = &response[pos..];
            let potential_id = after_skill.strip_prefix("SKILL:").unwrap_or("").trim();
            let id_part = potential_id
                .split(|c: char| c == '|' || c == ' ' || c == '\n' || c == '\r')
                .next()
                .unwrap_or("");
            let invalid_ids = [
                "<skill_id>",
                "<id>",
                "工具id",
                "工具ID",
                "immediately",
                "format",
                "directly",
                "direct",
                "output",
                "skill",
                "id",
                "real",
                "actual",
            ];
            if !id_part.is_empty() && !invalid_ids.contains(&id_part) {
                if let Some(pipe_pos) = after_skill.find('|') {
                    let after_pipe = &after_skill[pipe_pos + 1..];
                    let mut depth = 0i32;
                    let mut json_end = 0;
                    let mut in_json = false;
                    for (i, c) in after_pipe.char_indices() {
                        if c == '{' {
                            depth += 1;
                            in_json = true;
                        } else if c == '}' {
                            depth -= 1;
                            if in_json && depth == 0 {
                                json_end = i + 1;
                                break;
                            }
                        }
                    }
                    if json_end > 0 {
                        return format!("{}", &after_skill[..pipe_pos + 1 + json_end]);
                    }
                }
                // Fallback: if no JSON found, just take the first line
                return after_skill
                    .lines()
                    .next()
                    .unwrap_or(after_skill)
                    .trim()
                    .to_string();
            }
            // Invalid skill ID (placeholder), fall through to thinking-prefix
            // cleanup
        }

        // Continue with cleaned response (REASONING_SCRATCHPAD already stripped)
        let response = cleaned.as_str();

        // If response starts with known thinking prefixes, try to extract actual answer
        let thinking_prefixes = [
            "用户问的是",
            "用户询问的是",
            "用户想知道",
            "用户想要",
            "用户要求",
            "用户请求",
            "用户的问题是",
            "查看可用的skills",
            "让我看看可用的",
            "看看可用的技能",
            "查看可用技能",
            "这是一个关于",
            "这是关于",
            "但是，",
            "但是 ",
            "不过，",
            "系统提示我",
            "系统指令",
            "根据系统指令",
            "系统指令明确说",
            "首先，我需要",
            "首先我需要",
            "我需要",
            "我来分析",
            "让我分析一下",
            "让我整理",
            "等等，让我",
            "等等，",
            "不过 ",
            "首先，",
            "第一步",
            "根据系统提示",
            "根据要求",
            "根据可用技能",
            "根据规则",
            "参数要求",
            "参数分析",
            "查看可用的 skills",
            "INSTRUCTION:",
            "RULES:",
            "规则：",
        ];
        let thinking_keywords = [
            "用户问的是",
            "用户询问的是",
            "用户想知道",
            "查看可用的skills",
            "让我看看可用的",
            "看看可用的技能",
            "查看可用技能",
            "可用的技能列表",
            "skill 列表",
            "技能列表",
            "不属于需要调用专门skill",
            "系统指令",
            "RULES",
        ];
        // Detect if response is mostly analysis: starts with thinking prefix OR
        // contains multiple thinking keywords
        let starts_with_thinking = thinking_prefixes.iter().any(|p| response.starts_with(p));
        let thinking_keyword_count = thinking_keywords
            .iter()
            .filter(|k| response.contains(**k))
            .count();
        let is_pure_analysis = starts_with_thinking
            || thinking_keyword_count >= 2
            || (response.contains("用户") && response.contains("skill") && response.len() > 200);
        if is_pure_analysis {
            // Check if response contains a list - if so, keep everything from first list
            // item
            let lines: Vec<&str> = response.lines().collect();
            let mut first_list_idx = None;
            for (idx, line) in lines.iter().enumerate() {
                let trimmed = line.trim();
                if trimmed.starts_with("-")
                    || trimmed.starts_with("•")
                    || (trimmed.len() > 2
                        && trimmed.as_bytes()[0].is_ascii_digit()
                        && trimmed.as_bytes()[1] == b'.')
                {
                    first_list_idx = Some(idx);
                    break;
                }
            }
            if let Some(start_idx) = first_list_idx {
                let result = lines[start_idx..].join("\n");
                if result.len() >= 20 {
                    return result;
                }
            }

            // Try to find any sentence that looks like an actual answer (not starting with
            // thinking prefixes)
            for line in response.lines() {
                let trimmed = line.trim();
                if trimmed.len() > 20
                    && !trimmed.starts_with("-")
                    && !trimmed.starts_with("•")
                    && !trimmed.starts_with("【")
                    && !trimmed.starts_with("[")
                {
                    // Verify this line doesn't contain heavy thinking keywords
                    let has_thinking = thinking_keywords.iter().any(|k| trimmed.contains(*k));
                    if !has_thinking {
                        return trimmed.to_string();
                    }
                }
            }
            // Fallback: return a generic message encouraging rephrasing
            return "抱歉，我暂时无法准确回答这个问题。您可以换个方式描述您的需求，\
                    我会尽力帮助您。"
                .to_string();
        }
        response.to_string()
    }

    /// 🆕 OPTIMIZATION PHASE 2: Extract <REASONING_SCRATCHPAD> content and
    /// return cleaned response
    fn extract_reasoning_scratchpad(response: &str) -> (String, Option<String>) {
        let start_tag = "<REASONING_SCRATCHPAD>";
        let end_tag = "</REASONING_SCRATCHPAD>";

        if let Some(start_pos) = response.find(start_tag) {
            if let Some(end_pos) = response.find(end_tag) {
                let reasoning_start = start_pos + start_tag.len();
                let reasoning = response[reasoning_start..end_pos].trim().to_string();

                // Remove the scratchpad from response
                let before = &response[..start_pos];
                let after = &response[end_pos + end_tag.len()..];
                let cleaned = format!("{}{}", before.trim(), after.trim());

                return (cleaned.trim().to_string(), Some(reasoning));
            }
        }

        (response.to_string(), None)
    }

    /// Connect to device (if configured)
    pub async fn connect_device(&self) -> crate::error::Result<()> {
        if let Some(ref device) = self.device {
            match device {
                Device::Node(d) => d.connect().await,
                Device::Ios(d) => d.connect().await,
                Device::Android(d) => d.connect().await,
            }
        } else {
            Err(AgentError::InvalidConfig(
                "No device configured".to_string(),
            ))
        }
    }

    /// Disconnect from device
    pub async fn disconnect_device(&self) -> crate::error::Result<()> {
        if let Some(ref device) = self.device {
            match device {
                Device::Node(d) => d.disconnect().await,
                Device::Ios(d) => d.disconnect().await,
                Device::Android(d) => d.disconnect().await,
            }
        } else {
            Ok(())
        }
    }

    pub async fn initialize(&mut self) -> Result<(), AgentError> {
        if let Some(mcp) = self.mcp_manager.as_mut() {
            mcp.initialize_all().await?;
        }

        // Channel lifecycle is managed globally by ChannelInstanceManager, not
        // per-agent.

        *self.state.lock().unwrap() = state_manager::AgentState::Idle;
        Ok(())
    }

    pub fn get_state(&self) -> state_manager::AgentState {
        self.state.lock().unwrap().clone()
    }

    pub fn get_config(&self) -> &AgentConfig {
        &self.config
    }

    /// 🛡️ CRITICAL FIX: Changed from &mut self to &self so the kernel task can
    /// call execute_task while only holding a *read* lock (or no lock at all).
    /// State transitions are handled internally via std::sync::Mutex.
    pub async fn execute_task(&self, task: Task) -> Result<TaskResult, AgentError> {
        *self.state.lock().unwrap() = state_manager::AgentState::Working {
            task_id: task.id.clone(),
        };

        let result = self.process_task(task).await;

        *self.state.lock().unwrap() = state_manager::AgentState::Idle;
        result
    }

    /// 🆕 OPTIMIZATION PHASE 4: Process task with max rounds limit to prevent
    /// infinite loops
    async fn process_task_with_round_limit(&self, task: Task) -> Result<TaskResult, AgentError> {
        let mut rounds = 0u32;
        let mut current_task = task;

        loop {
            rounds += 1;
            if rounds > self.max_rounds {
                return Ok(TaskResult {
                    task_id: current_task.id.clone(),
                    success: false,
                    output: format!(
                        "达到最大交互轮次限制 ({} 轮)，任务未完成。请简化需求或分步执行。",
                        self.max_rounds
                    ),
                    artifacts: vec![],
                    execution_time_ms: 0,
                });
            }

            let result = self.process_task(current_task.clone()).await;

            match &result {
                Ok(task_result) => {
                    // Simple heuristic: if output doesn't indicate need for more rounds, we're done
                    if !task_result.output.contains("SKILL:")
                        && !task_result.output.contains("需要更多信息")
                    {
                        return result;
                    }
                    // Otherwise continue with the output as new input (for multi-round reasoning)
                    current_task.input = task_result.output.clone();
                }
                Err(_) => return result,
            }
        }
    }

    /// 🟢 P1 FIX: 批量执行任务
    pub async fn execute_batch(&mut self, tasks: Vec<Task>) -> Vec<Result<TaskResult, AgentError>> {
        if tasks.is_empty() {
            return vec![];
        }

        info!("Starting batch execution of {} tasks", tasks.len());
        let start_time = std::time::Instant::now();

        let mut results = Vec::with_capacity(tasks.len());
        for task in tasks {
            results.push(self.execute_task(task).await);
        }

        let elapsed = start_time.elapsed();
        info!(
            "Batch execution completed: {} tasks in {:?}",
            results.len(),
            elapsed
        );

        results
    }

    /// Process a task with full implementation
    ///
    /// 🆕 PLANNING FIX: Enhanced with automatic complexity detection and
    /// planning integration 🆕 OPTIMIZATION: Intent-based routing for
    /// efficient task handling 🆕 SKILL MATCHING V2: Pure LLM-driven intent
    /// + skill selection (zero hardcoded rules)
    async fn process_task(&self, task: Task) -> Result<TaskResult, AgentError> {
        info!("Processing task {} of type {}", task.id, task.task_type);

        let start_time = std::time::Instant::now();
        let task_id = task.id.clone();

        let result = if matches!(task.task_type, TaskType::LlmChat | TaskType::Custom(_)) {
            self.process_task_react(&task).await
        } else {
            self.process_task_legacy(task).await
        };

        let execution_time_ms = start_time.elapsed().as_millis() as u64;

        match result {
            Ok((output, artifacts)) => {
                info!(
                    "Task {} completed successfully in {}ms",
                    task_id, execution_time_ms
                );
                Ok(TaskResult {
                    task_id,
                    success: true,
                    output,
                    artifacts,
                    execution_time_ms,
                })
            }
            Err(e) => {
                error!(
                    "Task {} failed after {}ms: {}",
                    task_id, execution_time_ms, e
                );
                Err(e)
            }
        }
    }

    fn extract_react_input(
        &self,
        task: &Task,
    ) -> (
        String,
        HashMap<String, String>,
        Vec<(String, String)>,
        Option<String>,
        Option<String>,
        String,
    ) {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&task.input) {
            let message = json
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or(&task.input)
                .to_string();
            let mut params = task.parameters.clone();
            for key in [
                "platform",
                "channel_id",
                "user_id",
                "session_id",
                "db_session_id",
            ] {
                if let Some(value) = json.get(key).and_then(|v| v.as_str()) {
                    params.insert(key.to_string(), value.to_string());
                }
            }
            let history: Vec<(String, String)> = json
                .get("history")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| {
                            let role = v.get("role").and_then(|r| r.as_str())?;
                            let content = v.get("content").and_then(|c| c.as_str())?;
                            Some((role.to_string(), content.to_string()))
                        })
                        .collect()
                })
                .unwrap_or_default();
            let memory_context = json
                .get("memory_context")
                .and_then(|m| m.as_str())
                .map(|s| s.to_string());
            let weather_data = json
                .get("weather_data")
                .and_then(|m| m.as_str())
                .map(|s| s.to_string());
            let session_key = params
                .get("session_id")
                .or_else(|| params.get("channel_id"))
                .cloned()
                .unwrap_or_else(|| task.id.clone());
            (
                message,
                params,
                history,
                memory_context,
                weather_data,
                session_key,
            )
        } else {
            (
                task.input.clone(),
                task.parameters.clone(),
                Vec::new(),
                None,
                None,
                task.parameters
                    .get("session_id")
                    .or_else(|| task.parameters.get("channel_id"))
                    .cloned()
                    .unwrap_or_else(|| task.id.clone()),
            )
        }
    }

    async fn build_react_messages(
        &self,
        input_text: &str,
        history: &[(String, String)],
        gateway_memory_context: &Option<String>,
        weather_data: &Option<String>,
        session_key: &str,
    ) -> Vec<communication::Message> {
        let skill_index = self.build_skill_index_context().await;
        let mcp_tool_index = self.build_mcp_tool_index_context().await;
        let active_skill_context = self.active_skill_context_for_session(session_key).await;
        let controlled_tool_prompt = if self.controlled_workspace_tools_enabled() {
            "Controlled workspace tools are currently enabled. You may use write_file, edit_file, \
             and exec when the user asks you to create files, modify files, run code, or execute \
             commands. Relative paths resolve from the workspace root; absolute paths are allowed \
             when the user asks you to configure or inspect files elsewhere."
        } else {
            "Controlled workspace tools such as write_file, edit_file, and exec are not exposed \
             right now. Do not claim you can write files or run commands unless those tools are \
             present."
        };
        let runtime_context = Self::runtime_context_prompt();
        let mut system = format!(
            concat!(
                "你是 {} ({})。\n\n",
                "使用 OpenClaw 风格的 ReAct ",
                "循环：不需要工具时直接回答；否则调用最合适的工具，观察结果，然后继续，",
                "直到能给用户一个自然的最终答案。不要声称你无法检查环境；需要检查时使用工具。\n\n",
                "{}\n\n",
                "## 工作区\n",
                "工作目录：{}\n",
                "相对路径从此目录解析。当用户要求检查或配置其他位置的文件时，允许使用绝对路径。\n",
                "\n",
                "这是 BeeBotOS 用户工作区，通常相对于仓库是 ./data/workspace。",
                "将其视为当前工作目录，而不是仓库根目录。\n\n",
                "## 工具\n",
                "可用工具包括 read_file、list_dir、glob、grep、write_file、edit_file、exec、",
                "web_fetch、web_search、",
                "activate_skill，以及运行时策略启用的 mcp_tool_search。{}\n\n",
                "## 技能\n",
                "{}\n\n",
                "重要：所有内部思考过程（thought）和工具调用理由（reasoning）必须使用中文输出。",
            ),
            self.config.name,
            self.config.description,
            runtime_context,
            self.workspace_dir_display(),
            controlled_tool_prompt,
            skill_index
        );
        if let Some(mcp_tools) = mcp_tool_index {
            system.push_str("\n\n## MCP Tools\n");
            system.push_str(&mcp_tools);
        }
        if let Some(active) = active_skill_context {
            system.push_str("\n\n## Active Skill Contexts\n");
            system.push_str(&active);
        }

        let mut messages = vec![communication::Message::with_metadata(
            uuid::Uuid::new_v4(),
            communication::PlatformType::Custom,
            system,
            {
                let mut metadata = HashMap::new();
                metadata.insert("role".to_string(), "system".to_string());
                metadata
            },
        )];

        if let Some(memory) = gateway_memory_context {
            if !memory.trim().is_empty() {
                messages.push(communication::Message::with_metadata(
                    uuid::Uuid::new_v4(),
                    communication::PlatformType::Custom,
                    format!("Relevant memory:\n{}", memory),
                    {
                        let mut metadata = HashMap::new();
                        metadata.insert("role".to_string(), "system".to_string());
                        metadata
                    },
                ));
            }
        } else if let Some(ref memory) = self.memory_system {
            if let Ok(results) = memory.search(input_text).await {
                let context = results
                    .iter()
                    .take(4)
                    .map(|r| format!("- {}", r.content))
                    .collect::<Vec<_>>()
                    .join("\n");
                if !context.is_empty() {
                    messages.push(communication::Message::with_metadata(
                        uuid::Uuid::new_v4(),
                        communication::PlatformType::Custom,
                        format!("Relevant memory:\n{}", context),
                        {
                            let mut metadata = HashMap::new();
                            metadata.insert("role".to_string(), "system".to_string());
                            metadata
                        },
                    ));
                }
            }
        }

        for (role, content) in history {
            let normalized_role = match role.as_str() {
                "assistant" => "assistant",
                "system" => "system",
                _ => "user",
            };
            let mut metadata = HashMap::new();
            metadata.insert("role".to_string(), normalized_role.to_string());
            messages.push(communication::Message::with_metadata(
                uuid::Uuid::new_v4(),
                communication::PlatformType::Custom,
                content.clone(),
                metadata,
            ));
        }

        let user_message = if let Some(weather) = weather_data {
            if weather.trim().is_empty() {
                input_text.to_string()
            } else {
                format!(
                    "{}\n\nReference weather data:\n{}\nUse this data if relevant.",
                    input_text, weather
                )
            }
        } else {
            input_text.to_string()
        };
        let mut metadata = HashMap::new();
        metadata.insert("role".to_string(), "user".to_string());
        messages.push(communication::Message::with_metadata(
            uuid::Uuid::new_v4(),
            communication::PlatformType::Custom,
            user_message,
            metadata,
        ));
        messages
    }

    fn is_cancelled(cancel_rx: &Option<tokio::sync::watch::Receiver<bool>>) -> bool {
        cancel_rx.as_ref().map(|rx| *rx.borrow()).unwrap_or(false)
    }

    async fn cancellation_receiver(
        session_key: &str,
        params: &HashMap<String, String>,
    ) -> Option<tokio::sync::watch::Receiver<bool>> {
        if let Some(generation) = params
            .get("cancellation_generation")
            .and_then(|value| value.parse::<u64>().ok())
        {
            crate::session_cancellation::get_receiver_for_generation(session_key, generation).await
        } else {
            crate::session_cancellation::get_receiver(session_key).await
        }
    }

    async fn process_task_react(&self, task: &Task) -> Result<(String, Vec<Artifact>), AgentError> {
        let llm = self
            .llm_interface
            .as_ref()
            .ok_or_else(|| AgentError::InvalidConfig("LLM interface not configured".into()))?;
        if !llm.supports_native_tools() {
            return self.handle_llm_task_internal(task, None).await;
        }

        let (input_text, mut extra_params, history, memory_context, weather_data, session_key) =
            self.extract_react_input(task);
        let cancel_rx = Self::cancellation_receiver(&session_key, &extra_params).await;
        let mut tools = self.builtin_react_tools();
        self.add_mcp_tool_search_if_available(&mut tools);
        let mut loop_messages = self
            .build_react_messages(
                &input_text,
                &history,
                &memory_context,
                &weather_data,
                &session_key,
            )
            .await;
        extra_params.insert("tool_choice".to_string(), "auto".to_string());
        extra_params.insert("max_tokens".to_string(), "8192".to_string());
        if let Ok(json) = serde_json::to_string(&tools) {
            extra_params.insert("tools_json".to_string(), json);
        }
        extra_params.insert("max_tool_rounds".to_string(), self.max_rounds.to_string());

        let max_tool_rounds = crate::react_trace::clamp_react_max_tool_rounds(self.max_rounds);
        let run_id = Self::react_run_id(&task.id);
        extra_params.insert("agent_id".to_string(), self.config.id.clone());
        extra_params.insert("react_run_id".to_string(), run_id.clone());
        self.emit_react_trace(
            crate::react_trace::ReActTraceEvent::new(
                run_id.clone(),
                session_key.clone(),
                self.config.id.clone(),
                0,
                max_tool_rounds,
                crate::react_trace::ReActTracePhase::LoopStarted,
            )
            .with_content_preview(&input_text),
        )
        .await;
        // 🛡️ Circuit-breaker: abort if the same tool keeps failing.
        const MAX_CONSECUTIVE_TOOL_ERRORS: u32 = 10;
        const LLM_TURN_TIMEOUT_SECS: u64 = 180;
        let mut consecutive_tool_errors: u32 = 0;

        for round in 0..max_tool_rounds {
            let round_number = round + 1;
            if Self::is_cancelled(&cancel_rx) {
                info!(
                    "process_task_react: session {} cancelled before round {}",
                    session_key, round_number
                );
                return Ok(("⏹️ 已停止当前任务。".to_string(), vec![]));
            }
            extra_params.insert("react_round".to_string(), round_number.to_string());

            // Guard each LLM turn with a hard timeout so a stalled provider cannot
            // hang the whole ReAct loop for minutes.
            let turn = tokio::time::timeout(
                std::time::Duration::from_secs(LLM_TURN_TIMEOUT_SECS),
                llm.call_llm_tool_turn(
                    loop_messages.clone(),
                    tools.clone(),
                    Some(extra_params.clone()),
                ),
            )
            .await
            .map_err(|_| {
                AgentError::Execution(format!(
                    "LLM ReAct turn timed out after {}s (round {})",
                    LLM_TURN_TIMEOUT_SECS, round_number
                ))
            })?
            .map_err(|e| AgentError::Execution(format!("LLM ReAct turn failed: {}", e)))?;

            if turn.tool_calls.is_empty() {
                let response = Self::cleanup_thinking_process(&turn.content);
                self.emit_react_trace(
                    crate::react_trace::ReActTraceEvent::new(
                        run_id.clone(),
                        session_key.clone(),
                        self.config.id.clone(),
                        round_number,
                        max_tool_rounds,
                        crate::react_trace::ReActTracePhase::FinalAnswer,
                    )
                    .with_content_preview(&response),
                )
                .await;
                if response.trim().is_empty() {
                    return Ok((
                        "抱歉，AI 暂时无法生成回复，请稍后再试。".to_string(),
                        vec![],
                    ));
                }
                if let Some(skill_observation) = self
                    .try_execute_skill_trigger_text(&response, &input_text, &weather_data)
                    .await?
                {
                    if skill_observation.success {
                        return Ok((skill_observation.output, vec![]));
                    }

                    info!(
                        "Text skill trigger returned recoverable observation; continuing ReAct \
                         loop"
                    );
                    let synthetic_tool_id = format!("skill-text-{}", uuid::Uuid::new_v4());
                    loop_messages.push(Self::assistant_tool_call_message(
                        &[crate::llm::ToolCall {
                            id: synthetic_tool_id.clone(),
                            r#type: "function".to_string(),
                            function: crate::llm::FunctionCall {
                                name: skill_observation.skill_id,
                                arguments: skill_observation.arguments,
                            },
                        }],
                        None,
                    ));
                    loop_messages.push(Self::tool_result_message(
                        &synthetic_tool_id,
                        skill_observation.output,
                    ));
                    continue;
                }
                return Ok((response, vec![]));
            }

            info!(
                "process_task_react: round {} returned {} tool call(s)",
                round_number,
                turn.tool_calls.len()
            );
            self.emit_react_trace(
                crate::react_trace::ReActTraceEvent::new(
                    run_id.clone(),
                    session_key.clone(),
                    self.config.id.clone(),
                    round_number,
                    max_tool_rounds,
                    crate::react_trace::ReActTracePhase::AssistantToolCall,
                )
                .with_content_preview(format!("{} tool call(s)", turn.tool_calls.len())),
            )
            .await;
            loop_messages.push(Self::assistant_tool_call_message(
                &turn.tool_calls,
                turn.reasoning_content.as_deref(),
            ));
            for tool_call in &turn.tool_calls {
                if Self::is_cancelled(&cancel_rx) {
                    info!(
                        "process_task_react: session {} cancelled before tool {}",
                        session_key, tool_call.function.name
                    );
                    return Ok(("⏹️ 已停止当前任务。".to_string(), vec![]));
                }
                self.emit_react_trace(
                    crate::react_trace::ReActTraceEvent::new(
                        run_id.clone(),
                        session_key.clone(),
                        self.config.id.clone(),
                        round_number,
                        max_tool_rounds,
                        crate::react_trace::ReActTracePhase::ToolStarted,
                    )
                    .with_tool(tool_call.function.name.clone(), tool_call.id.clone())
                    .with_arguments_preview(Self::tool_arguments_preview(tool_call))
                    .with_reasoning(turn.reasoning_content.as_deref().unwrap_or("")),
                )
                .await;
                let output = match self
                    .execute_react_tool_call(tool_call, &session_key, &input_text, &extra_params)
                    .await
                {
                    Ok(output) => {
                        consecutive_tool_errors = 0;
                        self.emit_react_trace(
                            crate::react_trace::ReActTraceEvent::new(
                                run_id.clone(),
                                session_key.clone(),
                                self.config.id.clone(),
                                round_number,
                                max_tool_rounds,
                                crate::react_trace::ReActTracePhase::ToolResult,
                            )
                            .with_tool(tool_call.function.name.clone(), tool_call.id.clone())
                            .with_content_preview(&output),
                        )
                        .await;
                        output
                    }
                    Err(e) => {
                        consecutive_tool_errors += 1;
                        warn!(
                            "ReAct tool '{}' failed ({}/{}): {}",
                            tool_call.function.name,
                            consecutive_tool_errors,
                            MAX_CONSECUTIVE_TOOL_ERRORS,
                            e
                        );
                        if consecutive_tool_errors >= MAX_CONSECUTIVE_TOOL_ERRORS {
                            let msg = format!(
                                "工具连续失败 {} 次（最近失败: \
                                 {}），任务已终止。建议：请简化需求、检查文件路径或参数格式。",
                                MAX_CONSECUTIVE_TOOL_ERRORS, tool_call.function.name
                            );
                            return Ok((msg, vec![]));
                        }
                        let output = Self::recoverable_tool_error(&tool_call.function.name, e);
                        self.emit_react_trace(
                            crate::react_trace::ReActTraceEvent::new(
                                run_id.clone(),
                                session_key.clone(),
                                self.config.id.clone(),
                                round_number,
                                max_tool_rounds,
                                crate::react_trace::ReActTracePhase::ToolError,
                            )
                            .with_tool(tool_call.function.name.clone(), tool_call.id.clone())
                            .with_content_preview(&output)
                            .error(),
                        )
                        .await;
                        output
                    }
                };
                loop_messages.push(Self::tool_result_message(&tool_call.id, output));

                if tool_call.function.name == "activate_skill" {
                    if let Some(active) = self.active_skill_context_for_session(&session_key).await
                    {
                        let mut metadata = HashMap::new();
                        metadata.insert("role".to_string(), "system".to_string());
                        loop_messages.push(communication::Message::with_metadata(
                            uuid::Uuid::new_v4(),
                            communication::PlatformType::Custom,
                            format!("Updated active skill contexts:\n{}", active),
                            metadata,
                        ));
                    }
                }

                if Self::is_mcp_search_tool(&tool_call.function.name) {
                    if let Some(targets) = self
                        .mcp_targets_from_search_arguments(&tool_call.function.arguments)
                        .await
                    {
                        for target in targets {
                            match self.mcp_tool_definition(&target).await {
                                Ok(definition) => {
                                    if !tools.iter().any(|tool| tool.name == definition.name) {
                                        tools.push(definition);
                                    }
                                }
                                Err(e) => {
                                    warn!(
                                        "Failed to dynamically load MCP tool schema for {}/{}: {}",
                                        target.server_name, target.tool_name, e
                                    );
                                }
                            }
                        }
                        if let Ok(json) = serde_json::to_string(&tools) {
                            extra_params.insert("tools_json".to_string(), json);
                        }
                    }
                }
            }
        }

        self.emit_react_trace(
            crate::react_trace::ReActTraceEvent::new(
                run_id.clone(),
                session_key.clone(),
                self.config.id.clone(),
                max_tool_rounds,
                max_tool_rounds,
                crate::react_trace::ReActTracePhase::LoopLimitReached,
            )
            .with_content_preview("Maximum ReAct/tool rounds reached; requesting final answer."),
        )
        .await;
        let response = self
            .force_final_react_answer(llm, loop_messages, tools, extra_params, max_tool_rounds)
            .await
            .unwrap_or_else(|e| {
                warn!("Forced final ReAct answer failed: {}", e);
                format!(
                    "工具循环达到最大轮次限制 ({} 轮)，且模型未能生成最终回复。",
                    max_tool_rounds
                )
            });
        Ok((response, vec![]))
    }

    async fn try_execute_skill_trigger_text(
        &self,
        response: &str,
        input_text: &str,
        weather_data: &Option<String>,
    ) -> Result<Option<TextSkillObservation>, AgentError> {
        let trimmed = response.trim();
        let Some(skill_start) = trimmed.find("SKILL:") else {
            return Ok(None);
        };
        let skill_part = trimmed[skill_start + "SKILL:".len()..].trim();
        if skill_part.is_empty() {
            return Ok(None);
        }

        let (skill_id, arguments) = if let Some(pos) = skill_part.find('|') {
            let skill_id = skill_part[..pos].trim();
            let arguments = skill_part[pos + 1..]
                .lines()
                .next()
                .unwrap_or("")
                .trim()
                .to_string();
            (skill_id, arguments)
        } else {
            let skill_id = skill_part.split_whitespace().next().unwrap_or("").trim();
            (skill_id, "{}".to_string())
        };

        if skill_id.is_empty() {
            return Ok(None);
        }

        info!(
            "ReAct returned text skill trigger '{}'; executing instead of returning raw protocol",
            skill_id
        );

        let tool_call = crate::llm::ToolCall {
            id: format!("skill-text-{}", uuid::Uuid::new_v4()),
            r#type: "function".to_string(),
            function: crate::llm::FunctionCall {
                name: skill_id.to_string(),
                arguments: arguments.clone(),
            },
        };

        let (output, success) = self
            .execute_native_tool_call(&tool_call, &HashMap::new(), input_text, weather_data)
            .await?;
        Ok(Some(TextSkillObservation {
            skill_id: skill_id.to_string(),
            arguments,
            output,
            success,
        }))
    }

    /// 🆕 SKILL MATCHING V2: Pure LLM-driven task processing
    ///
    /// Graceful degradation: if V2 analysis times out or fails, falls back to
    /// legacy path.
    async fn process_task_v2(&self, task: Task) -> Result<(String, Vec<Artifact>), AgentError> {
        let task_id = task.id.clone();
        let message_text = if let Ok(json) = serde_json::from_str::<serde_json::Value>(&task.input)
        {
            json.get("message")
                .and_then(|m| m.as_str())
                .unwrap_or(&task.input)
                .to_string()
        } else {
            task.input.clone()
        };

        // 🆕 MCP PARAMETER EXTRACTION: Check for form submission responses FIRST.
        // If the user is replying to a parameter form, handle it before any other
        // logic.
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&task.input) {
            if let Some(form_submission) = json.get("form_submission") {
                if let Some(req_id) = form_submission.get("request_id").and_then(|v| v.as_str()) {
                    return self.handle_form_submission(req_id, &form_submission).await;
                }
            }
        }
        // Also try to detect plain-text form responses (user replies directly with
        // values) Heuristic: only treat as form submission if message looks
        // like parameter values (contains commas/numbers/short length) and
        // there is an active pending form.
        {
            let has_pending_form = {
                let forms = self.pending_parameter_forms.read().await;
                forms.values().any(|form| !form.is_expired())
            };
            if has_pending_form {
                // Exclude pure confirmation words from form detection so they fall through
                // to the approval confirmation handler below.
                let is_pure_confirmation = message_text.trim().len() <= 6
                    && ["确认", "同意", "yes", "ok", "好", "可以", "执行", "y"]
                        .iter()
                        .any(|w| message_text.to_lowercase().trim() == *w);
                let looks_like_form_response = !is_pure_confirmation
                    && (message_text.contains(',')
                        || message_text.contains('，')
                        || message_text.chars().any(|c| c.is_ascii_digit())
                        || message_text.trim().len() <= 30);
                if looks_like_form_response {
                    let req_id = {
                        let forms = self.pending_parameter_forms.read().await;
                        forms
                            .iter()
                            .find(|(_, form)| !form.is_expired())
                            .map(|(req_id, _)| req_id.clone())
                    };
                    if let Some(req_id) = req_id {
                        match self
                            .handle_text_form_submission(&req_id, &message_text)
                            .await
                        {
                            Ok(result) => return Ok(result),
                            Err(_) => {
                                // Form expired or parsing failed; continue to
                                // normal processing
                            }
                        }
                    }
                }
            }
        }

        // 🆕 FIX (Plan C): Check for pending approval confirmations BEFORE intent
        // analysis. If user says "confirm"/"同意"/"yes"/"ok", execute the
        // pending operation. This is checked AFTER form submission so that
        // parameter values containing "确认" (e.g., "买入，确认") are handled
        // as form input first.
        if is_pending_approval_adjustment(&message_text) {
            let mut approvals = self.pending_approvals.write().await;
            if let Some((req_id, request)) = approvals
                .iter_mut()
                .max_by_key(|(_, request)| request.created_at)
            {
                request.original_input = format!(
                    "{}\n用户补充/调整：{}",
                    request.original_input,
                    message_text.trim()
                );
                info!(
                    "Plan C: User adjusted pending approval {} for skill '{}'",
                    req_id, request.skill_id
                );
                let response = format!(
                    "已更新待确认操作：{}\n\n新的补充信息：{}\n\n请回复「确认」或「同意」执行；\
                     如需继续修改，请直接说明。",
                    request.skill_id,
                    message_text.trim()
                );
                return Ok((response, vec![]));
            }
        }

        let is_confirmation = is_confirmation_message(&message_text);

        if is_confirmation {
            let mut approvals = self.pending_approvals.write().await;
            if !approvals.is_empty() {
                if let Some((req_id, request)) = approvals
                    .iter()
                    .max_by(|(_, a), (_, b)| a.created_at.cmp(&b.created_at))
                    .map(|(k, v)| (k.clone(), v.clone()))
                {
                    approvals.remove(&req_id);
                    drop(approvals);
                    info!(
                        "Plan C: User confirmed pending approval {} for skill '{}'",
                        req_id, request.skill_id
                    );

                    if request.skill_id.starts_with("mcp:") {
                        let target = match Self::parse_mcp_display_tool_name(&request.skill_id) {
                            Ok(target) => target,
                            Err(e) => {
                                return Ok((
                                    format!("已确认操作，但 MCP 工具名无效: {}", e),
                                    vec![],
                                ));
                            }
                        };
                        let dynamic_tool_name =
                            Self::mcp_dynamic_tool_name(&target.server_name, &target.tool_name);
                        let confirmed_params = request.params.as_object().map(|obj| {
                            obj.iter()
                                .filter_map(|(k, v)| {
                                    if let Some(s) = v.as_str() {
                                        Some((k.clone(), s.to_string()))
                                    } else if v.is_null() {
                                        None
                                    } else {
                                        Some((k.clone(), v.to_string()))
                                    }
                                })
                                .collect::<HashMap<_, _>>()
                        });

                        self.skip_approval
                            .store(true, std::sync::atomic::Ordering::SeqCst);
                        let execution = self
                            .execute_mcp_dynamic_tool(
                                &dynamic_tool_name,
                                confirmed_params.as_ref(),
                                &request.original_input,
                            )
                            .await;
                        self.skip_approval
                            .store(false, std::sync::atomic::Ordering::SeqCst);

                        return match execution {
                            Ok((output, true)) => Ok((output, vec![])),
                            Ok((output, false)) => {
                                Ok((format!("已确认操作，但执行失败: {}", output), vec![]))
                            }
                            Err(e) => Ok((format!("已确认操作，但执行失败: {}", e), vec![])),
                        };
                    }

                    // Re-execute the skill with approval bypassed
                    if let Some(ref registry) = self.skill_registry {
                        if let Some(skill) = registry.get(&request.skill_id).await {
                            // 🆕 FIX: Temporarily bypass approval gate for confirmed operation
                            // 🆕 FIX: Use original_input instead of params.to_string() so
                            // MCP parameter extractor can re-extract params from natural language.
                            self.skip_approval
                                .store(true, std::sync::atomic::Ordering::SeqCst);
                            let confirmed_params = request.params.as_object().map(|obj| {
                                obj.iter()
                                    .filter_map(|(k, v)| {
                                        if let Some(s) = v.as_str() {
                                            Some((k.clone(), s.to_string()))
                                        } else if v.is_null() {
                                            None
                                        } else {
                                            Some((k.clone(), v.to_string()))
                                        }
                                    })
                                    .collect::<HashMap<_, _>>()
                            });
                            let confirmed_input = if !request.params.is_null()
                                && request
                                    .params
                                    .as_object()
                                    .map_or(false, |obj| !obj.is_empty())
                            {
                                request.params.to_string()
                            } else if request.original_input.trim().is_empty() {
                                request.params.to_string()
                            } else {
                                request.original_input.clone()
                            };
                            let skill_result = self
                                .execute_registered_skill(
                                    &skill,
                                    &confirmed_input,
                                    confirmed_params,
                                )
                                .await;
                            self.skip_approval
                                .store(false, std::sync::atomic::Ordering::SeqCst);

                            match skill_result {
                                Ok(result) => {
                                    let _ = registry.record_usage(&request.skill_id).await;
                                    let output = self.synthesize_skill_output(
                                        &message_text,
                                        &result.output,
                                        &request.skill_id,
                                    );
                                    return Ok((output, vec![]));
                                }
                                Err(e) => {
                                    return Ok((format!("已确认操作，但执行失败: {}", e), vec![]));
                                }
                            }
                        }
                    }
                    return Ok(("已确认，但找不到对应的技能。".to_string(), vec![]));
                }
            }
            return Ok((
                "当前没有待确认的真实工具操作。上一条确认提示没有创建后端审批单，\
                 我不会继续用聊天文本模拟下单。请重新发送下单请求，我会先调用对应的 MCP \
                 工具生成真实确认单，再由您确认执行。"
                    .to_string(),
                vec![],
            ));
        }

        // Step 1: LLM Intent Analysis (zero hardcoded rules)
        let intent_analyzer = self.llm_intent_analyzer.as_ref().ok_or_else(|| {
            AgentError::InvalidConfig("LLM Intent Analyzer not configured".into())
        })?;

        let intent_v2 = match intent_analyzer.analyze(&message_text, None).await {
            Ok(intent) => intent,
            Err(e) => {
                warn!(
                    "V2 Intent analysis failed for task {} ({}), falling back to legacy path",
                    task_id, e
                );
                return self.process_task_legacy(task).await;
            }
        };

        info!(
            "V2 Intent: direct_answer={}, needs_skill={}, needs_planning={}, confidence={:.2} for \
             task {}",
            intent_v2.direct_answer,
            intent_v2.needs_skill,
            intent_v2.needs_planning,
            intent_v2.confidence,
            task_id
        );

        // 🆕 FIX (Plan B): Skill introspection shortcut — when user asks about
        // available skills, directly query the registry instead of relying on
        // LLM knowledge (which is stale). This handles queries like
        // "有哪些skills", "本机技能", "你有什么能力" etc.
        let query_lower = message_text.to_lowercase();
        let is_skill_query = query_lower.contains("skill")
            || query_lower.contains("技能")
            || query_lower.contains("有哪些能力")
            || query_lower.contains("你能做什么")
            || query_lower.contains("会什么")
            || (query_lower.contains("有哪些")
                && (query_lower.contains("功能") || query_lower.contains("工具")))
            || query_lower.contains("list skills")
            || query_lower.contains("available skills")
            || query_lower.contains("what can you do");

        if is_skill_query {
            if let Some(ref registry) = self.skill_registry {
                let skills = registry.list_enabled().await;
                let mut lines = vec![
                    format!("🛠️ 本机共有 {} 个可用技能 (enabled skills):", skills.len()),
                    String::new(),
                ];
                for skill in &skills {
                    let manifest = &skill.skill.manifest;
                    lines.push(format!("• **{}** (`{}`)", skill.skill.name, skill.skill.id));
                    if !manifest.description.is_empty() {
                        lines.push(format!("  - {}", manifest.description));
                    }
                    if !manifest.capabilities.is_empty() {
                        lines.push(format!("  - 能力: {}", manifest.capabilities.join(", ")));
                    }
                    lines.push(String::new());
                }
                let response = lines.join("\n");
                info!(
                    "Plan B: Skill introspection shortcut answered query with {} skills",
                    skills.len()
                );
                return Ok((response, vec![]));
            }
        }

        // Initialize trace for observability
        let mut trace =
            crate::skill_matching::SkillActivationTrace::new(&message_text, intent_v2.clone());

        // Step 2: Route based on LLM intent (no hardcoded keyword matching)
        let result = if intent_v2.direct_answer || !intent_v2.needs_skill {
            // Direct answer path — no skill injection
            self.handle_direct_answer(&task).await
        } else {
            // Step 3: Skill Selection (pure LLM-driven)
            let selector = self
                .skill_selector
                .as_ref()
                .ok_or_else(|| AgentError::InvalidConfig("Skill Selector not configured".into()))?;

            let selection = match selector
                .select(&message_text, &intent_v2.query_summary)
                .await
            {
                Ok(sel) => sel,
                Err(e) => {
                    warn!(
                        "V2 Skill selection failed for task {} ({}), continuing without skill \
                         injection",
                        task_id, e
                    );
                    // Fallback: proceed without skill hint, let LLM handle directly
                    return self
                        .handle_llm_task_v2(
                            &task,
                            &intent_v2,
                            &crate::skill_matching::SkillSelection {
                                selected_skill: None,
                                selected_skill_name: None,
                                needs_planning: intent_v2.needs_planning,
                                confidence: 0.0,
                                scores: Vec::new(),
                                selection_reasoning: format!("Skill selection failed: {}", e),
                                disclosure_level: crate::skills::registry::SkillDisclosureLevel::L0,
                            },
                        )
                        .await;
                }
            };

            info!(
                "V2 Skill Selection: selected={:?}, confidence={:.2}, reasoning='{}'",
                selection.selected_skill,
                selection.confidence,
                selection
                    .selection_reasoning
                    .chars()
                    .take(80)
                    .collect::<String>()
            );

            // Update trace with retrieval and ranking results
            let candidate_ids: Vec<String> = selection
                .scores
                .iter()
                .map(|s| s.skill_id.clone())
                .collect();
            let recall_scores: Vec<(String, f32)> = selection
                .scores
                .iter()
                .map(|s| (s.skill_id.clone(), s.overall_score))
                .collect();
            trace = trace.with_retrieval(crate::skill_matching::RetrievalTrace {
                method: "registry_search".to_string(),
                candidate_skills: candidate_ids,
                recall_scores,
            });
            trace = trace.with_ranking(crate::skill_matching::RankingTrace {
                llm_model: String::new(),
                scores: selection.scores.clone(),
                selected_skill: selection.selected_skill.clone(),
                reasoning: selection.selection_reasoning.clone(),
                confidence: selection.confidence,
            });

            // Step 4: Build task with skill hint if selected
            let mut task = task;
            if let Some(ref skill_id) = selection.selected_skill {
                let registry = self.skill_registry.as_ref().ok_or_else(|| {
                    AgentError::InvalidConfig("Skill registry not configured".into())
                })?;

                match registry.get(skill_id).await {
                    Some(skill) => {
                        let l3_doc = registry
                            .get_skill_description(
                                skill_id,
                                crate::skills::registry::SkillDisclosureLevel::L3,
                            )
                            .await;

                        // Inject skill hint into task input
                        if let Ok(mut input_json) =
                            serde_json::from_str::<serde_json::Value>(&task.input)
                        {
                            if let Some(obj) = input_json.as_object_mut() {
                                obj.insert("skill_hint_v2".to_string(), serde_json::json!({
                                    "id": skill_id,
                                    "name": skill.skill.name,
                                    "description": skill.skill.manifest.description,
                                    "prompt_template": l3_doc.unwrap_or_else(|| skill.skill.manifest.prompt_template.clone()),
                                    "confidence": selection.confidence,
                                    "needs_planning": intent_v2.needs_planning || selection.needs_planning,
                                }));
                                task.input = input_json.to_string();
                            }
                        }
                    }
                    None => {
                        warn!(
                            "V2 Skill Selection: skill '{}' selected but not found in registry",
                            skill_id
                        );
                    }
                }
            }

            // Step 5: Route to handler based on planning need
            let mut should_use_planning = intent_v2.needs_planning || selection.needs_planning;

            // 🆕 FIX: If selected skill is a knowledge skill (no executable scripts),
            // skip planning and execute it directly. Planning is only useful for
            // multi-step code execution or skill composition.
            if should_use_planning
                && selection.selected_skill.is_some()
                && selection.confidence >= 0.5
            {
                if let Some(ref registry) = self.skill_registry {
                    if let Some(skill) = registry
                        .get(selection.selected_skill.as_ref().unwrap())
                        .await
                    {
                        let source = &skill.skill.source_path;
                        let is_knowledge = !source.as_os_str().is_empty() && !source.is_dir();
                        if is_knowledge {
                            info!(
                                "Selected skill '{}' is a knowledge skill (no scripts). Skipping \
                                 planning and executing directly.",
                                skill.skill.id
                            );
                            should_use_planning = false;
                        }
                    }
                }
            }

            // 🆕 UNIFIED REACT: All planning tasks delegate to LLM self-decision
            // via the general native tool loop (handle_llm_task_internal).
            // Legacy execute_with_react_planning (alpaca hard-coded) removed.
            if should_use_planning {
                self.execute_with_planning(task).await
            } else {
                self.handle_llm_task_v2(&task, &intent_v2, &selection).await
            }
        };

        // Store trace for observability (fire-and-forget, don't fail the task)
        if let Some(ref store) = self.trace_store {
            if let Err(e) = store.store(&trace).await {
                warn!("Failed to store skill activation trace: {}", e);
            }
        }

        result
    }

    // =====================================================================
    // 🆕 UNIFIED REACT: Autonomous Planning & Investment Analysis
    // =====================================================================

    /// Determine whether a task should use the Unified ReAct executor
    /// for autonomous multi-step planning instead of the static planning
    /// engine.
    fn should_use_react_planning(
        &self,
        message_text: &str,
        intent: &crate::skill_matching::IntentAnalysisV2,
        selection: &crate::skill_matching::SkillSelection,
    ) -> bool {
        let lower = message_text.to_lowercase();

        // Trigger 1: User explicitly asks for analysis / advice
        let analysis_keywords = [
            "分析",
            "走势",
            "能不能买",
            "能不能卖",
            "建议",
            "怎么看",
            "值得买",
            "值得投",
            "抄底",
            "逃顶",
            "预测",
            "前景",
            "analyze",
            "analysis",
            "trend",
            "should i buy",
            "should i sell",
            "advice",
            "recommend",
            "outlook",
            "forecast",
        ];
        let has_analysis_keyword = analysis_keywords.iter().any(|kw| lower.contains(kw));

        // Trigger 2: Selected skill is crypto/finance related
        let selected_crypto = selection.selected_skill.as_ref().map_or(false, |id| {
            let id_lower = id.to_lowercase();
            id_lower.contains("crypto")
                || id_lower.contains("trade")
                || id_lower.contains("alpaca")
                || id_lower.contains("finance")
                || id_lower.contains("invest")
                || id_lower.contains("stock")
                || id_lower.contains("btc")
                || id_lower.contains("eth")
        });

        // Trigger 3: User input contains crypto symbols
        let crypto_symbols = [
            "btc",
            "bitcoin",
            "比特币",
            "eth",
            "ethereum",
            "以太坊",
            "sol",
            "xrp",
            "doge",
            "加密货币",
            "crypto",
            "数字货币",
        ];
        let has_crypto_symbol = crypto_symbols.iter().any(|sym| lower.contains(sym));

        // Trigger 4: Intent indicates multi-step data gathering
        let is_multi_step =
            intent.needs_planning || intent.intent == crate::intent::UserIntent::MultiStepPlanning;

        // 🆕 FIX: ReAct triggers for ANY crypto-related multi-step task,
        // regardless of whether an analysis keyword is present.
        // This ensures "帮我开一单BTC" (no analysis keyword) still routes to ReAct.
        let use_react = has_crypto_symbol && (has_analysis_keyword || is_multi_step);

        if use_react {
            info!(
                "Unified ReAct: triggered for message='{}' (analysis_kw={}, selected_crypto={}, \
                 crypto_sym={}, multi_step={})",
                message_text.chars().take(40).collect::<String>(),
                has_analysis_keyword,
                selected_crypto,
                has_crypto_symbol,
                is_multi_step
            );
        }

        use_react
    }

    /// Execute a task using the Unified ReAct executor with autonomous
    /// planning.
    async fn execute_with_react_planning(
        &self,
        task: &Task,
        message_text: &str,
        intent: &crate::skill_matching::IntentAnalysisV2,
    ) -> Result<(String, Vec<Artifact>), AgentError> {
        let task_id = task.id.clone();
        info!(
            "Unified ReAct: executing task {} with autonomous planning",
            task_id
        );

        // Step 1: Get LLM interface
        let llm = self
            .llm_interface
            .clone()
            .ok_or_else(|| AgentError::InvalidConfig("LLM interface not configured".into()))?;

        // Step 2: Build analysis tools (MCP crypto tools + computed indicators)
        let tools =
            crate::skills::investment_analysis::build_analysis_tools(self.mcp_manager.as_deref())
                .await;

        if tools.is_empty() {
            warn!("Unified ReAct: no analysis tools available, falling back to direct LLM answer");
            return Box::pin(self.handle_direct_answer(task)).await;
        }

        // Step 3: Build user context (risk level, positions, emotional state, etc.)
        let user_risk_level = "moderate"; // TODO: load from user profile
        let user_positions = "未知"; // TODO: load from user portfolio
        let emotional_state = if message_text.contains("慌") || message_text.contains("怕") {
            "焦虑"
        } else if message_text.contains("追") || message_text.contains("错过") {
            "FOMO"
        } else {
            "理性"
        };
        let preferences = "偏好技术指标: RSI, MACD, 布林带"; // TODO: load from user profile
        let psychological_prices = "暂无记录"; // TODO: load from memory

        // Step 4: Build the investment analysis System Prompt
        let system_prompt = crate::skills::investment_analysis::build_investment_analysis_prompt(
            &tools,
            user_risk_level,
            user_positions,
            emotional_state,
            preferences,
            psychological_prices,
        );

        let session_key = task
            .parameters
            .get("session_id")
            .or_else(|| task.parameters.get("channel_id"))
            .cloned()
            .or_else(|| {
                serde_json::from_str::<serde_json::Value>(&task.input)
                    .ok()
                    .and_then(|json| {
                        json.get("session_id")
                            .or_else(|| json.get("channel_id"))
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string())
                    })
            })
            .unwrap_or_else(|| task.id.clone());
        let cancel_rx = Self::cancellation_receiver(&session_key, &task.parameters).await;

        // Step 5: Execute the ReAct loop
        let executor = crate::skills::UnifiedReActExecutor::new(llm).with_config(
            crate::skills::UnifiedReActConfig {
                max_rounds: crate::react_trace::clamp_react_max_tool_rounds(self.max_rounds)
                    as usize,
                round_timeout_sec: 1200,
                tool_timeout_sec: 1200,
                max_parse_failures: 3,
                max_duplicate_tool_calls: 2,
                max_consecutive_tool_errors: 3,
                enable_reflection: true,
                require_structured_output: true,
                cancel_rx,
                stream_tx: None,
            },
        );

        let react_result = executor.execute(&system_prompt, message_text, &tools).await;

        match react_result {
            Ok(raw_content) => {
                info!(
                    "Unified ReAct: task {} completed, result length={}",
                    task_id,
                    raw_content.len()
                );

                // Step 6: Post-process the final answer (safety checks)
                let mut processed =
                    match crate::skills::investment_analysis::post_process_final_answer(
                        &raw_content,
                        user_risk_level,
                    ) {
                        Ok(report_json) => {
                            // Step 7: Format as user-friendly Markdown
                            let formatted =
                                match crate::skills::investment_analysis::format_report_for_user(
                                    &report_json,
                                ) {
                                    Ok(formatted) => formatted,
                                    Err(e) => {
                                        warn!(
                                            "Failed to format report: {}. Returning raw JSON.",
                                            e
                                        );
                                        report_json.clone()
                                    }
                                };

                            // 🆕 FIX: If user originally requested a trade, try to extract
                            // trade_request from the ReAct report and trigger execution.
                            let trade_keywords = [
                                "开单", "下单", "买入", "卖出", "交易", "买", "卖", "order", "buy",
                                "sell", "place", "trade",
                            ];
                            let has_trade_intent = trade_keywords
                                .iter()
                                .any(|kw| message_text.to_lowercase().contains(kw));
                            if has_trade_intent {
                                if let Ok(report) = serde_json::from_str::<
                                crate::skills::investment_analysis::types::InvestmentAnalysisReport,
                            >(&report_json)
                            {
                                if let Some(trade_req) = report.trade_request {
                                    info!(
                                        "ReAct report contains trade_request: {:?} for task {}",
                                        trade_req, task_id
                                    );
                                    // Build natural-language input for parameter extraction
                                    let trade_input = format!(
                                        "{}，{}，{} USD",
                                        trade_req.symbol,
                                        trade_req.side,
                                        trade_req.notional.unwrap_or_else(|| trade_req.qty.clone().unwrap_or_default())
                                    );
                                    // Trigger trade skill execution (will go through approval gate)
                                    match self.execute_skill_by_id(
                                        "mcp:alpaca/place_crypto_order",
                                        &trade_input,
                                        None,
                                    )
                                    .await
                                    {
                                        Ok(skill_result) => {
                                            let mut combined = formatted;
                                            combined.push('\n');
                                            combined.push_str("---");
                                            combined.push('\n');
                                            combined.push_str(&skill_result.output);
                                            return Ok((combined, vec![]));
                                        }
                                        Err(e) => {
                                            warn!("Trade execution after ReAct failed: {}", e);
                                        }
                                    }
                                }
                            }
                            }

                            formatted
                        }
                        Err(e) => {
                            warn!(
                                "Post-processing failed for task {}: {}. Returning raw LLM output.",
                                task_id, e
                            );
                            raw_content
                        }
                    };

                Ok((processed, vec![]))
            }
            Err(e) => {
                warn!(
                    "Unified ReAct: task {} failed: {}. Falling back to direct answer.",
                    task_id, e
                );
                Box::pin(self.handle_direct_answer(task)).await
            }
        }
    }

    /// 🆕 MCP PARAMETER EXTRACTION: Handle structured form submission.
    async fn handle_form_submission(
        &self,
        request_id: &str,
        form_submission: &serde_json::Value,
    ) -> Result<(String, Vec<Artifact>), AgentError> {
        let values = form_submission
            .get("values")
            .and_then(|v| v.as_object())
            .cloned()
            .unwrap_or_default();

        let pending = self
            .pending_parameter_forms
            .write()
            .await
            .remove(request_id);
        let pending = match pending {
            Some(p) if !p.is_expired() => p,
            _ => {
                return Ok(("表单已过期或不存在，请重新发起请求。".to_string(), vec![]));
            }
        };

        // Merge partial params with submitted values
        let mut final_params = pending.partial_params.clone();
        for (k, v) in &values {
            final_params.insert(k.clone(), v.clone());
        }

        info!(
            "Form submission for '{}': merged params: {:?}",
            pending.skill_id,
            final_params.keys().collect::<Vec<_>>()
        );

        // Re-execute the skill with complete parameters
        if let Some(ref registry) = self.skill_registry {
            if let Some(skill) = registry.get(&pending.skill_id).await {
                let params_map: std::collections::HashMap<String, String> = final_params
                    .iter()
                    .map(|(k, v)| {
                        let s = match v {
                            serde_json::Value::String(s) => s.clone(),
                            _ => v.to_string(),
                        };
                        (k.clone(), s)
                    })
                    .collect();
                let skill_result = self
                    .execute_registered_skill(&skill, &pending.user_input, Some(params_map))
                    .await;
                match skill_result {
                    Ok(result) => {
                        let _ = registry.record_usage(&pending.skill_id).await;
                        return Ok((result.output, vec![]));
                    }
                    Err(e) => {
                        return Ok((format!("参数已补充，但执行失败: {}", e), vec![]));
                    }
                }
            }
        }

        Ok(("找不到对应的技能，请重新发起请求。".to_string(), vec![]))
    }

    /// 🆕 MCP PARAMETER EXTRACTION: Handle plain-text form responses.
    /// Attempts to parse comma-separated values into the missing fields.
    async fn handle_text_form_submission(
        &self,
        request_id: &str,
        message_text: &str,
    ) -> Result<(String, Vec<Artifact>), AgentError> {
        let pending = self
            .pending_parameter_forms
            .write()
            .await
            .remove(request_id);
        let pending = match pending {
            Some(p) if !p.is_expired() => p,
            _ => {
                // Form not found or expired — caller should fall through to normal processing
                return Err(AgentError::Execution(
                    "Form expired or not found".to_string(),
                ));
            }
        };

        // Try to parse comma-separated or space-separated values
        let parts: Vec<&str> = message_text
            .split(&[',', '，', '|'])
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();
        let mut final_params = pending.partial_params.clone();

        // Simple heuristic mapping: try to match each part to a missing field by
        // position
        for (i, field) in pending.missing_fields.iter().enumerate() {
            if let Some(value) = parts.get(i) {
                let json_value = match field.param_type.as_str() {
                    "number" | "integer" => {
                        if let Ok(n) = value.parse::<f64>() {
                            serde_json::json!(n)
                        } else {
                            serde_json::json!(value)
                        }
                    }
                    "boolean" => {
                        serde_json::json!(value.to_lowercase() == "true" || *value == "是")
                    }
                    _ => serde_json::json!(value),
                };
                final_params.insert(field.name.clone(), json_value);
            }
        }

        info!(
            "Text form submission for '{}': parsed params: {:?}",
            pending.skill_id,
            final_params.keys().collect::<Vec<_>>()
        );

        // Validate that all originally-missing fields are now present
        let still_missing: Vec<String> = pending
            .missing_fields
            .iter()
            .filter(|f| !final_params.contains_key(&f.name))
            .map(|f| f.name.clone())
            .collect();
        if !still_missing.is_empty() {
            // Re-create the form with updated partial params for the user to try again
            let req_id = uuid::Uuid::new_v4().to_string();
            let remaining_fields: Vec<crate::skills::FieldSchema> = pending
                .missing_fields
                .into_iter()
                .filter(|f| !final_params.contains_key(&f.name))
                .collect();
            let new_form = crate::skills::PendingParameterForm::new(
                req_id.clone(),
                pending.skill_id.clone(),
                pending.user_input.clone(),
                final_params,
                remaining_fields.clone(),
            );
            self.pending_parameter_forms
                .write()
                .await
                .insert(req_id.clone(), new_form);
            let output =
                Self::render_parameter_form(&req_id, &remaining_fields, &serde_json::Map::new());
            return Ok((format!("还有以下信息未提供，请补充：\n{}", output), vec![]));
        }

        // Re-execute the skill with complete parsed parameters
        if let Some(ref registry) = self.skill_registry {
            if let Some(skill) = registry.get(&pending.skill_id).await {
                let params_map: std::collections::HashMap<String, String> = final_params
                    .iter()
                    .map(|(k, v)| {
                        let s = match v {
                            serde_json::Value::String(s) => s.clone(),
                            _ => v.to_string(),
                        };
                        (k.clone(), s)
                    })
                    .collect();
                let skill_result = self
                    .execute_registered_skill(&skill, &pending.user_input, Some(params_map))
                    .await;
                match skill_result {
                    Ok(result) => {
                        let _ = registry.record_usage(&pending.skill_id).await;
                        return Ok((result.output, vec![]));
                    }
                    Err(e) => {
                        return Ok((format!("参数已识别，但执行失败: {}", e), vec![]));
                    }
                }
            }
        }

        Ok(("找不到对应的技能，请重新发起请求。".to_string(), vec![]))
    }

    /// 🆕 SKILL MATCHING V2: Handle LLM task with V2 intent + skill selection
    async fn handle_llm_task_v2(
        &self,
        task: &Task,
        intent: &crate::skill_matching::IntentAnalysisV2,
        _selection: &crate::skill_matching::SkillSelection,
    ) -> Result<(String, Vec<Artifact>), AgentError> {
        // Convert V2 types to legacy types for handler compatibility
        let legacy_intent =
            crate::intent::IntentAnalysis::new(intent.intent.clone(), intent.confidence)
                .with_entities(intent.entities.clone())
                .with_constraints(intent.constraints.clone());

        self.handle_llm_task_with_intent(task, &legacy_intent).await
    }

    /// Legacy task processing path (preserved for backward compatibility)
    async fn process_task_legacy(&self, task: Task) -> Result<(String, Vec<Artifact>), AgentError> {
        let task_id = task.id.clone();

        // 🆕 OPTIMIZATION PHASE 1: Intent Engine前置 — 基于实际消息内容分类意图
        let intent_analysis = if matches!(task.task_type, TaskType::LlmChat | TaskType::Custom(_)) {
            let message_text =
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&task.input) {
                    json.get("message")
                        .and_then(|m| m.as_str())
                        .unwrap_or(&task.input)
                        .to_string()
                } else {
                    task.input.clone()
                };
            self.classify_intent(&message_text).await
        } else {
            crate::intent::IntentAnalysis::new(crate::intent::UserIntent::SingleToolCall, 0.8)
        };

        info!(
            "Intent classified as {:?} (confidence: {:.2}) for task {}",
            intent_analysis.intent, intent_analysis.confidence, task_id
        );

        match &task.task_type {
            TaskType::LlmChat => {
                // 🆕 OPTIMIZATION: Route based on intent classification
                match intent_analysis.intent {
                    crate::intent::UserIntent::DirectAnswer => {
                        // Skip tool injection, direct LLM answer — saves 5k-10k tokens
                        self.handle_direct_answer(&task).await
                    }
                    crate::intent::UserIntent::MetaQuestion => {
                        // Skip LLM, directly return skill registry info
                        self.handle_meta_question(&task).await
                    }
                    crate::intent::UserIntent::Correction => {
                        // Handle correction/modification of previous behavior
                        self.handle_correction(&task, &intent_analysis).await
                    }
                    crate::intent::UserIntent::WorkflowTrigger => {
                        self.handle_workflow_task(&task).await
                    }
                    crate::intent::UserIntent::MultiStepPlanning => {
                        // 🆕 FIX: Legacy P2 Planning replaced by Unified ReAct.
                        self.execute_with_planning(task).await
                    }
                    crate::intent::UserIntent::SingleToolCall => {
                        self.handle_llm_task_with_intent(&task, &intent_analysis)
                            .await
                    }
                }
            }
            TaskType::SkillExecution => self.handle_skill_task(&task).await,
            TaskType::McpTool => self.handle_mcp_task(&task).await,
            TaskType::FileProcessing => self.handle_file_task(&task).await,
            TaskType::A2aSend => self.handle_a2a_task(&task).await,
            TaskType::ChainTransaction => self.handle_chain_transaction_task(&task).await,
            TaskType::PlanCreation => self.handle_plan_creation_task(&task).await,
            TaskType::PlanExecution => self.handle_plan_execution_task(&task).await,
            TaskType::PlanAdaptation => self.handle_plan_adaptation_task(&task).await,
            TaskType::DeviceAutomation => self.handle_device_automation_task(&task).await,
            TaskType::AppLifecycle => self.handle_app_lifecycle_task(&task).await,
            TaskType::WorkflowExecution => self.handle_workflow_task(&task).await,
            TaskType::ForeignPythonWasm
            | TaskType::ForeignPythonProcess
            | TaskType::ForeignNodeJsWasm
            | TaskType::ForeignNodeJsProcess => self.handle_foreign_runtime_task(&task).await,
            TaskType::Custom(type_name) => {
                // 🆕 FIX: All custom planning tasks route through Unified ReAct.
                if self.should_use_planning(&task).await {
                    self.execute_with_planning(task).await
                } else {
                    warn!("Unknown custom task type: {}", type_name);
                    Err(AgentError::InvalidConfig(format!(
                        "Unsupported task type: {}",
                        type_name
                    )))
                }
            }
        }
    }

    /// 🆕 OPTIMIZATION PHASE 1: Handle direct answer intents — no tool
    /// injection, saves tokens 🆕 FIX: Safety net — queries that need
    /// real-time data (weather, crypto, stock) are
    /// routed to skill-injection path instead of pure direct answer, preventing
    /// stale/fabricated data.
    async fn handle_direct_answer(
        &self,
        task: &Task,
    ) -> Result<(String, Vec<Artifact>), AgentError> {
        info!("Direct answer path (no tools) for task {}", task.id);

        let input_text = if let Ok(json) = serde_json::from_str::<serde_json::Value>(&task.input) {
            json.get("message")
                .and_then(|m| m.as_str())
                .unwrap_or(&task.input)
                .to_string()
        } else {
            task.input.clone()
        };
        let lower = input_text.to_lowercase();

        let skip_direct_answer_reroute = task
            .parameters
            .get("_skip_direct_answer")
            .map(|v| v == "true")
            .unwrap_or(false);
        if !skip_direct_answer_reroute
            && self
                .llm_interface
                .as_ref()
                .map_or(false, |llm| llm.supports_native_tools())
        {
            let mut tool_task = task.clone();
            tool_task
                .parameters
                .insert("_skip_direct_answer".to_string(), "true".to_string());
            let intent =
                crate::intent::IntentAnalysis::new(crate::intent::UserIntent::DirectAnswer, 0.8);
            return Box::pin(self.handle_llm_task_with_intent(&tool_task, &intent)).await;
        }

        // Safety net: real-time data queries must go through skill injection
        let needs_realtime_data = lower.contains("天气")
            || lower.contains("weather")
            || lower.contains("temperature")
            || lower.contains("气温")
            || lower.contains("预报")
            || lower.contains("forecast")
            || lower.contains("btc")
            || lower.contains("比特币")
            || lower.contains("eth")
            || lower.contains("以太坊")
            || lower.contains("crypto")
            || lower.contains("加密货币")
            || lower.contains("股价")
            || lower.contains("股票")
            || lower.contains("stock price")
            || lower.contains("aapl")
            || lower.contains("tsla")
            || lower.contains("行情")
            || lower.contains("价格");

        let skip_routing = task
            .parameters
            .get("_skip_planning")
            .map(|v| v == "true")
            .unwrap_or(false);

        if !skip_routing && needs_realtime_data {
            info!(
                "Direct answer intercepted for real-time data query: '{}'",
                input_text
            );
            let legacy_intent =
                crate::intent::IntentAnalysis::new(crate::intent::UserIntent::SingleToolCall, 0.75)
                    .with_toolsets(vec![
                        "weather".to_string(),
                        "crypto-data".to_string(),
                        "stock-data".to_string(),
                    ]);
            return Box::pin(self.handle_llm_task_with_intent(task, &legacy_intent)).await;
        }

        let llm = self
            .llm_interface
            .as_ref()
            .ok_or_else(|| AgentError::InvalidConfig("LLM interface not configured".into()))?;

        let messages = vec![
            communication::Message::new(
                uuid::Uuid::new_v4(),
                communication::PlatformType::Custom,
                format!(
                    "你是 {} ({})。回答问题时请保持友好、专业、有帮助。\
                     所有内部思考过程和工具调用理由必须使用中文。",
                    self.config.name, self.config.description
                ),
            ),
            communication::Message::new(
                uuid::Uuid::new_v4(),
                communication::PlatformType::Custom,
                format!("用户: {}", input_text),
            ),
        ];

        // 🆕 FIX: Limit max_tokens to 1024 for direct answers to prevent
        // Kimi k2.6 thinking mode from generating excessive reasoning tokens.
        let mut context = std::collections::HashMap::new();
        context.insert("max_tokens".to_string(), "1024".to_string());

        let response = llm
            .call_llm(messages, Some(context))
            .await
            .map_err(|e| AgentError::Execution(format!("LLM call failed: {}", e)))?;

        Ok((response, vec![]))
    }

    /// 🆕 OPTIMIZATION PHASE 1: Handle meta questions — return skill info
    /// directly
    async fn handle_meta_question(
        &self,
        task: &Task,
    ) -> Result<(String, Vec<Artifact>), AgentError> {
        info!("Meta question path for task {}", task.id);
        let registry = self
            .skill_registry
            .as_ref()
            .ok_or_else(|| AgentError::InvalidConfig("Skill registry not configured".into()))?;

        let skills = registry.list_enabled().await;
        let skill_list: Vec<String> = skills
            .iter()
            .map(|s| {
                format!(
                    "- {}: {}",
                    s.skill.name,
                    s.skill
                        .manifest
                        .description
                        .chars()
                        .take(60)
                        .collect::<String>()
                )
            })
            .collect();

        let response = format!(
            "我是 {}，目前可用的技能包括：\n\n{}\n\n您可以直接描述需求，\
             我会自动调用合适的技能来帮您完成。",
            self.config.name,
            skill_list.join("\n")
        );

        Ok((response, vec![]))
    }

    /// 🆕 OPTIMIZATION PHASE 1: Handle correction intents
    async fn handle_correction(
        &self,
        task: &Task,
        intent: &crate::intent::IntentAnalysis,
    ) -> Result<(String, Vec<Artifact>), AgentError> {
        info!(
            "Correction path for task {}: constraints={:?}",
            task.id, intent.constraints
        );
        // For now, treat correction as a direct LLM task with context about constraints
        // In a full implementation, this would modify/undo previous actions
        let input_text = if let Ok(json) = serde_json::from_str::<serde_json::Value>(&task.input) {
            json.get("message")
                .and_then(|m| m.as_str())
                .unwrap_or(&task.input)
                .to_string()
        } else {
            task.input.clone()
        };

        let llm = self
            .llm_interface
            .as_ref()
            .ok_or_else(|| AgentError::InvalidConfig("LLM interface not configured".into()))?;

        let constraint_text = if intent.constraints.is_empty() {
            "用户要求修改或撤销之前的操作。".to_string()
        } else {
            format!("用户约束: {}", intent.constraints.join(", "))
        };

        let messages = vec![
            communication::Message::new(
                uuid::Uuid::new_v4(),
                communication::PlatformType::Custom,
                format!("{}", constraint_text),
            ),
            communication::Message::new(
                uuid::Uuid::new_v4(),
                communication::PlatformType::Custom,
                format!("用户: {}", input_text),
            ),
        ];

        let response = llm
            .call_llm(messages, None)
            .await
            .map_err(|e| AgentError::Execution(format!("LLM call failed: {}", e)))?;

        Ok((response, vec![]))
    }

    /// 🆕 OPTIMIZATION PHASE 1: Handle LLM task with intent-aware tool
    /// filtering
    async fn handle_llm_task_with_intent(
        &self,
        task: &Task,
        intent: &crate::intent::IntentAnalysis,
    ) -> Result<(String, Vec<Artifact>), AgentError> {
        Box::pin(self.handle_llm_task_internal(task, Some(intent))).await
    }

    /// Original handle_llm_task — delegates to internal implementation
    #[allow(dead_code)]
    async fn handle_llm_task(&self, task: &Task) -> Result<(String, Vec<Artifact>), AgentError> {
        self.handle_llm_task_internal(task, None).await
    }

    /// Internal LLM task handler with optional intent for tool filtering
    async fn handle_llm_task_internal(
        &self,
        task: &Task,
        intent_opt: Option<&crate::intent::IntentAnalysis>,
    ) -> Result<(String, Vec<Artifact>), AgentError> {
        // 🆕 PLANNING FIX: 基于实际消息内容判断复杂度，复杂任务使用 planning 执行
        let (message_text, skill_hint) =
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&task.input) {
                let msg = json
                    .get("message")
                    .and_then(|m| m.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| task.input.clone());
                // 🆕 SKILL MATCHING V2: Check for skill_hint_v2 first, fallback to skill_hint
                let hint = json
                    .get("skill_hint_v2")
                    .cloned()
                    .or_else(|| json.get("skill_hint").cloned());
                (msg, hint)
            } else {
                (task.input.clone(), None)
            };

        let char_count = message_text.chars().count();
        let has_explicit_planning_param = task.parameters.contains_key("multi_step")
            || task.parameters.contains_key("dependencies")
            || task.parameters.contains_key("plan");

        // 🆕 FIX: 优化 planning 触发条件，适配中文场景
        // 1. 明确标记的参数总是触发
        // 2. 中等文本(>50字)且含规划关键词，或含多步骤连接词(先...再...然后)
        // 3. 较长文本(>120字)默认触发
        // 4. 英文场景保持原阈值(>200 chars)
        let has_planning_keywords = message_text.contains("计划")
            || message_text.contains("步骤")
            || message_text.contains("安排")
            || message_text.contains("规划")
            || message_text.contains("方案")
            || message_text.contains("攻略")
            || message_text.contains("流程");
        let has_multi_step_indicators = (message_text.contains("先")
            || message_text.contains("首先"))
            && (message_text.contains("再")
                || message_text.contains("然后")
                || message_text.contains("最后")
                || message_text.contains("接着"));

        // 中文文本密度高，适当降低阈值
        let is_chinese = message_text
            .chars()
            .any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c));
        let planning_threshold = if is_chinese { 50 } else { 120 };
        let long_threshold = if is_chinese { 120 } else { 300 };

        // 🆕 FIX: 强规划关键词（如"计划"/"规划"/"攻略"）即使短文本也应触发 planning，
        // 避免"去汕头市旅游五天的计划"（14字）因低于50字阈值而被误判为简单查询。
        // 设置 6 字符下限防止单字误触发（如"计"）。
        // 🆕 SKILL MATCHING V2: Removed hardcoded generative skill exclusions.
        // Planning need is determined by the query semantics, not skill name keywords.

        let is_complex = has_explicit_planning_param
            || (has_planning_keywords && char_count >= 6)
            || has_multi_step_indicators
            || (char_count > planning_threshold
                && (has_planning_keywords || has_multi_step_indicators))
            || char_count > long_threshold;

        let skip_planning = task
            .parameters
            .get("_skip_planning")
            .map(|v| v == "true")
            .unwrap_or(false);

        if !skip_planning && is_complex {
            info!(
                "🧠 Complex LLM task detected (message length: {}), using Unified ReAct for task \
                 {}",
                message_text.len(),
                task.id
            );
            return Box::pin(self.execute_with_planning(task.clone())).await;
        }

        // 🆕 P2 FIX: Auto-pipeline detection for multi-step skill chaining
        if let Some(pipeline) = self.try_build_auto_pipeline(&message_text).await {
            info!(
                "🔄 Auto-pipeline detected for task {}, executing {} steps",
                task.id,
                pipeline.steps.len()
            );
            match pipeline.execute(&message_text, self).await {
                Ok(result) => {
                    return Ok((
                        result.clone(),
                        vec![Artifact {
                            id: task.id.clone(),
                            artifact_type: "pipeline_result".to_string(),
                            content: result.as_bytes().to_vec(),
                            mime_type: "text/plain".to_string(),
                        }],
                    ));
                }
                Err(e) => {
                    warn!(
                        "Auto-pipeline execution failed for task {}: {}, falling back to LLM",
                        task.id, e
                    );
                }
            }
        }

        let llm = self
            .llm_interface
            .as_ref()
            .ok_or_else(|| AgentError::InvalidConfig("LLM interface not configured".into()))?;

        // Parse structured input JSON to extract current message and context metadata
        let (
            input_text,
            mut extra_params,
            image_urls,
            history,
            gateway_memory_context,
            weather_data,
        ) = if let Ok(json) = serde_json::from_str::<serde_json::Value>(&task.input) {
            let message = json
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or(&task.input)
                .to_string();
            let mut params = task.parameters.clone();
            if let Some(platform) = json.get("platform").and_then(|p| p.as_str()) {
                params.insert("platform".to_string(), platform.to_string());
            }
            if let Some(channel_id) = json.get("channel_id").and_then(|c| c.as_str()) {
                params.insert("channel_id".to_string(), channel_id.to_string());
            }
            if let Some(user_id) = json.get("user_id").and_then(|u| u.as_str()) {
                params.insert("user_id".to_string(), user_id.to_string());
            }
            if let Some(session_id) = json.get("session_id").and_then(|s| s.as_str()) {
                params.insert("session_id".to_string(), session_id.to_string());
            }
            let images: Vec<String> = json
                .get("images")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();
            let history: Vec<(String, String)> = json
                .get("history")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| {
                            let role = v.get("role").and_then(|r| r.as_str())?;
                            let content = v.get("content").and_then(|c| c.as_str())?;
                            Some((role.to_string(), content.to_string()))
                        })
                        .collect()
                })
                .unwrap_or_default();
            let memory_context = json
                .get("memory_context")
                .and_then(|m| m.as_str())
                .map(|s| s.to_string());
            let weather_data = json
                .get("weather_data")
                .and_then(|m| m.as_str())
                .map(|s| s.to_string());
            (
                message,
                params,
                images,
                history,
                memory_context,
                weather_data,
            )
        } else {
            (
                task.input.clone(),
                task.parameters.clone(),
                Vec::new(),
                Vec::new(),
                None,
                None,
            )
        };

        let mut metadata = std::collections::HashMap::new();
        if !image_urls.is_empty() {
            metadata.insert(
                "image_urls".to_string(),
                serde_json::to_string(&image_urls).unwrap_or_default(),
            );
        }

        // 🆕 FIX: When gateway has already matched a skill, execute it directly.
        // This bypasses the LLM call for MCP tools and other registered skills that
        // the gateway has confidently matched.
        if let Some(ref hint) = skill_hint {
            if let Some(skill_id) = hint.get("id").and_then(|v| v.as_str()) {
                // Try registry first
                let registry_result = if let Some(ref registry) = self.skill_registry {
                    registry.get(skill_id).await
                } else {
                    None
                };

                if let Some(registered) = registry_result {
                    info!(
                        "Gateway matched skill '{}', executing directly without LLM call",
                        skill_id
                    );
                    // 🆕 FIX: Enrich input with gateway-provided context (weather_data, etc.)
                    let mut enriched_input = if let Some(ref weather) = weather_data {
                        if !weather.is_empty() {
                            format!(
                                "{}\n\n[参考数据] 实时天气：{}\n请基于以上数据回答。",
                                input_text, weather
                            )
                        } else {
                            input_text.clone()
                        }
                    } else {
                        input_text.clone()
                    };

                    // 🆕 FIX: Inject conversation history into knowledge skill input so
                    // multi-turn skills (e.g. Travel Planner) can see the full context.
                    // Without this, the skill only sees the current message and asks for
                    // information the user already provided in earlier turns.
                    if !history.is_empty() {
                        let mut context = String::new();
                        for (role, content) in &history {
                            let prefix = match role.as_str() {
                                "user" => "用户",
                                "assistant" => "助手",
                                "system" => "系统",
                                _ => &role,
                            };
                            context.push_str(&format!("{}: {}\n", prefix, content));
                        }
                        context.push_str(&format!("用户: {}\n", enriched_input));
                        enriched_input = context;
                    }

                    let skill_result = self
                        .execute_registered_skill(&registered, &enriched_input, None)
                        .await;
                    match skill_result {
                        Ok(result) => {
                            if let Some(ref registry) = self.skill_registry {
                                let _ = registry.record_usage(skill_id).await;
                            }
                            let output =
                                self.synthesize_skill_output(&input_text, &result.output, skill_id);
                            return Ok((output, vec![]));
                        }
                        Err(e) => {
                            let err_str = e.to_string();
                            // 🆕 FIX: If direct execution failed due to missing arguments,
                            // fall back to LLM so it can extract parameters from natural language.
                            if err_str.contains("Missing required parameter")
                                || err_str.contains("argument validation failed")
                                || err_str.contains("Unknown parameter")
                            {
                                warn!(
                                    "Direct skill execution for '{}' failed due to args issue, \
                                     falling back to LLM: {}",
                                    skill_id, e
                                );
                                // Continue to LLM path below instead of
                                // returning error
                            } else {
                                warn!("Direct skill execution for '{}' failed: {}", skill_id, e);
                                return Ok((
                                    format!("执行 skill '{}' 时出错: {}", skill_id, e),
                                    vec![],
                                ));
                            }
                        }
                    }
                } else {
                    warn!(
                        "Gateway matched skill '{}' but not found in registry",
                        skill_id
                    );
                }
            }
        }

        // Build message list with memory context, history, and current message
        let mut messages: Vec<communication::Message> = Vec::new();

        // 🆕 FIX: 当 gateway 传入 skill_hint 时，使用其 prompt_template 作为核心
        // persona。 若 gateway 已通过 memory_context 注入 skill
        // prompt（当前标准行为），则 persona 只做轻量标识，
        // 避免同一份 prompt_template 在 persona message 和 memory message
        // 中重复出现，浪费 token。 🆕 OPTIMIZATION PHASE 2: Use cached base
        // persona when no skill_hint
        let base_persona = self
            .build_system_prompt_cached(
                intent_opt
                    .map(|i| &i.intent)
                    .unwrap_or(&crate::intent::UserIntent::DirectAnswer),
            )
            .await;

        let persona = if let Some(ref hint) = skill_hint {
            let name = hint
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or(&self.config.name);
            let prompt_template = hint
                .get("prompt_template")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if !prompt_template.is_empty() {
                // 检查 gateway 是否已把 skill prompt 注入 memory_context
                let gateway_has_skill_prompt = gateway_memory_context.as_ref().map_or(false, |m| {
                    m.contains(prompt_template.trim().split('\n').next().unwrap_or(""))
                });
                if gateway_has_skill_prompt {
                    // Gateway 已注入完整 skill prompt，persona 只做轻量标识
                    format!(
                        "你是 {}。回答问题时请保持友好、专业、有帮助。\
                         所有内部思考过程和工具调用理由必须使用中文。",
                        name
                    )
                } else {
                    // Gateway 未注入，使用 skill prompt_template 作为 persona
                    format!("[角色] {}\n\n{}", name, prompt_template)
                }
            } else {
                let desc = hint
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&self.config.description);
                format!(
                    "你是 {} ({})。回答问题时请保持友好、专业、有帮助。\
                     所有内部思考过程和工具调用理由必须使用中文。",
                    name, desc
                )
            }
        } else {
            base_persona
        };

        // 🆕 FIX: Append skill-catalog trigger instruction to persona so the LLM
        // knows to emit SKILL:<id> when the user request matches a registered skill.
        // 🆕 SKILL MATCHING V2: Removed hardcoded generative skill exclusions.
        let persona = if self.skill_catalog.is_some() {
            if let Some(ref hint) = skill_hint {
                // 🆕 FIX: Gateway already matched a skill; tell LLM to emit SKILL:id|params
                // directly
                let skill_id = hint.get("id").and_then(|v| v.as_str()).unwrap_or("");
                format!(
                    "{}\n\n[系统指令] Gateway 建议使用的 skill 是 '{}'. 如果该 skill \
                     能直接满足用户请求，请只回复 SKILL:{}|{{\"param\":\"value\"}} \
                     格式，不要添加任何解释。如果该 skill \
                     无法满足用户请求（例如用户要求的功能不在该 skill \
                     范围内），请直接以自然语言回答用户的问题，不要强行调用不匹配的 skill。",
                    persona, skill_id, skill_id
                )
            } else {
                format!(
                    "{}\n\n[系统指令] 当用户请求与某个 skill 匹配时，请只回复 \
                     SKILL:<skill_id>|{{\"param\":\"value\"}}，不要提供其他解释。",
                    persona
                )
            }
        } else {
            persona
        };
        // 🆕 FIX: Force direct answer — Kimi k2.6 tends to explain system instructions
        let mut persona = format!(
            "{}\n\n[强制规则] \
             直接回答用户问题，不要解释你收到了什么数据、什么技能指引或系统指令。禁止以\"\
             用户问的是...\"、\"系统提示我...\"、\"我需要...\"、\"根据规则...\"开头。",
            persona
        );
        let workspace_context = format!(
            "\n\n## Workspace\nYour working directory is: {}\nTreat this directory as the single \
             default workspace for relative file operations unless the user explicitly gives a \
             narrower or absolute path. By default this is ./data/workspace relative to the \
             BeeBotOS repository, not the repository root.\n\n## Built-in Workspace Tools\nYou \
             can call these top-level tools when the user asks about files, code, commands, or \
             the current environment:\n- read_file(path, max_chars?): read a UTF-8 file\n- \
             list_dir(path?): list a directory\n{} Use available tools instead of claiming you \
             cannot inspect the environment.",
            self.workspace_dir_display(),
            if self.controlled_workspace_tools_enabled() {
                "Controlled tools are enabled: write_file(path, content), edit_file(path, \
                 old_text, new_text, replace_all?), and exec(command, working_dir?, timeout_ms?) \
                 are available. Relative paths and missing working_dir default to this workspace; \
                 absolute paths and absolute working_dir values are allowed."
            } else {
                "Controlled tools such as write_file, edit_file, and exec exist but are disabled \
                 unless runtime policy explicitly enables them."
            }
        );
        persona.push_str(&workspace_context);

        // 🆕 OPTIMIZATION PHASE 2: Add REASONING_SCRATCHPAD hint for complex tasks
        if matches!(
            intent_opt.map(|i| &i.intent),
            Some(crate::intent::UserIntent::MultiStepPlanning)
        ) {
            persona.push_str(
                "\n\n[推理指南] 这是一个复杂任务，请按以下步骤思考并在回复中包含 \
                 <REASONING_SCRATCHPAD> 标签：\n1. 分析用户目标\n2. 确定需要调用的工具及顺序\n3. \
                 验证每一步的依赖关系\n输出格式：<REASONING_SCRATCHPAD>你的思考过程</\
                 REASONING_SCRATCHPAD>\n然后输出实际回答或工具调用。",
            );
        }

        messages.push(communication::Message::new(
            uuid::Uuid::new_v4(),
            communication::PlatformType::Custom,
            persona,
        ));

        // 🟢 P1 FIX: Use gateway-provided memory_context if available (avoids redundant
        // search + dirty query)
        if let Some(ref gateway_memory) = gateway_memory_context {
            if !gateway_memory.is_empty() {
                info!(
                    "Using gateway-provided memory context ({} chars) for agent {}",
                    gateway_memory.len(),
                    self.config.id
                );
                messages.push(communication::Message::new(
                    uuid::Uuid::new_v4(),
                    communication::PlatformType::Custom,
                    format!(
                        "[系统提示：以下是该用户的历史记忆，回答时必须结合这些信息]\n{}",
                        gateway_memory
                    ),
                ));
            }
        }
        // Weather data will be appended to the user message below instead of a separate
        // system message
        if let Some(ref memory) = self.memory_system {
            // Fallback: local search using ONLY input_text (never concatenate history —
            // prevents self-referential duplication)
            let query = input_text.clone();

            // 🆕 FIX (方案B): fallback 记忆检索也采用独立预算
            let char_count = query.chars().count();
            let is_simple = char_count <= 10;
            let is_complex = char_count > 30
                || query.contains("计划")
                || query.contains("规划")
                || query.contains("步骤")
                || query.contains("安排")
                || query.contains("攻略")
                || query.contains("对比")
                || query.contains("分析")
                || query.contains("总结");
            let search_limit = if is_complex {
                6
            } else if char_count > 15 {
                4
            } else {
                2
            };
            let max_memory_chars = if is_simple {
                400
            } else if is_complex {
                1200
            } else {
                800
            };

            match memory.search(&query).await {
                Ok(results) => {
                    info!(
                        "Agent {} local memory search returned {} results (limit={}) for query \
                         '{}'..",
                        self.config.id,
                        results.len(),
                        search_limit,
                        query.chars().take(40).collect::<String>()
                    );
                    let input_lower = input_text.to_lowercase();
                    let mut total_chars = 0;
                    let memory_context: String = results
                        .iter()
                        .filter(|r| {
                            // Skip memories that are essentially the current query being repeated
                            let is_self_referential =
                                r.content.to_lowercase().contains(&input_lower);
                            if is_self_referential {
                                info!(
                                    "Filtering out self-referential memory: {}",
                                    r.content.chars().take(40).collect::<String>()
                                );
                            }
                            !is_self_referential
                        })
                        .take(search_limit)
                        .filter_map(|r| {
                            let entry = format!("- {}", r.content);
                            if total_chars + entry.len() > max_memory_chars {
                                if total_chars == 0 {
                                    // First entry already too long, truncate it
                                    // FIX: Use chars() to avoid slicing in the middle of a UTF-8
                                    // char
                                    let trunc_len = max_memory_chars.saturating_sub(4);
                                    let truncated = format!(
                                        "- {}...",
                                        r.content.chars().take(trunc_len).collect::<String>()
                                    );
                                    total_chars += truncated.len();
                                    return Some(truncated);
                                }
                                return Some("- ...（更多记忆已省略）".to_string());
                            }
                            total_chars += entry.len();
                            Some(entry)
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    if !memory_context.is_empty() {
                        info!(
                            "Injecting memory context ({} chars) into agent LLM prompt",
                            memory_context.len()
                        );
                        messages.push(communication::Message::new(
                            uuid::Uuid::new_v4(),
                            communication::PlatformType::Custom,
                            format!(
                                "[系统提示：以下是该用户的历史记忆，回答时必须结合这些信息]\n{}",
                                memory_context
                            ),
                        ));
                    }
                }
                Err(e) => {
                    warn!("Memory search failed for agent {}: {}", self.config.id, e);
                }
            }
        }

        for (role, content) in history {
            let prefix = match role.as_str() {
                "user" => "用户",
                "assistant" => "助手",
                "system" => "系统",
                _ => &role,
            };
            messages.push(communication::Message::new(
                uuid::Uuid::new_v4(),
                communication::PlatformType::Custom,
                format!("{}: {}", prefix, content),
            ));
        }

        // Add current user message (with weather data appended if available)
        let user_message = if let Some(ref weather) = weather_data {
            if !weather.is_empty() {
                format!(
                    "用户: {}\n\n[参考数据] 实时天气：{}\n请基于以上数据回答。",
                    input_text, weather
                )
            } else {
                format!("用户: {}", input_text)
            }
        } else {
            format!("用户: {}", input_text)
        };
        messages.push(communication::Message::with_metadata(
            uuid::Uuid::new_v4(),
            communication::PlatformType::Custom,
            user_message,
            metadata,
        ));

        // 🟢 P2 FIX: Dynamic max_tokens based on message complexity
        // 🆕 SKILL MATCHING V2: Removed hardcoded generative skill exclusions.
        let dynamic_max_tokens = "8192".to_string();
        extra_params.insert("max_tokens".to_string(), dynamic_max_tokens);

        // 🆕 OPTIMIZATION PHASE 1: Intent-aware tool filtering with Toolsets
        // Adjust tool count based on intent: DirectAnswer=0, SingleToolCall=10,
        // MultiStepPlanning=20
        let top_n = match intent_opt.map(|i| &i.intent) {
            Some(crate::intent::UserIntent::DirectAnswer) => 0usize,
            Some(crate::intent::UserIntent::SingleToolCall) => 10usize,
            Some(crate::intent::UserIntent::MetaQuestion) => 0usize,
            _ => 20usize,
        };

        let mut tool_name_map: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        let mut native_tools: Vec<communication::ToolDefinition> = self.builtin_workspace_tools();
        self.add_mcp_tool_search_if_available(&mut native_tools);
        if let Some(ref registry) = self.skill_registry {
            let all_skills = registry.list_all().await;
            if !all_skills.is_empty() && top_n > 0 {
                let query_lower = input_text.to_lowercase();
                let stopwords: std::collections::HashSet<&str> = [
                    "的",
                    "了",
                    "是",
                    "我",
                    "你",
                    "他",
                    "她",
                    "它",
                    "们",
                    "在",
                    "有",
                    "和",
                    "就",
                    "不",
                    "人",
                    "都",
                    "一",
                    "一个",
                    "上",
                    "也",
                    "很",
                    "到",
                    "说",
                    "要",
                    "去",
                    "可以",
                    "会",
                    "这",
                    "那",
                    "有",
                    "个",
                    "之",
                    "与",
                    "及",
                    "等",
                    "从",
                    "让",
                    "向",
                    "往",
                    "为",
                    "被",
                    "把",
                    "给",
                    "请",
                    "帮",
                    "来",
                    "做",
                    "看",
                    "想",
                    "知道",
                    "一下",
                    "根据",
                    "当前",
                    "现在",
                    "市场",
                    "形势",
                    "这个",
                    "那个",
                    "the",
                    "a",
                    "an",
                    "is",
                    "are",
                    "was",
                    "were",
                    "be",
                    "been",
                    "being",
                    "have",
                    "has",
                    "had",
                    "do",
                    "does",
                    "did",
                    "will",
                    "would",
                    "could",
                    "should",
                    "may",
                    "might",
                    "must",
                    "shall",
                    "can",
                    "need",
                    "dare",
                    "ought",
                    "used",
                    "to",
                    "of",
                    "in",
                    "for",
                    "on",
                    "with",
                    "at",
                    "by",
                    "from",
                    "as",
                    "into",
                    "through",
                    "during",
                    "before",
                    "after",
                    "above",
                    "below",
                    "between",
                    "under",
                    "and",
                    "but",
                    "or",
                    "yet",
                    "so",
                    "if",
                    "because",
                    "although",
                    "though",
                    "while",
                    "where",
                    "when",
                    "that",
                    "which",
                    "who",
                    "whom",
                    "whose",
                    "what",
                    "how",
                    "why",
                    "it",
                    "its",
                    "this",
                    "these",
                    "those",
                    "i",
                    "me",
                    "my",
                    "myself",
                    "we",
                    "our",
                    "you",
                    "your",
                    "he",
                    "him",
                    "his",
                    "she",
                    "her",
                    "they",
                    "them",
                    "their",
                    "just",
                    "only",
                    "even",
                    "also",
                    "too",
                    "very",
                    "so",
                    "such",
                    "no",
                    "not",
                    "than",
                    "then",
                    "now",
                    "here",
                    "there",
                    "up",
                    "out",
                    "off",
                    "down",
                    "over",
                    "again",
                    "further",
                    "once",
                    "more",
                    "most",
                    "other",
                    "some",
                    "any",
                    "each",
                    "few",
                    "much",
                    "many",
                    "all",
                    "both",
                    "either",
                    "neither",
                    "one",
                    "two",
                    "first",
                    "last",
                    "good",
                    "new",
                    "old",
                    "great",
                    "high",
                    "small",
                    "different",
                    "large",
                    "next",
                    "early",
                    "young",
                    "important",
                    "public",
                    "same",
                    "able",
                ]
                .iter()
                .cloned()
                .collect();

                // 🆕 FIX: Split on non-alphanumeric; for CJK text also extract individual
                // characters so Chinese queries can match skills with Chinese descriptions.
                let mut keywords: Vec<String> = Vec::new();
                for part in query_lower.split(|c: char| !c.is_alphanumeric() && c != '/') {
                    if part.is_empty() || part.len() < 2 {
                        continue;
                    }
                    if !stopwords.contains(part) {
                        keywords.push(part.to_string());
                    }
                }
                keywords.sort();
                keywords.dedup();

                // Expand crypto/trading related terms
                let query_lower_str = query_lower.as_str();
                if query_lower_str.contains("btc") || query_lower_str.contains("bitcoin") {
                    keywords.push("btc".to_string());
                    keywords.push("bitcoin".to_string());
                    keywords.push("crypto".to_string());
                }
                if query_lower_str.contains("eth") || query_lower_str.contains("ethereum") {
                    keywords.push("eth".to_string());
                    keywords.push("ethereum".to_string());
                    keywords.push("crypto".to_string());
                }
                if query_lower_str.contains("下单")
                    || query_lower_str.contains("order")
                    || query_lower_str.contains("buy")
                    || query_lower_str.contains("sell")
                {
                    keywords.push("order".to_string());
                    keywords.push("trade".to_string());
                    keywords.push("trading".to_string());
                    keywords.push("place".to_string());
                    keywords.push("下单".to_string());
                }
                if query_lower_str.contains("行情")
                    || query_lower_str.contains("price")
                    || query_lower_str.contains("snapshot")
                {
                    keywords.push("snapshot".to_string());
                    keywords.push("price".to_string());
                    keywords.push("quote".to_string());
                    keywords.push("market".to_string());
                    keywords.push("行情".to_string());
                }
                // 🆕 FIX (Plan D): Expand search-related keywords to ensure web search skills
                // are ranked
                if query_lower_str.contains("搜索")
                    || query_lower_str.contains("search")
                    || query_lower_str.contains("查找")
                    || query_lower_str.contains("查一下")
                    || query_lower_str.contains("网上")
                    || query_lower_str.contains("google")
                    || query_lower_str.contains("百度")
                    || query_lower_str.contains("look up")
                    || query_lower_str.contains("find online")
                    || query_lower_str.contains("搜")
                {
                    keywords.push("search".to_string());
                    keywords.push("web_search".to_string());
                    keywords.push("web".to_string());
                    keywords.push("查找".to_string());
                }

                // Detect explicit trading intent to boost order-placement tools
                let has_trading_intent = query_lower_str.contains("下单")
                    || query_lower_str.contains("购买")
                    || query_lower_str.contains("买入")
                    || query_lower_str.contains("卖出")
                    || query_lower_str.contains("buy")
                    || query_lower_str.contains("sell")
                    || query_lower_str.contains("order")
                    || query_lower_str.contains("place");

                // 🆕 FIX: Detect weather intent to boost weather-related skills
                let has_weather_intent = query_lower_str.contains("天气")
                    || query_lower_str.contains("weather")
                    || query_lower_str.contains("temperature")
                    || query_lower_str.contains("气温")
                    || query_lower_str.contains("forecast")
                    || query_lower_str.contains("预报")
                    || query_lower_str.contains("rain")
                    || query_lower_str.contains("雨")
                    || query_lower_str.contains("snow")
                    || query_lower_str.contains("雪");
                if has_weather_intent {
                    keywords.push("weather".to_string());
                    keywords.push("get_weather".to_string());
                    keywords.push("forecast".to_string());
                }

                // 🆕 OPTIMIZATION PHASE 1: Toolsets-based filter-first + scoring
                let active_toolsets: std::collections::HashSet<String> = intent_opt
                    .map(|i| i.active_toolsets.iter().cloned().collect())
                    .unwrap_or_default();

                // Phase 1: Filter — only keep skills from active toolsets (if any are detected)
                let mut candidates: Vec<&skills::registry::RegisteredSkill> = all_skills
                    .iter()
                    .filter(|s| s.enabled)
                    .filter(|s| {
                        if active_toolsets.is_empty() {
                            return true; // no toolset constraints
                        }
                        let skill_id_lower = s.skill.id.to_lowercase();
                        active_toolsets
                            .iter()
                            .any(|ts| skill_id_lower.contains(ts) || s.tags.contains(ts))
                    })
                    .collect();

                // Fallback: if too few matches after toolset filtering, use all enabled
                if candidates.len() < 3 && !active_toolsets.is_empty() {
                    candidates = all_skills.iter().filter(|s| s.enabled).collect();
                }

                // Phase 2: Score within filtered candidates
                let mut scored_skills: Vec<(usize, &skills::registry::RegisteredSkill)> =
                    Vec::new();
                for registered in &candidates {
                    let manifest = &registered.skill.manifest;
                    let searchable =
                        format!("{} {} {}", manifest.id, manifest.name, manifest.description)
                            .to_lowercase();
                    let mut score = keywords
                        .iter()
                        .filter(|k| searchable.contains(k.as_str()))
                        .count();

                    // 🆕 FIX: Boost order placement tools when user explicitly wants to trade
                    if has_trading_intent {
                        let skill_id_lower = manifest.id.to_lowercase();
                        if skill_id_lower.contains("place_") && skill_id_lower.contains("_order") {
                            score += 20;
                        }
                    }

                    // 🆕 FIX: Boost weather skills when user asks about weather
                    if has_weather_intent {
                        let skill_id_lower = manifest.id.to_lowercase();
                        if skill_id_lower.contains("weather")
                            || skill_id_lower.contains("get_weather")
                        {
                            score += 20;
                        }
                    }
                    if score > 0 {
                        scored_skills.push((score, *registered));
                    }
                }

                // Sort by relevance (highest first) and take top N based on intent
                scored_skills.sort_by(|a, b| b.0.cmp(&a.0));
                let selected = if scored_skills.len() >= 3 {
                    scored_skills
                        .into_iter()
                        .take(top_n)
                        .map(|(_, s)| s)
                        .collect::<Vec<_>>()
                } else {
                    // 🆕 FIX: When too few keyword matches, still prioritize scored skills.
                    // Inject only scored skills + enough top enabled skills to reach 3,
                    // instead of flooding the LLM with 30 unrelated tools.
                    let mut selected: Vec<&skills::registry::RegisteredSkill> =
                        scored_skills.into_iter().map(|(_, s)| s).collect();
                    if selected.len() < 3 {
                        for s in all_skills.iter().filter(|s| s.enabled) {
                            if !selected.iter().any(|sel| sel.skill.id == s.skill.id) {
                                selected.push(s);
                            }
                            if selected.len() >= 3 {
                                break;
                            }
                        }
                    }
                    selected
                };

                let mut tools = Vec::new();
                for registered in &selected {
                    let manifest = &registered.skill.manifest;
                    let func = manifest.functions.first();
                    let (params_schema, desc) = if let Some(f) = func {
                        let mut props = serde_json::Map::new();
                        let mut required = Vec::new();
                        for param in &f.inputs {
                            let mut prop = serde_json::Map::new();
                            prop.insert(
                                "type".to_string(),
                                serde_json::Value::String(param.param_type.clone()),
                            );
                            if !param.description.is_empty() {
                                prop.insert(
                                    "description".to_string(),
                                    serde_json::Value::String(param.description.clone()),
                                );
                            }
                            props.insert(param.name.clone(), serde_json::Value::Object(prop));
                            if param.required {
                                required.push(param.name.clone());
                            }
                        }
                        let schema = serde_json::json!({
                            "type": "object",
                            "properties": props,
                            "required": required,
                        });
                        // 🆕 FIX: Avoid duplicating manifest prefix + function docstring.
                        // Use function description if available (it's usually the focused tool
                        // description), otherwise fall back to manifest description.
                        let description = if f.description.is_empty() {
                            manifest.description.clone()
                        } else {
                            f.description.clone()
                        };
                        (schema, description)
                    } else {
                        (
                            serde_json::json!({"type": "object", "properties": {}, "required": []}),
                            manifest.description.clone(),
                        )
                    };

                    let original_name = registered.skill.id.clone();
                    let normalized_name = original_name.replace(':', "-").replace('/', "-");
                    tool_name_map.insert(normalized_name.clone(), original_name);

                    tools.push(communication::ToolDefinition {
                        name: normalized_name,
                        description: desc,
                        parameters: params_schema,
                    });
                }

                if !tools.is_empty() {
                    native_tools.extend(tools);
                    match serde_json::to_string(&native_tools) {
                        Ok(json) => {
                            extra_params.insert("tools_json".to_string(), json);
                            info!(
                                "handle_llm_task: injected {} total tools (including built-ins), \
                                 {} / {} matched skills for native function calling (keywords: \
                                 {:?})",
                                native_tools.len(),
                                native_tools
                                    .len()
                                    .saturating_sub(self.builtin_workspace_tools().len()),
                                all_skills.len(),
                                keywords
                            );
                        }
                        Err(e) => warn!("Failed to serialize tools: {}", e),
                    }
                }
            }
        }
        if !native_tools.is_empty() && !extra_params.contains_key("tools_json") {
            match serde_json::to_string(&native_tools) {
                Ok(json) => {
                    extra_params.insert("tools_json".to_string(), json);
                    info!(
                        "handle_llm_task: injected {} built-in workspace tools for native \
                         function calling",
                        native_tools.len()
                    );
                }
                Err(e) => warn!("Failed to serialize built-in tools: {}", e),
            }
        }

        // 🟢 P2 FIX: Check LLM response cache for simple text queries (no images, < 500
        // chars)
        let cache_key =
            if native_tools.is_empty() && image_urls.is_empty() && input_text.len() < 500 {
                let memory_hash = gateway_memory_context
                    .as_ref()
                    .map(|m| m.len().to_string())
                    .unwrap_or_else(|| "0".to_string());
                Some(format!(
                    "{}|{}|{}",
                    self.config.id,
                    input_text.trim(),
                    memory_hash
                ))
            } else {
                None
            };

        if let Some(ref key) = cache_key {
            let cache = self.llm_response_cache.read().await;
            if let Some((cached_response, timestamp)) = cache.get(key) {
                if timestamp.elapsed() < Duration::from_secs(300) {
                    info!(
                        "P2 CACHE HIT: agent {} returning cached response for '{}' (age {:?})",
                        self.config.id,
                        input_text.chars().take(40).collect::<String>(),
                        timestamp.elapsed()
                    );
                    return Ok((cached_response.clone(), vec![]));
                }
            }
            drop(cache);
        }

        // 🆕 FIX: When native function calling is active (tools_json present), skip the
        // bulky text-based skill catalog and inject a strong command-style system hint.
        let messages = if extra_params.contains_key("tools_json") {
            let runtime_context = Self::runtime_context_prompt();
            let approval_tool_policy = if has_side_effect_execution_intent(&input_text) {
                "\n\nSide-effect requests must be handled through real tool calls. Do not write a \
                 confirmation card, risk warning, or ask the user to confirm in final text before \
                 a real tool call has created a runtime approval request. For MCP capabilities, \
                 first call mcp_tool_search for the matching MCP tool, then call the dynamically \
                 exposed MCP tool with the action parameters directly. MCP tool calls are \
                 temporarily allowed to run without runtime confirmation. If required parameters \
                 are missing, ask for those parameters directly; do not invent a fake approval \
                 flow."
            } else {
                ""
            };
            let mut result = vec![communication::Message::new(
                uuid::Uuid::new_v4(),
                communication::PlatformType::Custom,
                format!(
                    "You are a workspace-aware assistant. Use function calling when the user asks \
                     about files, code, commands, the current environment, or a task that clearly \
                     needs a tool. For ordinary questions that do not need tools, answer \
                     directly. When you do call a tool, call the most appropriate tool with \
                     correct parameters. If a parameter is missing, use a reasonable default or \
                     leave it empty.{}\n\n{}",
                    approval_tool_policy, runtime_context
                ),
            )];
            result.extend(messages);
            result
        } else {
            self.inject_skill_catalog(messages)
        };
        info!(
            "handle_llm_task: messages count after inject = {}, skill_catalog set = {}, \
             native_tools = {}",
            messages.len(),
            self.skill_catalog.is_some(),
            extra_params.contains_key("tools_json")
        );

        let mut response = if !native_tools.is_empty() && llm.supports_native_tools() {
            info!(
                "handle_llm_task: using native tool loop with {} tools",
                native_tools.len()
            );
            let mut loop_messages = messages.clone();
            let max_tool_rounds = crate::react_trace::clamp_react_max_tool_rounds(self.max_rounds);
            extra_params.insert("max_tool_rounds".to_string(), max_tool_rounds.to_string());
            let session_key = extra_params
                .get("session_id")
                .or_else(|| extra_params.get("channel_id"))
                .cloned()
                .unwrap_or_else(|| task.id.clone());
            let cancel_rx = Self::cancellation_receiver(&session_key, &extra_params).await;
            let run_id = Self::react_run_id(&task.id);
            extra_params.insert("agent_id".to_string(), self.config.id.clone());
            extra_params.insert("react_run_id".to_string(), run_id.clone());
            let mut final_text = String::new();
            let mut called_real_tool = false;

            self.emit_react_trace(
                crate::react_trace::ReActTraceEvent::new(
                    run_id.clone(),
                    session_key.clone(),
                    self.config.id.clone(),
                    0,
                    max_tool_rounds,
                    crate::react_trace::ReActTracePhase::LoopStarted,
                )
                .with_content_preview(&input_text),
            )
            .await;

            for round in 0..max_tool_rounds {
                let round_number = round + 1;
                if Self::is_cancelled(&cancel_rx) {
                    info!(
                        "handle_llm_task: session {} cancelled before native tool round {}",
                        session_key, round_number
                    );
                    final_text = "⏹️ 已停止当前任务。".to_string();
                    break;
                }
                extra_params.insert("react_round".to_string(), round_number.to_string());
                let turn = llm
                    .call_llm_tool_turn(
                        loop_messages.clone(),
                        native_tools.clone(),
                        Some(extra_params.clone()),
                    )
                    .await
                    .map_err(|e| {
                        AgentError::Execution(format!("LLM call with tools failed: {}", e))
                    })?;

                if turn.tool_calls.is_empty() {
                    if has_side_effect_execution_intent(&input_text)
                        && !called_real_tool
                        && looks_like_unbacked_approval_confirmation(&turn.content)
                        && round + 1 < max_tool_rounds
                    {
                        warn!(
                            "Native tool loop blocked unbacked approval confirmation; forcing \
                             tool call"
                        );
                        loop_messages.push(communication::Message::new(
                            uuid::Uuid::new_v4(),
                            communication::PlatformType::Custom,
                            "System: You just wrote a confirmation prompt for a side-effecting \
                             operation without creating a runtime approval request. That is not \
                             allowed. Use a real tool call for the requested action. For MCP \
                             actions, call mcp_tool_search first, then call the dynamically \
                             exposed MCP tool with the user's parameters. Do not ask for \
                             confirmation in final text."
                                .to_string(),
                        ));
                        continue;
                    }
                    final_text = turn.content;
                    self.emit_react_trace(
                        crate::react_trace::ReActTraceEvent::new(
                            run_id.clone(),
                            session_key.clone(),
                            self.config.id.clone(),
                            round_number,
                            max_tool_rounds,
                            crate::react_trace::ReActTracePhase::FinalAnswer,
                        )
                        .with_content_preview(&final_text),
                    )
                    .await;
                    break;
                }

                info!(
                    "handle_llm_task: round {} returned {} tool call(s)",
                    round_number,
                    turn.tool_calls.len()
                );
                self.emit_react_trace(
                    crate::react_trace::ReActTraceEvent::new(
                        run_id.clone(),
                        session_key.clone(),
                        self.config.id.clone(),
                        round_number,
                        max_tool_rounds,
                        crate::react_trace::ReActTracePhase::AssistantToolCall,
                    )
                    .with_content_preview(format!("{} tool call(s)", turn.tool_calls.len())),
                )
                .await;
                loop_messages.push(Self::assistant_tool_call_message(
                    &turn.tool_calls,
                    turn.reasoning_content.as_deref(),
                ));

                for tool_call in &turn.tool_calls {
                    if !Self::is_mcp_search_tool(&tool_call.function.name) {
                        called_real_tool = true;
                    }
                    if Self::is_cancelled(&cancel_rx) {
                        info!(
                            "handle_llm_task: session {} cancelled before native tool {}",
                            session_key, tool_call.function.name
                        );
                        final_text = "⏹️ 已停止当前任务。".to_string();
                        break;
                    }
                    self.emit_react_trace(
                        crate::react_trace::ReActTraceEvent::new(
                            run_id.clone(),
                            session_key.clone(),
                            self.config.id.clone(),
                            round_number,
                            max_tool_rounds,
                            crate::react_trace::ReActTracePhase::ToolStarted,
                        )
                        .with_tool(tool_call.function.name.clone(), tool_call.id.clone())
                        .with_arguments_preview(Self::tool_arguments_preview(tool_call))
                        .with_reasoning(turn.reasoning_content.as_deref().unwrap_or("")),
                    )
                    .await;
                    let output = match self
                        .execute_native_tool_call(
                            tool_call,
                            &tool_name_map,
                            &input_text,
                            &weather_data,
                        )
                        .await
                    {
                        Ok((output, success)) => {
                            self.emit_react_trace(
                                crate::react_trace::ReActTraceEvent::new(
                                    run_id.clone(),
                                    session_key.clone(),
                                    self.config.id.clone(),
                                    round_number,
                                    max_tool_rounds,
                                    if success {
                                        crate::react_trace::ReActTracePhase::ToolResult
                                    } else {
                                        crate::react_trace::ReActTracePhase::ToolError
                                    },
                                )
                                .with_tool(tool_call.function.name.clone(), tool_call.id.clone())
                                .with_content_preview(&output),
                            )
                            .await;
                            output
                        }
                        Err(e) => {
                            warn!("Native tool '{}' failed: {}", tool_call.function.name, e);
                            let output = Self::recoverable_tool_error(&tool_call.function.name, e);
                            self.emit_react_trace(
                                crate::react_trace::ReActTraceEvent::new(
                                    run_id.clone(),
                                    session_key.clone(),
                                    self.config.id.clone(),
                                    round_number,
                                    max_tool_rounds,
                                    crate::react_trace::ReActTracePhase::ToolError,
                                )
                                .with_tool(tool_call.function.name.clone(), tool_call.id.clone())
                                .with_content_preview(&output)
                                .error(),
                            )
                            .await;
                            output
                        }
                    };
                    loop_messages.push(Self::tool_result_message(&tool_call.id, output));

                    if Self::is_mcp_search_tool(&tool_call.function.name) {
                        if let Some(targets) = self
                            .mcp_targets_from_search_arguments(&tool_call.function.arguments)
                            .await
                        {
                            for target in targets {
                                match self.mcp_tool_definition(&target).await {
                                    Ok(definition) => {
                                        if !native_tools
                                            .iter()
                                            .any(|tool| tool.name == definition.name)
                                        {
                                            native_tools.push(definition);
                                        }
                                    }
                                    Err(e) => {
                                        warn!(
                                            "Failed to dynamically load MCP tool schema for \
                                             {}/{}: {}",
                                            target.server_name, target.tool_name, e
                                        );
                                    }
                                }
                            }
                            if let Ok(json) = serde_json::to_string(&native_tools) {
                                extra_params.insert("tools_json".to_string(), json);
                            }
                        }
                    }
                }
                if final_text == "⏹️ 已停止当前任务。" {
                    break;
                }
            }

            if final_text.trim().is_empty() {
                warn!(
                    "Native tool loop ended without final text after {} rounds",
                    max_tool_rounds
                );
                self.emit_react_trace(
                    crate::react_trace::ReActTraceEvent::new(
                        run_id.clone(),
                        session_key.clone(),
                        self.config.id.clone(),
                        max_tool_rounds,
                        max_tool_rounds,
                        crate::react_trace::ReActTracePhase::LoopLimitReached,
                    )
                    .with_content_preview(
                        "Maximum native tool rounds reached; requesting final answer.",
                    ),
                )
                .await;
                self.force_final_react_answer(
                    llm,
                    loop_messages,
                    native_tools.clone(),
                    extra_params.clone(),
                    max_tool_rounds,
                )
                .await
                .unwrap_or_else(|e| {
                    warn!("Forced final native-tool answer failed: {}", e);
                    "工具调用已完成，但模型没有生成最终回复。".to_string()
                })
            } else {
                final_text
            }
        } else {
            llm.call_llm(messages.clone(), Some(extra_params.clone()))
                .await
                .map_err(|e| AgentError::Execution(format!("LLM call failed: {}", e)))?
        };

        info!(
            "handle_llm_task: LLM raw response (first 200 chars) = {}",
            &response.chars().take(200).collect::<String>()
        );

        // 🆕 FIX: Guard against empty LLM responses to avoid corrupting conversation
        // history
        if response.trim().is_empty() {
            warn!("LLM returned empty response; skipping cache/history storage");
            return Ok((
                "抱歉，AI 暂时无法生成回复，请稍后再试。".to_string(),
                vec![],
            ));
        }

        // 🆕 FIX: Clean up thinking process BEFORE parsing skill triggers so that
        // trailing analysis text after SKILL: does not pollute skill ID extraction.
        let mut response = Self::cleanup_thinking_process(&response);

        // 🆕 OPTIMIZATION PHASE 3: Detect and execute tool chains (multi-step
        // conditional calls)
        if response.contains("STEP 1:") || response.contains("IF ") {
            match self.try_execute_tool_chain(&response, &input_text).await {
                Ok(Some(result)) => return Ok((result, vec![])),
                Ok(None) => {} // Not a valid tool chain, continue normal flow
                Err(e) => warn!("Tool chain execution failed: {}", e),
            }
        }

        // 🆕 FIX: If the LLM response is a skill trigger (e.g. "SKILL:hello-world"),
        // look up the skill in the registry and execute it instead of returning raw
        // text.
        let trimmed = response.trim();
        let skill_part = if trimmed.starts_with("SKILL:") {
            trimmed.strip_prefix("SKILL:")
        } else if let Some(pos) = trimmed.find("SKILL:") {
            trimmed[pos..].strip_prefix("SKILL:")
        } else {
            None
        };
        if let Some(skill_part) = skill_part {
            let skill_part = skill_part.trim();
            // 🆕 FIX: Parse skill ID and optional parameters separated by '|'
            let (skill_id, skill_params, json_parse_failed) = match skill_part.find('|') {
                Some(pos) => {
                    let id = skill_part[..pos].trim();
                    let params_json = skill_part[pos + 1..].trim();
                    let mut parse_failed = false;
                    let params = if params_json.is_empty() || params_json == "{}" {
                        None
                    } else {
                        match serde_json::from_str::<
                            std::collections::HashMap<String, serde_json::Value>,
                        >(params_json)
                        {
                            Ok(map) => {
                                let string_map: std::collections::HashMap<String, String> = map
                                    .into_iter()
                                    .map(|(k, v)| {
                                        let value = match v {
                                            serde_json::Value::String(s) => s,
                                            serde_json::Value::Bool(b) => b.to_string(),
                                            serde_json::Value::Number(n) => n.to_string(),
                                            other => other.to_string(),
                                        };
                                        (k, value)
                                    })
                                    .collect();
                                Some(string_map)
                            }
                            Err(e) => {
                                warn!(
                                    "LLM returned invalid JSON parameters for skill '{}': {} \
                                     (raw: {})",
                                    id, e, params_json
                                );
                                parse_failed = true;
                                None
                            }
                        }
                    };
                    (id, params, parse_failed)
                }
                None => {
                    // 🛡️ Fallback: Parse only the skill ID before any whitespace
                    let id = skill_part.split_whitespace().next().unwrap_or("").trim();
                    (id, None, false)
                }
            };

            // 🆕 FIX: Common words that LLM mistakenly uses as skill IDs when it
            // misinterprets system prompt
            let invalid_skill_ids = [
                "immediately",
                "format",
                "directly",
                "direct",
                "output",
                "skill",
                "id",
                "real",
                "actual",
                "<skill_id>",
            ];
            let is_invalid_id = invalid_skill_ids.contains(
                &skill_id
                    .to_lowercase()
                    .trim_matches(|c: char| !c.is_alphanumeric()),
            );
            // 🆕 FIX: Detect placeholder/example parameters that LLM copied from system
            // prompt
            let is_example_params = skill_params.as_ref().map_or(false, |p| {
                p.len() == 1 && p.get("param") == Some(&"value".to_string())
            });
            if skill_id.is_empty() {
                warn!(
                    "LLM returned empty skill ID after parsing: {}",
                    response.trim()
                );
            } else if is_invalid_id || is_example_params {
                warn!(
                    "LLM output invalid skill ID '{}' or example params {:?}. Falling back to \
                     normal LLM path.",
                    skill_id, skill_params
                );
                response = "抱歉，我没有理解您的具体需求。请告诉我您想做什么，我会尽力帮助您。"
                    .to_string();
            } else if json_parse_failed {
                // 🆕 FIX: LLM output SKILL:id|{incomplete_json — JSON parse failed.
                // Skip skill execution and fall back to normal LLM path so the LLM can retry or
                // answer directly.
                warn!(
                    "LLM returned SKILL:{} with invalid/incomplete JSON parameters. Falling back \
                     to normal LLM path.",
                    skill_id
                );
                response = format!(
                    "我注意到您可能需要使用 \
                     '{}'，但缺少必要的参数。请补充相关信息，我会立即帮您处理。",
                    skill_id
                );
                // Continue to normal LLM path below instead of executing skill
                // with missing params
            } else {
                info!(
                    "LLM requested skill execution: {} (params: {:?})",
                    skill_id, skill_params
                );
                if Self::is_builtin_workspace_tool(skill_id) {
                    match self
                        .execute_builtin_workspace_tool(
                            skill_id,
                            skill_params.as_ref(),
                            &input_text,
                        )
                        .await
                    {
                        Ok(output) => return Ok((output, vec![])),
                        Err(e) => {
                            warn!("Built-in workspace tool '{}' failed: {}", skill_id, e);
                            return Ok((format!("工具 '{}' 执行失败: {}", skill_id, e), vec![]));
                        }
                    }
                }
                if let Some(ref registry) = self.skill_registry {
                    let mut resolved_skill = registry.get(skill_id).await;
                    // 🆕 FIX: When using native function calling, the LLM sees normalized names
                    // (colons/slashes replaced by dashes). Map back to the original skill ID.
                    if resolved_skill.is_none() {
                        if let Some(original_id) = tool_name_map.get(skill_id) {
                            info!(
                                "Resolved normalized skill ID '{}' to original '{}'",
                                skill_id, original_id
                            );
                            resolved_skill = registry.get(original_id).await;
                        }
                    }
                    // 🆕 Fallback: if exact match fails, search for skill ID ending with
                    // /{skill_id} This handles cases where LLM returns just the
                    // tool name without mcp:server/ prefix
                    if resolved_skill.is_none()
                        && !skill_id.contains(':')
                        && !skill_id.contains('/')
                    {
                        let all_skills = registry.list_all().await;
                        for skill in &all_skills {
                            if skill.skill.id.ends_with(&format!("/{}", skill_id)) {
                                info!(
                                    "Resolved partial skill ID '{}' to full ID '{}'",
                                    skill_id, skill.skill.id
                                );
                                resolved_skill = Some(skill.clone());
                                break;
                            }
                        }
                    }
                    if let Some(registered) = resolved_skill {
                        let resolved_id = registered.skill.id.clone();
                        // 🆕 FIX: Pass parsed parameters to skill execution instead of always None
                        // 🆕 FIX: Enrich input with gateway-provided context (weather_data, etc.)
                        let enriched_input = if let Some(ref weather) = weather_data {
                            if !weather.is_empty() {
                                format!(
                                    "{}\n\n[参考数据] 实时天气：{}\n请基于以上数据回答。",
                                    input_text, weather
                                )
                            } else {
                                input_text.clone()
                            }
                        } else {
                            input_text.clone()
                        };
                        let skill_input = if skill_params.is_some() {
                            ""
                        } else {
                            enriched_input.as_str()
                        };
                        let mut skill_params = skill_params;
                        Self::apply_react_skill_param_defaults(
                            &resolved_id,
                            &enriched_input,
                            Some(&input_text),
                            &mut skill_params,
                        );
                        let skill_result = self
                            .execute_registered_skill(&registered, skill_input, skill_params)
                            .await;
                        match skill_result {
                            Ok(result) => {
                                let _ = registry.record_usage(&resolved_id).await;
                                let output = self.synthesize_skill_output(
                                    &input_text,
                                    &result.output,
                                    &resolved_id,
                                );
                                return Ok((output, vec![]));
                            }
                            Err(e) => {
                                let err_str = e.to_string().to_lowercase();
                                let is_dependency_failure = err_str.contains("mx_apikey")
                                    || err_str.contains("apikey")
                                    || err_str.contains("api key")
                                    || err_str.contains("environment variable")
                                    || err_str.contains("not set");
                                if is_dependency_failure {
                                    warn!(
                                        "Skill '{}' disabled due to missing dependency: {}",
                                        resolved_id, e
                                    );
                                    let _ = registry.disable(&resolved_id).await;
                                }
                                warn!(
                                    "Skill execution for '{}' failed: {}. Falling back to direct \
                                     answer.",
                                    resolved_id, e
                                );
                                return self.handle_direct_answer(task).await;
                            }
                        }
                    } else {
                        warn!(
                            "LLM requested unknown skill: '{}'. Falling back to direct answer.",
                            skill_id
                        );
                        return self.handle_direct_answer(task).await;
                    }
                }
            }
        }

        // 🟢 P2 FIX: Store response in cache
        if let Some(ref key) = cache_key {
            let mut cache = self.llm_response_cache.write().await;
            cache.insert(key.clone(), (response.clone(), Instant::now()));
            // Simple eviction: if cache grows beyond 100 entries, clear oldest half
            if cache.len() > 100 {
                let mut entries: Vec<_> = cache.drain().collect();
                entries.sort_by(|a, b| b.1 .1.cmp(&a.1 .1)); // newest first
                let keep = entries.len() / 2;
                for (k, v) in entries.into_iter().take(keep) {
                    cache.insert(k, v);
                }
            }
            drop(cache);
            info!(
                "P2 CACHE STORE: agent {} cached response for '{}'",
                self.config.id,
                input_text.chars().take(40).collect::<String>()
            );
        }

        Ok((response, vec![]))
    }

    /// Check whether a skill directory contains executable scripts.
    /// Checks both the root directory and the `scripts/` subdirectory.
    async fn has_scripts_in_dir(&self, dir: &std::path::Path) -> bool {
        // Helper to check a single directory for script files
        async fn check_dir_for_scripts(dir: &std::path::Path) -> bool {
            let mut entries = match tokio::fs::read_dir(dir).await {
                Ok(e) => e,
                Err(_) => return false,
            };
            while let Ok(Some(entry)) = entries.next_entry().await {
                if let Some(ext) = entry.path().extension().and_then(|e| e.to_str()) {
                    if matches!(ext, "py" | "js" | "sh" | "ts") {
                        return true;
                    }
                }
            }
            false
        }

        // 1. Check root directory
        if check_dir_for_scripts(dir).await {
            return true;
        }
        // 2. Check scripts/ subdirectory
        check_dir_for_scripts(&dir.join("scripts")).await
    }

    /// 🟢 P1 FIX: Public API to execute a skill by ID (used by composition,
    /// SkillCallTool, and external callers) 🆕 OPTIMIZATION PHASE 3:
    /// Integrated with skill feedback collection
    pub async fn execute_skill_by_id(
        &self,
        skill_id: &str,
        input: &str,
        parameters: Option<HashMap<String, String>>,
    ) -> Result<skills::executor::SkillExecutionResult, AgentError> {
        let registry = self
            .skill_registry
            .as_ref()
            .ok_or_else(|| AgentError::InvalidConfig("Skill registry not configured".into()))?;

        let registered_skill = registry
            .get(skill_id)
            .await
            .ok_or_else(|| AgentError::SkillNotFound(skill_id.to_string()))?;

        let result = self
            .execute_registered_skill(&registered_skill, input, parameters.clone())
            .await?;
        let _ = registry.record_usage(skill_id).await;

        // 🆕 OPTIMIZATION PHASE 3: Collect skill feedback for self-improvement
        self.collect_skill_feedback(
            skill_id,
            input,
            &result.output,
            result.success,
            result.execution_time_ms,
        )
        .await;

        Ok(result)
    }

    /// 🆕 OPTIMIZATION PHASE 3: Collect skill execution feedback
    async fn collect_skill_feedback(
        &self,
        skill_id: &str,
        input: &str,
        output: &str,
        success: bool,
        execution_time_ms: u64,
    ) {
        if let Some(ref collector) = self.skill_feedback_collector {
            let _feedback = collector.collect_feedback(skill_id, success, execution_time_ms);
            info!(
                "Skill feedback collected for '{}': success={}, time={}ms",
                skill_id, success, execution_time_ms
            );

            // If execution failed or took too long, log for improvement
            if !success || execution_time_ms > 10000 {
                let evaluation =
                    collector.build_evaluation_prompt(skill_id, input, output, success);
                info!(
                    "Skill '{}' needs attention: {} chars evaluation prompt ready",
                    skill_id,
                    evaluation.len()
                );
            }
        }
    }

    /// 🟢 P1 FIX: Internal helper for composition modules to call LLM with a
    /// simple prompt
    pub(crate) async fn call_llm_prompt(
        &self,
        prompt: impl Into<String>,
        system: Option<impl Into<String>>,
    ) -> Result<String, AgentError> {
        let llm = self
            .llm_interface
            .as_ref()
            .ok_or_else(|| AgentError::InvalidConfig("LLM interface not configured".into()))?;
        let mut messages: Vec<communication::Message> = Vec::new();
        if let Some(sys) = system {
            messages.push(communication::Message::new(
                uuid::Uuid::new_v4(),
                communication::PlatformType::Custom,
                sys.into(),
            ));
        }
        messages.push(communication::Message::new(
            uuid::Uuid::new_v4(),
            communication::PlatformType::Custom,
            prompt.into(),
        ));
        llm.call_llm(self.inject_skill_catalog(messages), None)
            .await
            .map_err(|e| AgentError::Execution(format!("LLM call failed: {}", e)))
    }

    /// 🆕 FIX: Synthesize structured skill output (JSON, etc.) into natural
    /// language. Uses dedicated templates for known skills to avoid extra
    /// LLM latency; falls back to generic JSON flattening for unknown
    /// skills.
    fn synthesize_skill_output(
        &self,
        _user_query: &str,
        raw_output: &str,
        skill_id: &str,
    ) -> String {
        let trimmed = raw_output.trim();
        let is_structured = trimmed.starts_with('{') || trimmed.starts_with('[');

        if !is_structured {
            return raw_output.to_string();
        }

        // 🆕 FIX: Format known MCP skills with dedicated templates for zero-latency
        // output
        if let Some(formatted) = format_known_skill_output(skill_id, raw_output) {
            return formatted;
        }

        // Generic JSON fallback: flatten to readable key-value list
        match format_generic_json(raw_output) {
            Some(text) => text,
            None => raw_output.to_string(),
        }
    }

    /// 🟢 P2 FIX: Judge a condition using LLM (used by LlmJudge in
    /// conditional/loop)
    pub(crate) async fn judge_condition(
        &self,
        prompt: &str,
        output: &str,
    ) -> Result<bool, AgentError> {
        let full_prompt = format!(
            "请根据以下条件判断给定的输出是否满足要求。\n\n条件: {}\n输出: \
             {}\n\n如果满足条件，只回答 'true'；如果不满足，只回答 'false'。不要解释。",
            prompt, output
        );
        let result = self
            .call_llm_prompt(
                full_prompt,
                Some::<String>(
                    "You are a strict conditional judge. Output only true or false.".into(),
                ),
            )
            .await?;
        let trimmed = result.trim().to_lowercase();
        Ok(trimmed.contains("true") || trimmed.starts_with("是") || trimmed.starts_with("yes"))
    }

    /// 🟢 P2 FIX: Helper to execute a registered skill (shared by
    /// handle_skill_task and planning) 🆕 OPTIMIZATION PHASE 1: Integrated
    /// with approval gate for destructive operations 🆕 OPTIMIZATION PHASE
    /// 4: Tool output truncation to prevent context overflow
    /// 🆕 OPTIMIZATION PHASE 4: Execute WASM skill in true sandbox with
    /// resource limits
    ///
    /// Creates a fresh WasmEngine with ResourceLimits-derived EngineConfig,
    /// ensuring memory/fuel/time constraints are enforced by wasmtime.
    async fn execute_wasm_in_sandbox(
        &self,
        wasm_path: &std::path::Path,
        entry_point: &str,
        input: &str,
        limits: &crate::security::ResourceLimits,
    ) -> Result<Option<skills::executor::SkillExecutionResult>, AgentError> {
        let start_time = std::time::Instant::now();

        let wasm_bytes = tokio::fs::read(wasm_path)
            .await
            .map_err(|e| AgentError::Execution(format!("Failed to read WASM file: {}", e)))?;

        // Build engine config from resource limits
        let engine_config = beebotos_kernel::wasm::EngineConfig {
            max_memory_size: limits.max_memory_mb * 1024 * 1024,
            max_fuel: limits.max_cpu_time_ms * 1000, // approximate fuel units from CPU time
            fuel_metering: true,
            memory_limits: true,
            wasi_enabled: false, // sandbox: disable WASI for untrusted skills
            debug_info: false,
            parallel_compilation: false,
            optimize: true,
        };

        let config_id = self.config.id.clone();
        let entry_point = entry_point.to_string();
        let input = input.to_string();
        let timeout_ms = limits.max_execution_time_secs.saturating_mul(1000);

        // 🟢 P0 FIX: Move the entire synchronous WASM execution chain into
        // tokio::task::spawn_blocking.  Wasmtime's call_typed() runs synchronously
        // and never yields to the Tokio runtime.  When a WASM guest enters an
        // infinite loop the tokio worker thread is pinned, which means outer
        // tokio::time::timeout / tokio::select! cancellation cannot fire promptly.
        // Running on a dedicated blocking thread allows the async runtime to stay
        // responsive so the task can be dropped and locks released.
        let blocking_task = tokio::task::spawn_blocking(move || {
            // Create sandboxed engine
            let engine = match beebotos_kernel::wasm::WasmEngine::new(engine_config) {
                Ok(e) => e,
                Err(e) => {
                    warn!("Failed to create sandboxed WASM engine: {}", e);
                    return Ok(None);
                }
            };

            // Compile WASM module
            let module = match engine.compile(&wasm_bytes) {
                Ok(m) => m,
                Err(e) => {
                    warn!("WASM compilation failed: {}", e);
                    return Ok(None);
                }
            };

            // Instantiate with host functions
            let mut instance = match engine.instantiate_with_host(&module, &config_id) {
                Ok(i) => i,
                Err(e) => {
                    warn!("WASM instantiation failed: {}", e);
                    return Ok(None);
                }
            };

            // Write input to WASM memory
            let input_bytes = input.as_bytes();
            if let Err(e) = instance.write_memory(0, input_bytes) {
                warn!("Failed to write input to WASM memory: {}", e);
                return Ok(None);
            }

            // Execute with fuel metering (CPU limit enforced by wasmtime engine config)
            const MAX_OUTPUT_SIZE: usize = 65536;
            let call_result = instance
                .call_typed::<(i32, i32), i32>(&entry_point, (0i32, input_bytes.len() as i32));

            match call_result {
                Ok(output_ptr) => {
                    let output_addr = output_ptr as usize;
                    // Read output length (first 4 bytes)
                    match instance.read_memory(output_addr, 4) {
                        Ok(len_bytes) => {
                            let output_len = u32::from_le_bytes([
                                len_bytes[0],
                                len_bytes[1],
                                len_bytes[2],
                                len_bytes[3],
                            ]) as usize;
                            if output_len <= MAX_OUTPUT_SIZE {
                                match instance.read_memory(output_addr + 4, output_len) {
                                    Ok(output_bytes) => {
                                        if let Ok(output) = String::from_utf8(output_bytes) {
                                            return Ok(Some(
                                                skills::executor::SkillExecutionResult {
                                                    task_id: entry_point,
                                                    success: true,
                                                    output,
                                                    structured_output: None,
                                                    execution_time_ms: start_time
                                                        .elapsed()
                                                        .as_millis()
                                                        as u64,
                                                },
                                            ));
                                        }
                                    }
                                    Err(e) => warn!("Failed to read WASM output: {}", e),
                                }
                            } else {
                                warn!("WASM output too large: {} bytes", output_len);
                            }
                        }
                        Err(e) => warn!("Failed to read WASM output length: {}", e),
                    }
                }
                Err(e) => warn!("WASM function call failed: {}", e),
            }

            Ok(None)
        });

        if timeout_ms > 0 {
            tokio::time::timeout(std::time::Duration::from_millis(timeout_ms), blocking_task)
                .await
                .map_err(|_| AgentError::Timeout("WASM sandbox execution timed out".to_string()))?
                .map_err(|e| AgentError::Execution(format!("WASM blocking task panicked: {}", e)))?
        } else {
            blocking_task
                .await
                .map_err(|e| AgentError::Execution(format!("WASM blocking task panicked: {}", e)))?
        }
    }

    async fn execute_registered_skill(
        &self,
        registered_skill: &skills::RegisteredSkill,
        input: &str,
        parameters: Option<HashMap<String, String>>,
    ) -> Result<skills::executor::SkillExecutionResult, AgentError> {
        let start_time = std::time::Instant::now();
        let skill_id = registered_skill.skill.id.clone();
        let parsed_mcp_skill: Option<(&str, &str)> = if skill_id.starts_with("mcp:") {
            let rest = &skill_id[4..];
            if let Some((server, tool)) = rest.split_once('/') {
                Some((server, tool))
            } else {
                return Ok(skills::executor::SkillExecutionResult {
                    task_id: skill_id.clone(),
                    success: false,
                    output: format!(
                        "Invalid MCP skill ID '{}'. Expected format: mcp:server/tool",
                        skill_id
                    ),
                    structured_output: None,
                    execution_time_ms: start_time.elapsed().as_millis() as u64,
                });
            }
        } else {
            None
        };

        // 🆕 OPTIMIZATION PHASE 1: Approval gate for destructive operations
        // 🆕 FIX (Plan C): Store pending approval for multi-step user confirmation
        if !self.skip_approval.load(std::sync::atomic::Ordering::SeqCst) {
            if let Some(ref gate) = self.approval_gate {
                let env = Self::approval_env();
                let params_json = parameters
                    .as_ref()
                    .map(|p| {
                        let mut map = serde_json::Map::new();
                        for (k, v) in p {
                            map.insert(k.clone(), serde_json::Value::String(v.clone()));
                        }
                        serde_json::Value::Object(map)
                    })
                    .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));

                match gate.evaluate(&skill_id, &params_json, &env) {
                    crate::security::ApprovalResult::Rejected { reason } => {
                        warn!(
                            "Approval required but not granted for skill '{}': {}",
                            skill_id, reason
                        );
                        let request = gate.build_request(&skill_id, &params_json, input);
                        let req_id = request.request_id.clone();
                        let description = request.description.clone();
                        let risk_level = request.risk_level;

                        // Store pending approval
                        {
                            let mut pending = self.pending_approvals.write().await;
                            pending.insert(req_id.clone(), request);
                            // Keep only the most recent 10 pending approvals
                            if pending.len() > 10 {
                                let oldest: Vec<String> = pending
                                    .keys()
                                    .cloned()
                                    .collect::<Vec<_>>()
                                    .into_iter()
                                    .take(pending.len() - 10)
                                    .collect();
                                for k in oldest {
                                    pending.remove(&k);
                                }
                            }
                        }

                        let risk_label = match risk_level {
                            crate::security::RiskLevel::Low => "🟢 低风险",
                            crate::security::RiskLevel::Medium => "🟡 中风险",
                            crate::security::RiskLevel::High => "🟠 高风险",
                            crate::security::RiskLevel::Critical => "🔴 关键操作",
                        };

                        return Ok(skills::executor::SkillExecutionResult {
                            task_id: skill_id,
                            success: false,
                            output: format!(
                                "{} {}\n\n{}\n\n⚠️ 这是一个高风险操作，需要您的确认后才能执行。\\
                                 n请回复「确认」或「同意」来执行此操作。",
                                risk_label, reason, description
                            ),
                            structured_output: None,
                            execution_time_ms: start_time.elapsed().as_millis() as u64,
                        });
                    }
                    crate::security::ApprovalResult::AutoApproved { rule } => {
                        info!("Skill '{}' auto-approved by rule: {}", skill_id, rule);
                    }
                    _ => {} // Approved or other states — proceed
                }
            }
        } // close skip_approval check

        // ── MCP Skill Bridge: Two-Stage Execution (Parameter Resolution → Confirmation
        // & Execution) ──
        if let Some((server_name, tool_name)) = parsed_mcp_skill {
            let mcp = self
                .mcp_manager
                .as_ref()
                .ok_or_else(|| AgentError::InvalidConfig("MCP manager not configured".into()))?;

            let client = mcp.get_client(server_name).await.ok_or_else(|| {
                AgentError::InvalidConfig(format!("MCP client '{}' not found", server_name))
            })?;

            // ===== STAGE 1: Parameter Resolution =====
            let mut arguments = serde_json::Map::new();
            if !input.is_empty() {
                match serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(input) {
                    Ok(map) => arguments = map,
                    Err(_) => {
                        arguments.insert(
                            "query".to_string(),
                            serde_json::Value::String(input.to_string()),
                        );
                    }
                }
            }
            if let Some(params) = parameters {
                for (k, v) in params {
                    if k != "skill" {
                        arguments.insert(k, serde_json::Value::String(v));
                    }
                }
            }

            // Fetch tool schema for validation and extraction
            let tools_result = client.list_tools(None).await.map_err(|e| {
                AgentError::Execution(format!("Failed to list tools for validation: {}", e))
            })?;
            let tool_opt = tools_result.tools.into_iter().find(|t| t.name == tool_name);
            let tool_schema = tool_opt.as_ref().map(|t| t.input_schema.clone());
            let tool_description = tool_opt
                .as_ref()
                .and_then(|t| t.description.clone())
                .unwrap_or_else(|| format!("MCP tool '{}'", tool_name));

            // Validate arguments; if incomplete, attempt LLM extraction
            let mut final_params = arguments.clone();
            Self::apply_mcp_default_params(&skill_id, &mut final_params);
            let needs_extraction = final_params.is_empty()
                || tool_schema.as_ref().map_or(false, |schema| {
                    crate::mcp::skill_bridge::validate_tool_arguments(schema, &final_params)
                        .is_err()
                });

            if needs_extraction {
                if let Some(ref llm) = self.llm_interface {
                    let extractor = skills::McpParameterExtractor::new(llm.clone());
                    match extractor
                        .extract(
                            input,
                            tool_schema.as_ref().unwrap_or(&serde_json::json!({})),
                            &skill_id,
                            &tool_description,
                        )
                        .await
                    {
                        Ok(skills::ExtractedParams::Complete(params)) => {
                            info!("MCP parameter extraction succeeded for '{}'", skill_id);
                            final_params = params;
                            Self::apply_mcp_default_params(&skill_id, &mut final_params);
                        }
                        Ok(skills::ExtractedParams::Partial { partial, missing }) => {
                            info!(
                                "MCP parameter extraction partial for '{}', missing: {:?}",
                                skill_id,
                                missing.iter().map(|f| &f.name).collect::<Vec<_>>()
                            );
                            let req_id = uuid::Uuid::new_v4().to_string();
                            let form = skills::PendingParameterForm::new(
                                req_id.clone(),
                                skill_id.clone(),
                                input.to_string(),
                                partial,
                                missing.clone(),
                            );
                            // Insert form and clean up in a single lock
                            {
                                let mut forms = self.pending_parameter_forms.write().await;
                                forms.insert(req_id.clone(), form);
                                // Remove expired forms; if still over limit, remove oldest
                                while forms.len() > 20 {
                                    let oldest = forms
                                        .iter()
                                        .min_by_key(|(_, f)| f.submitted_at)
                                        .map(|(k, _)| k.clone());
                                    if let Some(k) = oldest {
                                        forms.remove(&k);
                                    } else {
                                        break;
                                    }
                                }
                            }
                            let output = Self::render_parameter_form(
                                &req_id,
                                &missing,
                                &serde_json::Map::new(),
                            );
                            return Ok(skills::executor::SkillExecutionResult {
                                task_id: skill_id,
                                success: false,
                                output,
                                structured_output: None,
                                execution_time_ms: start_time.elapsed().as_millis() as u64,
                            });
                        }
                        Ok(skills::ExtractedParams::Unclear { reason }) => {
                            warn!(
                                "MCP parameter extraction unclear for '{}': {}",
                                skill_id, reason
                            );
                            return Ok(skills::executor::SkillExecutionResult {
                                task_id: skill_id.clone(),
                                success: false,
                                output: format!(
                                    "MCP parameter extraction was unclear.\nSkill: {}\nTool: \
                                     {}\nOriginal user request: {}\nReason: {}\n\nThis may mean \
                                     the selected tool is wrong for the user's intent, or the \
                                     user request lacks required fields. Retry with corrected \
                                     arguments if they can be inferred, choose a better matching \
                                     skill/tool if available, or ask one concise clarification \
                                     question.",
                                    skill_id, tool_name, input, reason
                                ),
                                structured_output: None,
                                execution_time_ms: start_time.elapsed().as_millis() as u64,
                            });
                        }
                        Err(e) => {
                            warn!("MCP parameter extraction failed for '{}': {}", skill_id, e);
                            return Ok(skills::executor::SkillExecutionResult {
                                task_id: skill_id.clone(),
                                success: false,
                                output: format!(
                                    "MCP parameter extraction failed.\nSkill: {}\nTool: \
                                     {}\nOriginal user request: {}\nError: {}\n\nTreat this as a \
                                     recoverable observation. Retry with corrected arguments, \
                                     select a more appropriate skill/tool, or ask the user for \
                                     the single missing detail needed.",
                                    skill_id, tool_name, input, e
                                ),
                                structured_output: None,
                                execution_time_ms: start_time.elapsed().as_millis() as u64,
                            });
                        }
                    }
                } else {
                    return Err(AgentError::InvalidConfig(
                        "LLM interface not configured for parameter extraction".into(),
                    ));
                }
            }

            // Final validation after extraction
            if let Some(ref schema) = tool_schema {
                if let Err(e) =
                    crate::mcp::skill_bridge::validate_tool_arguments(schema, &final_params)
                {
                    return Ok(skills::executor::SkillExecutionResult {
                        task_id: skill_id.clone(),
                        success: false,
                        output: format!(
                            "MCP parameter validation failed.\nSkill: {}\nTool: {}\nArguments: \
                             {}\nValidation error: {}\n\nTreat this as a recoverable observation. \
                             Retry with corrected arguments, choose a better matching skill/tool, \
                             or ask one concise clarification question.",
                            skill_id,
                            tool_name,
                            serde_json::Value::Object(final_params.clone()),
                            e
                        ),
                        structured_output: None,
                        execution_time_ms: start_time.elapsed().as_millis() as u64,
                    });
                }
            }

            // ===== STAGE 2: Confirmation & Execution =====
            let is_high_risk = Self::is_high_risk_mcp_skill(&skill_id);
            // 🆕 FIX: Skip approval check if skip_approval flag is set (e.g., Plan C
            // confirmed)
            if is_high_risk && !self.skip_approval.load(std::sync::atomic::Ordering::SeqCst) {
                let preview = Self::generate_action_preview(&skill_id, &tool_name, &final_params);
                let env = Self::approval_env();
                let params_json: serde_json::Value = final_params
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect::<serde_json::Map<String, serde_json::Value>>()
                    .into();

                if let Some(ref gate) = self.approval_gate {
                    match gate.evaluate(&skill_id, &params_json, &env) {
                        crate::security::ApprovalResult::Approved => {
                            // 🆕 FIX: Approved (no approval needed) — proceed directly
                            info!("MCP skill '{}' approved without confirmation", skill_id);
                        }
                        crate::security::ApprovalResult::AutoApproved { rule } => {
                            info!("MCP skill '{}' auto-approved by rule: {}", skill_id, rule);
                            // For paper trading, auto-approve is sufficient; for live trading,
                            // still show preview but mark as pre-approved.
                            if !rule.contains("paper") && !rule.contains("模拟") {
                                // Live trading: show preview for visibility
                                info!(
                                    "Live trading MCP skill '{}', showing preview before execution",
                                    skill_id
                                );
                            }
                        }
                        crate::security::ApprovalResult::Rejected { reason } => {
                            let request = gate.build_request(&skill_id, &params_json, input);
                            let req_id = request.request_id.clone();
                            {
                                let mut pending = self.pending_approvals.write().await;
                                pending.insert(req_id, request);
                            }

                            return Ok(skills::executor::SkillExecutionResult {
                                task_id: skill_id,
                                success: false,
                                output: format!(
                                    "{}\n\n{}\n\n⚠️ 这是一个高风险操作，需要您的确认后才能执行。\\
                                     n请回复「确认」或「同意」来执行此操作。",
                                    preview, reason
                                ),
                                structured_output: None,
                                execution_time_ms: start_time.elapsed().as_millis() as u64,
                            });
                        }
                        _ => {
                            // Needs confirmation: store pending approval with full params +
                            // original input
                            let request = gate.build_request(&skill_id, &params_json, input);
                            let req_id = request.request_id.clone();
                            {
                                let mut pending = self.pending_approvals.write().await;
                                pending.insert(req_id.clone(), request);
                            }
                            return Ok(skills::executor::SkillExecutionResult {
                                task_id: skill_id,
                                success: false,
                                output: format!(
                                    "{}\n\n⚠️ 这是一个高风险操作，需要您的确认后才能执行。\\
                                     n请回复「确认」或「同意」来执行此操作。",
                                    preview
                                ),
                                structured_output: None,
                                execution_time_ms: start_time.elapsed().as_millis() as u64,
                            });
                        }
                    }
                }
            }

            // Execute MCP tool call
            let args = if final_params.is_empty() {
                None
            } else {
                Some(final_params)
            };
            let result = client
                .call_tool(tool_name, args)
                .await
                .map_err(|e| AgentError::Execution(format!("MCP tool call failed: {}", e)))?;

            let output = if result.is_error {
                let error_text = result
                    .content
                    .iter()
                    .filter_map(|c| match c {
                        crate::mcp::types::ToolContent::Text { text } => Some(text.clone()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                let message = if error_text.is_empty() {
                    "unknown error".to_string()
                } else {
                    error_text
                };
                let output = format_mcp_call_error(&skill_id, &message);
                return Ok(skills::executor::SkillExecutionResult {
                    task_id: skill_id,
                    success: false,
                    output,
                    structured_output: None,
                    execution_time_ms: start_time.elapsed().as_millis() as u64,
                });
            } else {
                result
                    .content
                    .iter()
                    .filter_map(|c| match c {
                        crate::mcp::types::ToolContent::Text { text } => Some(text.clone()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            };

            // 🆕 OPTIMIZATION PHASE 4: Truncate large tool outputs to prevent context
            // overflow
            let truncated_output = Self::truncate_tool_output(&output, 4000);
            if truncated_output.len() < output.len() {
                info!(
                    "Tool output truncated from {} to {} chars for skill '{}'",
                    output.len(),
                    truncated_output.len(),
                    skill_id
                );
            }

            return Ok(skills::executor::SkillExecutionResult {
                task_id: skill_id,
                success: true,
                output: truncated_output,
                structured_output: None,
                execution_time_ms: start_time.elapsed().as_millis() as u64,
            });
        }

        // 🆕 FIX: Pass parameters to skill executor so ReAct knows the parsed args.
        let enriched_input = if parameters.as_ref().map_or(false, |p| !p.is_empty()) {
            let params_json = serde_json::to_string(&parameters).unwrap_or_default();
            if input.is_empty() {
                format!(
                    "[已解析参数] {}\n请使用这些参数执行相应的脚本。",
                    params_json
                )
            } else {
                format!(
                    "{}\n\n[已解析参数] {}\n请使用这些参数执行相应的脚本。",
                    input, params_json
                )
            }
        } else {
            input.to_string()
        };

        let context = skills::executor::SkillContext {
            input: input.to_string(),
            parameters: parameters.unwrap_or_default(),
        };

        // 🆕 FIX: Skip WASM attempt for markdown-based builtin skills that have no WASM
        // binary.
        let wasm_path_empty = registered_skill.skill.wasm_path.as_os_str().is_empty();

        // 1. Try WASM execution in true sandbox with resource limits
        if !wasm_path_empty {
            let limits = crate::security::ResourceLimits {
                max_memory_mb: 128,
                max_cpu_time_ms: 30000,
                max_execution_time_secs: 30,
                max_fs_usage_mb: 10,
                max_network_requests_per_min: 0,
            };
            match self
                .execute_wasm_in_sandbox(
                    &registered_skill.skill.wasm_path,
                    &registered_skill.skill.manifest.entry_point,
                    &context.input,
                    &limits,
                )
                .await
            {
                Ok(Some(result)) => return Ok(result),
                Ok(None) => {
                    info!(
                        "Skill '{}' WASM sandbox execution failed, falling back to LLM execution",
                        registered_skill.skill.name
                    );
                }
                Err(e) => {
                    warn!(
                        "Skill '{}' WASM sandbox error: {}",
                        registered_skill.skill.name, e
                    );
                }
            }
        }

        // 2. Knowledge / Code skill execution via ReAct executor
        // 🆕 SKILL MATCHING V2: Removed hardcoded generative skill exclusions.
        // All skills are treated uniformly; the Agent's LLM decides execution strategy.
        let source = &registered_skill.skill.source_path;
        if !source.as_os_str().is_empty() {
            if let Some(llm) = &self.llm_interface {
                let has_scripts = if source.is_dir() {
                    self.has_scripts_in_dir(source).await
                } else {
                    false
                };

                let result = if has_scripts {
                    info!(
                        "Executing code skill '{}' via ReAct with tools",
                        registered_skill.skill.name
                    );
                    let executor = skills::CodeSkillExecutor::new(llm.clone())
                        .with_mcp_manager(self.mcp_manager.clone());
                    executor.execute(source, &enriched_input).await
                } else {
                    info!(
                        "Executing knowledge skill '{}' via ReAct with tools",
                        registered_skill.skill.name
                    );
                    let executor = skills::KnowledgeSkillExecutor::new(llm.clone());
                    executor.execute(source, &enriched_input).await
                };

                return match result {
                    Ok(output) => Ok(skills::executor::SkillExecutionResult {
                        task_id: registered_skill.skill.id.clone(),
                        success: true,
                        output,
                        structured_output: None,
                        execution_time_ms: start_time.elapsed().as_millis() as u64,
                    }),
                    Err(e) => {
                        warn!(
                            "ReAct execution for skill '{}' failed: {}",
                            registered_skill.skill.name, e
                        );
                        Err(e)
                    }
                };
            }
        }

        // 3. Legacy LLM fallback (source_path empty or no llm_interface)
        info!(
            "Skill '{}' using legacy LLM fallback",
            registered_skill.skill.name
        );
        if let Some(llm) = &self.llm_interface {
            let manifest = &registered_skill.skill.manifest;
            let system_prompt = if !manifest.prompt_template.is_empty() {
                let mut prompt = manifest.prompt_template.clone();
                if !manifest.description.is_empty() && !prompt.contains(&manifest.description) {
                    prompt.push_str(&format!("\n\nAbout this skill: {}", manifest.description));
                }
                if !manifest.examples.is_empty() {
                    prompt.push_str(&format!("\n\nExamples:\n{}", manifest.examples));
                }
                prompt
            } else {
                format!(
                    "You are acting as the skill '{}'. {}\n\nSkill capabilities:\n{}\n\nExecute \
                     the following task using this skill persona.",
                    registered_skill.skill.name,
                    manifest.description,
                    manifest
                        .capabilities
                        .iter()
                        .map(|c| format!("- {}", c))
                        .collect::<Vec<_>>()
                        .join("\n")
                )
            };
            let messages = vec![
                communication::Message::new(
                    uuid::Uuid::new_v4(),
                    communication::PlatformType::Custom,
                    system_prompt,
                ),
                communication::Message::new(
                    uuid::Uuid::new_v4(),
                    communication::PlatformType::Custom,
                    context.input.clone(),
                ),
            ];

            match llm
                .call_llm(self.inject_skill_catalog(messages), None)
                .await
            {
                Ok(response) => {
                    return Ok(skills::executor::SkillExecutionResult {
                        task_id: registered_skill.skill.id.clone(),
                        success: true,
                        output: response,
                        structured_output: None,
                        execution_time_ms: start_time.elapsed().as_millis() as u64,
                    });
                }
                Err(e) => {
                    warn!(
                        "LLM fallback for skill '{}' also failed: {}",
                        registered_skill.skill.name, e
                    );
                }
            }
        }

        // Last resort: try legacy skill executor
        let executor = skills::SkillExecutor::new().map_err(|e| {
            AgentError::Execution(format!("Failed to create skill executor: {}", e))
        })?;
        executor
            .execute(&registered_skill.skill, context)
            .await
            .map_err(|e| AgentError::Execution(format!("Skill execution failed: {}", e)))
    }

    async fn handle_skill_task(&self, task: &Task) -> Result<(String, Vec<Artifact>), AgentError> {
        let skill_name = task
            .parameters
            .get("skill")
            .ok_or_else(|| AgentError::InvalidConfig("Missing 'skill' parameter".into()))?;

        let registry = self
            .skill_registry
            .as_ref()
            .ok_or_else(|| AgentError::InvalidConfig("Skill registry not configured".into()))?;

        let registered_skill = registry
            .get(skill_name)
            .await
            .ok_or_else(|| AgentError::SkillNotFound(skill_name.clone()))?;

        let result = self
            .execute_registered_skill(
                &registered_skill,
                &task.input,
                Some(task.parameters.clone()),
            )
            .await?;
        let execution_time_ms = result.execution_time_ms;

        let _ = registry.record_usage(skill_name).await;

        // 🆕 OPTIMIZATION PHASE 3: Collect feedback for self-improvement
        self.collect_skill_feedback(
            skill_name,
            &task.input,
            &result.output,
            result.success,
            execution_time_ms,
        )
        .await;

        let artifacts = if !result.output.is_empty() {
            vec![Artifact {
                id: uuid::Uuid::new_v4().to_string(),
                artifact_type: "skill_output".to_string(),
                content: result.output.clone().into_bytes(),
                mime_type: "text/plain".to_string(),
            }]
        } else {
            vec![]
        };

        Ok((
            format!(
                "Skill '{}' executed successfully in {}ms",
                skill_name, execution_time_ms
            ),
            artifacts,
        ))
    }

    async fn handle_mcp_task(&self, task: &Task) -> Result<(String, Vec<Artifact>), AgentError> {
        let mcp = self
            .mcp_manager
            .as_ref()
            .ok_or_else(|| AgentError::InvalidConfig("MCP manager not configured".into()))?;

        let tool_name = task
            .parameters
            .get("tool")
            .ok_or_else(|| AgentError::InvalidConfig("Missing 'tool' parameter".into()))?;

        let client = mcp
            .get_client("default")
            .await
            .ok_or_else(|| AgentError::InvalidConfig("MCP client not found".into()))?;

        let arguments: Option<serde_json::Map<String, serde_json::Value>> = if task.input.is_empty()
        {
            None
        } else {
            serde_json::from_str(&task.input)
                .map_err(|e| AgentError::InvalidConfig(format!("Invalid tool arguments: {}", e)))?
        };

        let result = client
            .call_tool(tool_name, arguments)
            .await
            .map_err(|e| AgentError::Execution(format!("MCP tool call failed: {}", e)))?;

        if result.is_error {
            let error_text = result
                .content
                .iter()
                .filter_map(|c| match c {
                    crate::mcp::types::ToolContent::Text { text } => Some(text.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n");
            return Err(AgentError::Execution(format!(
                "MCP tool returned an error: {}",
                if error_text.is_empty() {
                    "unknown error".to_string()
                } else {
                    error_text
                }
            )));
        }

        let output = result
            .content
            .iter()
            .filter_map(|c| match c {
                crate::mcp::types::ToolContent::Text { text } => Some(text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");

        Ok((output, vec![]))
    }

    async fn handle_file_task(&self, task: &Task) -> Result<(String, Vec<Artifact>), AgentError> {
        let file_path = task
            .parameters
            .get("file_path")
            .ok_or_else(|| AgentError::InvalidConfig("Missing 'file_path' parameter".into()))?;

        let content = tokio::fs::read_to_string(file_path)
            .await
            .map_err(|e| AgentError::Execution(format!("Failed to read file: {}", e)))?;

        let output = if task.input.is_empty() {
            format!(
                "File content ({} bytes): {}",
                content.len(),
                &content[..content.len().min(100)]
            )
        } else {
            let llm = self
                .llm_interface
                .as_ref()
                .ok_or_else(|| AgentError::InvalidConfig("LLM interface not configured".into()))?;

            let prompt = format!("{}\n\nFile content:\n{}", task.input, content);
            let messages = vec![communication::Message::new(
                uuid::Uuid::new_v4(),
                communication::PlatformType::Custom,
                prompt,
            )];

            llm.call_llm(self.inject_skill_catalog(messages), None)
                .await
                .map_err(|e| AgentError::Execution(format!("LLM processing failed: {}", e)))?
        };

        Ok((output, vec![]))
    }

    async fn handle_a2a_task(&self, task: &Task) -> Result<(String, Vec<Artifact>), AgentError> {
        let a2a = self
            .a2a_client
            .as_ref()
            .ok_or_else(|| AgentError::InvalidConfig("A2A client not configured".into()))?;

        let target_agent = task
            .parameters
            .get("target_agent")
            .ok_or_else(|| AgentError::InvalidConfig("Missing 'target_agent' parameter".into()))?;

        let mut params = HashMap::new();
        params.insert(
            "content".to_string(),
            serde_json::Value::String(task.input.clone()),
        );

        let payload = a2a::message::MessagePayload::Request {
            action: "send_message".to_string(),
            params,
        };

        let message = a2a::message::A2AMessage::new(
            a2a::message::MessageType::Request,
            types::AgentId::from_string(&self.config.id),
            Some(types::AgentId::from_string(target_agent)),
            payload,
        );

        let _response = a2a
            .send_message(message, target_agent)
            .await
            .map_err(|e| AgentError::A2A(format!("Failed to send A2A message: {}", e)))?;

        let output = format!("A2A message sent to {}. Response received.", target_agent);

        Ok((output, vec![]))
    }

    async fn handle_chain_transaction_task(
        &self,
        task: &Task,
    ) -> Result<(String, Vec<Artifact>), AgentError> {
        let wallet = self.wallet.as_ref().ok_or_else(|| {
            AgentError::InvalidConfig(
                "Wallet not configured. Use Agent::with_wallet() to enable chain transactions."
                    .into(),
            )
        })?;

        let to = task.parameters.get("to").ok_or_else(|| {
            AgentError::InvalidConfig("Missing 'to' parameter for chain transaction".into())
        })?;

        let value = task
            .parameters
            .get("value")
            .and_then(|v| v.parse::<u128>().ok())
            .unwrap_or(0);

        let data = task.parameters.get("data").cloned().unwrap_or_default();

        info!("Executing chain transaction: to={}, value={}", to, value);

        let tx_hash = wallet
            .send_transaction(
                to.parse()
                    .map_err(|_| AgentError::InvalidConfig("Invalid 'to' address".into()))?,
                value,
                if data.is_empty() {
                    None
                } else {
                    Some(data.into_bytes())
                },
            )
            .await
            .map_err(|e| AgentError::Execution(format!("Chain transaction failed: {}", e)))?;

        let output = format!("Transaction sent successfully. Hash: {:?}", tx_hash);

        let artifact = Artifact {
            id: uuid::Uuid::new_v4().to_string(),
            artifact_type: "transaction_receipt".to_string(),
            content: serde_json::json!({
                "tx_hash": format!("{:?}", tx_hash),
                "to": to,
                "value": value,
            })
            .to_string()
            .into_bytes(),
            mime_type: "application/json".to_string(),
        };

        Ok((output, vec![artifact]))
    }

    pub async fn shutdown(&mut self) -> Result<(), AgentError> {
        info!("Agent {} initiating graceful shutdown", self.config.id);
        *self.state.lock().unwrap() = state_manager::AgentState::ShuttingDown;

        if let Some(queue) = self.queue_manager.take() {
            info!("Shutting down queue manager...");
            queue.shutdown().await;
        }

        // Channels are disconnected globally; agent shutdown only cleans its own queue.

        if let Some(mcp) = self.mcp_manager.take() {
            info!("Closing MCP connections...");
            mcp.close_all().await;
        }

        self.a2a_client = None;
        self.skill_registry = None;
        self.llm_interface = None;

        info!("Agent {} shutdown complete", self.config.id);
        Ok(())
    }

    // ============================================================================
    // 🆕 PLANNING FIX: Planning Module Integration Methods
    // ============================================================================

    /// Analyze task complexity to determine execution strategy
    pub async fn analyze_task_complexity(&self, task: &Task) -> TaskComplexity {
        // Heuristic rules for complexity detection
        let is_complex = task.input.len() > 200 ||                              // Long description
            task.parameters.contains_key("multi_step") ||          // Explicit multi-step flag
            task.parameters.contains_key("dependencies") ||        // Has dependencies
            task.parameters.contains_key("plan") ||                // Explicit planning request
            matches!(task.task_type,
                TaskType::PlanCreation |
                TaskType::PlanExecution |
                TaskType::PlanAdaptation
            );

        if is_complex {
            TaskComplexity::Complex
        } else {
            TaskComplexity::Simple
        }
    }

    /// Determine if task should use planning
    pub async fn should_use_planning(&self, task: &Task) -> bool {
        if !self.is_planning_ready() {
            return false;
        }

        match self.analyze_task_complexity(task).await {
            TaskComplexity::Complex => true,
            TaskComplexity::Simple => {
                // Check for explicit planning override
                task.parameters
                    .get("use_planning")
                    .map(|v| v == "true" || v == "1")
                    .unwrap_or(false)
            }
        }
    }

    /// Execute task using planning
    /// 🆕 UNIFIED REACT: Replaces traditional P2 Planning with LLM
    /// self-decision. All planning tasks are now delegated to the LLM via
    /// either the investment-analysis ReAct loop (for crypto tasks) or
    /// native tool-calling (for general tasks).
    pub async fn execute_with_planning(
        &self,
        task: Task,
    ) -> Result<(String, Vec<Artifact>), AgentError> {
        let message_text = serde_json::from_str::<serde_json::Value>(&task.input)
            .ok()
            .and_then(|json| {
                json.get("message")
                    .and_then(|m| m.as_str())
                    .map(|s| s.to_string())
            })
            .unwrap_or_else(|| task.input.clone());

        info!(
            "🧠 Unified ReAct planning for task {} (legacy P2 Planning removed)",
            task.id
        );

        // General path: delegate to LLM with native tool-calling (single-round
        // autonomous skill selection). The LLM decides which tool to call based
        // on injected tool descriptions.
        // 🆕 FIX: Add _skip_planning marker to prevent recursive routing back to
        // execute_with_planning via handle_llm_task_internal's is_complex check.
        let mut task = task;
        task.parameters
            .insert("_skip_planning".to_string(), "true".to_string());
        let intent = crate::skill_matching::IntentAnalysisV2 {
            direct_answer: false,
            needs_skill: true,
            needs_planning: true,
            planning_strategy_hint: None,
            intent: crate::intent::UserIntent::MultiStepPlanning,
            entities: std::collections::HashMap::new(),
            constraints: Vec::new(),
            confidence: 1.0,
            query_summary: message_text.clone(),
            active_toolsets: vec![],
        };
        let selection = crate::skill_matching::SkillSelection {
            selected_skill: None,
            selected_skill_name: None,
            needs_planning: true,
            confidence: 0.0,
            scores: Vec::new(),
            selection_reasoning: "Unified ReAct general planning".to_string(),
            disclosure_level: crate::skills::registry::SkillDisclosureLevel::L0,
        };
        // 🆕 FIX: Use Box::pin to break the recursive async fn chain
        // (execute_with_planning → handle_llm_task_v2 → handle_llm_task_internal
        // → execute_with_planning).
        Box::pin(self.handle_llm_task_v2(&task, &intent, &selection)).await
    }

    /// Create plan context from task
    /// 🆕 OPTIMIZATION PHASE 3: Injects historical solutions from memory
    async fn create_plan_context(&self, task: &Task) -> Result<PlanContext, AgentError> {
        let mut context = PlanContext::new(&self.config.id);

        // Add available tools
        let tools = self.get_available_tools().await;
        context.available_tools = tools;

        // Add session info if present
        if let Some(session_id) = task.parameters.get("session_id") {
            context.session_id = Some(session_id.clone());
        }

        // Add constraints from task parameters
        if let Some(constraints) = task.parameters.get("constraints") {
            context.constraints = constraints
                .split(',')
                .map(|s| s.trim().to_string())
                .collect();
        }

        // 🆕 OPTIMIZATION PHASE 3: Inject historical solutions from memory
        if let Some(ref memory) = self.memory_system {
            let query = if let Ok(json) = serde_json::from_str::<serde_json::Value>(&task.input) {
                json.get("message")
                    .and_then(|m| m.as_str())
                    .unwrap_or(&task.input)
                    .to_string()
            } else {
                task.input.clone()
            };

            match memory.search(&query).await {
                Ok(results) => {
                    let historical: Vec<String> =
                        results.into_iter().take(3).map(|r| r.content).collect();
                    if !historical.is_empty() {
                        info!(
                            "Injected {} historical solutions into plan context",
                            historical.len()
                        );
                        context.history = historical;
                    }
                }
                Err(e) => {
                    warn!("Failed to search memory for planning context: {}", e);
                }
            }
        }

        Ok(context)
    }

    /// 🆕 OPTIMIZATION PHASE 3: Solidify successful plan execution into
    /// long-term memory
    ///
    /// Stores the plan + execution trail as a reusable experience entry.
    /// Future similar queries can retrieve this experience via memory search.
    async fn solidify_experience(
        &self,
        goal: &str,
        plan: &Plan,
        trail: &crate::planning::ToolTrail,
        output: &str,
    ) {
        if self.memory_system.is_none() {
            return;
        }

        // Generate experience summary
        let steps_markdown: String = plan
            .steps
            .iter()
            .enumerate()
            .map(|(i, s)| format!("{}. {}\n", i + 1, s.description))
            .collect();

        let key_tools: Vec<String> = trail
            .steps
            .iter()
            .filter_map(|s| s.tool_calls.first().map(|tc| tc.tool_name.clone()))
            .collect();

        let experience = format!(
            "## 问题类型: 规划执行\n\n### 用户目标\n{}\n\n### 执行步骤\n{}\n\n### \
             关键工具\n{}\n\n### 执行结果\n{}\n\n### 计划ID\n{}",
            goal,
            steps_markdown,
            key_tools.join(", "),
            output.chars().take(500).collect::<String>(),
            plan.id
        );

        let memory = self.memory_system.as_ref().unwrap();
        let mut metadata = std::collections::HashMap::new();
        metadata.insert("category".to_string(), "solution".to_string());
        metadata.insert("plan_id".to_string(), plan.id.to_string());
        metadata.insert("user_id".to_string(), self.config.id.clone());
        metadata.insert("step_count".to_string(), plan.steps.len().to_string());
        metadata.insert("source".to_string(), "solidified_experience".to_string());

        if let Err(e) = memory
            .add_entry(uuid::Uuid::new_v4(), &experience, metadata)
            .await
        {
            warn!("Failed to solidify experience to memory: {}", e);
        } else {
            info!("Solidified experience for plan {} into memory", plan.id);
        }
    }

    /// 🆕 PHASE 2: Auto-distill skill from successful execution trail
    async fn maybe_distill_skill(
        &self,
        trail: &crate::planning::ToolTrail,
        _goal: &str,
    ) -> Result<(), AgentError> {
        let distiller = SkillDistiller::new(DistillerConfig::default());

        // Count total tool calls
        let tool_call_count: usize = trail.steps.iter().map(|s| s.tool_calls.len()).sum();
        let trigger = DistillTrigger::ToolCallThreshold {
            count: tool_call_count,
        };

        if !distiller.should_distill(trail, &trigger) {
            return Ok(());
        }

        // Distill skill from trail
        let distilled = distiller
            .distill(trail)
            .map_err(|e| AgentError::Execution(format!("Distillation failed: {}", e)))?;

        // Quality gate
        if distilled.quality_score < distiller.config.min_quality_score {
            info!(
                "Distilled skill '{}' quality {:.1} below threshold {}, skipping",
                distilled.skill_id, distilled.quality_score, distiller.config.min_quality_score
            );
            return Ok(());
        }

        // Check for duplicates / patches against existing registry
        let existing = if let Some(registry) = &self.skill_registry {
            registry.list_all().await
        } else {
            vec![]
        };

        let decision = distiller.compare_to_existing(&distilled, &existing);

        match decision {
            crate::evolution::skill_distiller::DistillDecision::CreateNew => {
                let skill_dir = std::path::PathBuf::from("data/skills/auto_distilled");
                tokio::fs::create_dir_all(&skill_dir).await.map_err(|e| {
                    AgentError::Execution(format!("Failed to create skill dir: {}", e))
                })?;

                let skill_file = skill_dir.join(format!("{}.md", distilled.skill_id));
                let markdown = distiller.to_skill_markdown(&distilled);
                tokio::fs::write(&skill_file, &markdown)
                    .await
                    .map_err(|e| {
                        AgentError::Execution(format!("Failed to write skill file: {}", e))
                    })?;

                // Register with skill registry
                if let Some(registry) = &self.skill_registry {
                    let loaded_skill = LoadedSkill {
                        id: distilled.skill_id.clone(),
                        name: distilled.name.clone(),
                        version: Version {
                            major: 1,
                            minor: 0,
                            patch: 0,
                        },
                        wasm_path: std::path::PathBuf::new(),
                        source_path: skill_file.clone(),
                        manifest: SkillManifest {
                            id: distilled.skill_id.clone(),
                            name: distilled.name.clone(),
                            version: Version {
                                major: 1,
                                minor: 0,
                                patch: 0,
                            },
                            description: distilled.description.clone(),
                            author: "auto_distiller".to_string(),
                            capabilities: vec!["workflow".to_string()],
                            permissions: vec![],
                            entry_point: "execute".to_string(),
                            license: "".to_string(),
                            functions: vec![],
                            prompt_template: markdown.clone(),
                            examples: "".to_string(),
                            when_to_use: distilled.description.clone(),
                            when_not_to_use: None,
                            activation_examples: vec![],
                            activation_negative_examples: vec![],
                            dependencies: vec![],
                            ..Default::default()
                        },
                    };

                    registry
                        .register(loaded_skill, "auto_distilled", vec!["auto".to_string()])
                        .await;
                    info!(
                        "🆕 Auto-distilled skill '{}' registered (quality: {:.1})",
                        distilled.skill_id, distilled.quality_score
                    );
                }
            }
            crate::evolution::skill_distiller::DistillDecision::PatchExisting { skill_id } => {
                info!(
                    "Distilled skill '{}' is a patch of existing '{}', deferred to patch engine",
                    distilled.skill_id, skill_id
                );
                // TODO: Apply patch via PatchEngine when patch content is
                // available
            }
            crate::evolution::skill_distiller::DistillDecision::UpdateExisting { skill_id } => {
                info!(
                    "Distilled skill '{}' is an update of existing '{}', deferred to update flow",
                    distilled.skill_id, skill_id
                );
                // TODO: Trigger CAPO or manual update flow
            }
        }

        Ok(())
    }

    /// Get available tools for planning
    async fn get_available_tools(&self) -> Vec<String> {
        let mut tools = vec![];

        // Add LLM if available
        if self.llm_interface.is_some() {
            tools.push("llm".to_string());
        }

        // Add skills
        if self.skill_registry.is_some() {
            tools.push("skill".to_string());
        }

        // Add MCP tools
        if self.mcp_manager.is_some() {
            tools.push("mcp".to_string());
        }

        tools
    }

    /// Select planning strategy based on task
    pub fn select_plan_strategy(&self, task: &Task) -> PlanStrategy {
        if let Some(strategy_str) = task.parameters.get("strategy") {
            match strategy_str.as_str() {
                "react" => PlanStrategy::ReAct,
                "chain_of_thought" | "cot" => PlanStrategy::ChainOfThought,
                "goal_based" => PlanStrategy::GoalBased,
                "hybrid" => PlanStrategy::Hybrid,
                _ => PlanStrategy::Hybrid,
            }
        } else {
            PlanStrategy::Hybrid
        }
    }

    /// Execute plan with step handlers
    ///
    /// 🆕 OPTIMIZATION: Supports both sequential and parallel execution
    /// 🆕 OPTIMIZATION: Respects step dependencies when executing in parallel
    /// 🆕 FIX: Hard cap on step count to prevent LLM storm and timeout
    async fn execute_plan_internal(&self, plan: &Plan) -> Result<ExecutionResult, AgentError> {
        self.execute_plan_internal_with_trail(plan, None).await
    }

    /// 🆕 OPTIMIZATION PHASE 3: Execute plan with ToolTrail tracking
    async fn execute_plan_internal_with_trail(
        &self,
        plan: &Plan,
        trail: Option<&mut crate::planning::ToolTrail>,
    ) -> Result<ExecutionResult, AgentError> {
        const MAX_PLAN_STEPS: usize = 5;

        if plan.steps.len() > MAX_PLAN_STEPS {
            warn!(
                "Plan {} has {} steps, exceeding max {}. Truncating to first {} steps.",
                plan.id,
                plan.steps.len(),
                MAX_PLAN_STEPS,
                MAX_PLAN_STEPS
            );
            let mut truncated_plan = plan.clone();
            truncated_plan.steps.truncate(MAX_PLAN_STEPS);
            truncated_plan.dependencies.clear();
            for i in 1..truncated_plan.steps.len() {
                let _ =
                    truncated_plan.add_step_with_deps(truncated_plan.steps[i].clone(), vec![i - 1]);
            }
            return self
                .execute_plan_sequential_or_dependency_aware_with_trail(&truncated_plan, trail)
                .await;
        }

        let enable_parallel = plan
            .metadata
            .get("enable_parallel")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let max_concurrency = plan
            .metadata
            .get("max_concurrency")
            .and_then(|v| v.as_u64())
            .unwrap_or(3) as usize;

        let has_simple_chain_deps = plan.dependencies.len() + 1 == plan.steps.len()
            && plan
                .dependencies
                .iter()
                .all(|(k, v)| v.len() == 1 && v[0] + 1 == *k);

        if enable_parallel && plan.dependencies.is_empty() {
            self.execute_plan_parallel_with_trail(plan, max_concurrency, trail)
                .await
        } else if enable_parallel && has_simple_chain_deps {
            info!(
                "Plan {} has chain dependencies ({} steps), executing sequentially to avoid LLM \
                 waste",
                plan.id,
                plan.steps.len()
            );
            self.execute_plan_sequential_or_dependency_aware_with_trail(plan, trail)
                .await
        } else {
            self.execute_plan_sequential_or_dependency_aware_with_trail(plan, trail)
                .await
        }
    }

    /// Execute plan steps in parallel
    #[allow(dead_code)]
    async fn execute_plan_parallel(
        &self,
        plan: &Plan,
        max_concurrency: usize,
    ) -> Result<ExecutionResult, AgentError> {
        self.execute_plan_parallel_with_trail(plan, max_concurrency, None)
            .await
    }

    /// 🆕 OPTIMIZATION PHASE 3: Execute plan steps in parallel with ToolTrail
    async fn execute_plan_parallel_with_trail(
        &self,
        plan: &Plan,
        max_concurrency: usize,
        _trail: Option<&mut crate::planning::ToolTrail>,
    ) -> Result<ExecutionResult, AgentError> {
        let start_time = std::time::Instant::now();
        use futures::future::join_all;
        use tokio::sync::Semaphore;

        let semaphore = Arc::new(Semaphore::new(max_concurrency));

        // Create futures for all steps
        let mut futures = Vec::new();
        for (step_idx, step) in plan.steps.iter().enumerate() {
            let step = step.clone();
            let semaphore = semaphore.clone();

            futures.push(async move {
                let _permit = semaphore.acquire().await.unwrap();
                let result = self.execute_step_by_type(&step).await;
                (step_idx, result)
            });
        }

        // Execute all futures concurrently
        let results = join_all(futures).await;

        // Check if all succeeded
        let all_success = results.iter().all(|(_, r)| matches!(r, Ok(r) if r.success));
        let any_failed = results
            .iter()
            .any(|(_, r)| matches!(r, Ok(r) if !r.success));

        // Collect outputs from successful steps, and find first failure if needed
        let mut final_output = "Plan executed successfully".to_string();
        for (_, result) in &results {
            match result {
                Ok(exec_result) => {
                    if exec_result.success {
                        if let Some(data) = &exec_result.data {
                            if let Some(output) = data.get("output").and_then(|o| o.as_str()) {
                                final_output = output.to_string();
                            }
                        }
                    } else if any_failed && !self.should_continue_on_failure(plan) {
                        return Ok(exec_result.clone());
                    }
                }
                Err(_) => {}
            }
        }

        Ok(ExecutionResult {
            success: all_success,
            data: Some(serde_json::json!({ "output": final_output })),
            error: None,
            duration_ms: start_time.elapsed().as_millis() as u64,
            attempts: 1,
        })
    }

    /// Execute plan steps sequentially or with dependency awareness
    #[allow(dead_code)]
    async fn execute_plan_sequential_or_dependency_aware(
        &self,
        plan: &Plan,
    ) -> Result<ExecutionResult, AgentError> {
        self.execute_plan_sequential_or_dependency_aware_with_trail(plan, None)
            .await
    }

    /// 🆕 OPTIMIZATION PHASE 3: Execute plan steps sequentially with ToolTrail
    async fn execute_plan_sequential_or_dependency_aware_with_trail(
        &self,
        plan: &Plan,
        mut trail: Option<&mut crate::planning::ToolTrail>,
    ) -> Result<ExecutionResult, AgentError> {
        let start_time = std::time::Instant::now();

        // If no dependencies, execute sequentially
        if plan.dependencies.is_empty() {
            let mut final_output = "Plan executed successfully".to_string();
            for (step_idx, step) in plan.steps.iter().enumerate() {
                info!(
                    "Executing plan {} step {}: {}",
                    plan.id, step_idx, step.description
                );

                if let Some(ref mut t) = trail {
                    t.set_step_status(step_idx, crate::planning::TrailStepStatus::Running);
                }

                let step_start = std::time::Instant::now();
                let step_result = self
                    .execute_step_by_type_with_trail(step, step_idx, trail.as_deref_mut())
                    .await;
                let step_duration = step_start.elapsed().as_millis() as u64;

                if let Some(ref mut t) = trail {
                    t.set_step_duration(step_idx, step_duration);
                }

                match step_result {
                    Ok(result) => {
                        if result.success {
                            if let Some(data) = &result.data {
                                if let Some(output) = data.get("output").and_then(|o| o.as_str()) {
                                    final_output = output.to_string();
                                }
                            }
                            if let Some(ref mut t) = trail {
                                t.set_step_status(
                                    step_idx,
                                    crate::planning::TrailStepStatus::Success,
                                );
                            }
                        } else if !self.should_continue_on_failure(plan) {
                            if let Some(ref mut t) = trail {
                                t.set_step_status(
                                    step_idx,
                                    crate::planning::TrailStepStatus::Failed,
                                );
                            }
                            return Ok(result);
                        }
                    }
                    Err(e) => {
                        error!("Step {} execution failed: {}", step_idx, e);
                        if let Some(ref mut t) = trail {
                            t.set_step_status(step_idx, crate::planning::TrailStepStatus::Failed);
                        }
                        return Err(e);
                    }
                }
            }

            Ok(ExecutionResult {
                success: true,
                data: Some(serde_json::json!({ "output": final_output })),
                error: None,
                duration_ms: start_time.elapsed().as_millis() as u64,
                attempts: 1,
            })
        } else {
            // Dependency-aware execution
            return self.execute_plan_with_dependencies(plan).await;
        }
    }

    /// Execute step based on its type
    async fn execute_step_by_type(&self, step: &PlanStep) -> Result<ExecutionResult, AgentError> {
        self.execute_step_by_type_with_trail(step, 0, None).await
    }

    /// 🆕 OPTIMIZATION PHASE 3: Execute step with ToolTrail tracking
    async fn execute_step_by_type_with_trail(
        &self,
        step: &PlanStep,
        step_idx: usize,
        mut trail: Option<&mut crate::planning::ToolTrail>,
    ) -> Result<ExecutionResult, AgentError> {
        let result = match step.step_type {
            StepType::Action => self.execute_action_step(step).await,
            StepType::Decision => self.execute_decision_step(step).await,
            StepType::Reasoning => self.execute_reasoning_step(step).await,
            StepType::Information => self.execute_information_step(step).await,
            StepType::Validation => self.execute_validation_step(step).await,
        };

        // Record result in ToolTrail
        if let Some(ref mut t) = trail {
            match &result {
                Ok(exec_result) => {
                    let success = exec_result.success;
                    let output = exec_result
                        .data
                        .as_ref()
                        .and_then(|d| d.get("output"))
                        .and_then(|o| o.as_str())
                        .unwrap_or("")
                        .to_string();
                    t.record_tool_call(
                        step_idx,
                        format!("{:?}", step.step_type),
                        serde_json::Value::Null,
                        &output,
                        success,
                    );
                }
                Err(e) => {
                    t.record_tool_call(
                        step_idx,
                        format!("{:?}", step.step_type),
                        serde_json::Value::Null,
                        &e.to_string(),
                        false,
                    );
                }
            }
        }

        result
    }

    /// Execute plan with dependency awareness
    ///
    /// 🆕 OPTIMIZATION: Executes steps in waves based on dependencies
    async fn execute_plan_with_dependencies(
        &self,
        plan: &Plan,
    ) -> Result<ExecutionResult, AgentError> {
        let start_time = std::time::Instant::now();

        let mut completed: std::collections::HashSet<usize> = std::collections::HashSet::new();
        let total_steps = plan.steps.len();
        let mut last_output = "Plan executed successfully".to_string();

        while completed.len() < total_steps {
            // Find steps that are ready (all dependencies completed)
            let ready: Vec<usize> = plan
                .steps
                .iter()
                .enumerate()
                .filter(|(i, _)| !completed.contains(i))
                .filter(|(i, _)| {
                    plan.dependencies
                        .get(i)
                        .map(|deps| deps.iter().all(|d| completed.contains(d)))
                        .unwrap_or(true)
                })
                .map(|(i, _)| i)
                .collect();

            if ready.is_empty() {
                // Deadlock detected
                return Err(AgentError::Planning(
                    "Deadlock detected in plan dependencies".to_string(),
                ));
            }

            // Execute ready steps in parallel using join_all
            use futures::future::join_all;

            let futures: Vec<_> = ready
                .into_iter()
                .map(|step_idx| {
                    let step = plan.steps[step_idx].clone();
                    async move {
                        let result = self.execute_step_by_type(&step).await;
                        (step_idx, result)
                    }
                })
                .collect();

            let results = join_all(futures).await;

            // Process results
            for (step_idx, result) in results {
                match result {
                    Ok(exec_result) => {
                        if exec_result.success {
                            completed.insert(step_idx);
                            if let Some(data) = &exec_result.data {
                                if let Some(output) = data.get("output").and_then(|o| o.as_str()) {
                                    last_output = output.to_string();
                                }
                            }
                        } else if !self.should_continue_on_failure(plan) {
                            return Ok(exec_result);
                        }
                    }
                    Err(e) => return Err(e),
                }
            }
        }

        Ok(ExecutionResult {
            success: true,
            data: Some(serde_json::json!({ "output": last_output })),
            error: None,
            duration_ms: start_time.elapsed().as_millis() as u64,
            attempts: 1,
        })
    }

    /// 🆕 FIX: Smart skill search for plan steps using keyword domain mapping.
    /// Maps step descriptions to relevant skills based on semantic keyword
    /// overlap rather than simple string containment.
    async fn search_skills_for_step(
        &self,
        registry: &Arc<skills::SkillRegistry>,
        step_description: &str,
    ) -> Vec<skills::RegisteredSkill> {
        let desc_lower = step_description.to_lowercase();

        // Domain keyword → skill name/tag mappings
        let domain_keywords: &[(&[&str], &str)] = &[
            (
                &[
                    "travel",
                    "tour",
                    "trip",
                    "itinerary",
                    "旅游",
                    "旅行",
                    "行程",
                    "攻略",
                    "景点",
                    "酒店",
                ],
                "travel_planner",
            ),
            (
                &[
                    "code", "program", "develop", "debug", "coding", "编程", "代码", "开发",
                ],
                "python_developer",
            ),
            (&["code", "rust", "cargo", "编程", "代码"], "rust_developer"),
            (
                &["contract", "solidity", "smart contract", "合约", "区块链"],
                "solidity_developer",
            ),
            (&["write", "email", "draft", "邮件", "写信"], "email_writer"),
            (
                &["story", "novel", "fiction", "write", "故事", "小说"],
                "story_writer",
            ),
            (&["game", "gaming", "游戏", "玩家"], "game_master"),
            (
                &["data", "analyze", "analysis", "数据", "分析", "统计"],
                "data_analyst",
            ),
            (
                &["image", "photo", "picture", "图", "照片"],
                "image_analyst",
            ),
            (
                &["calendar", "schedule", "meeting", "日历", "会议", "安排"],
                "calendar_assistant",
            ),
            (&["task", "todo", "plan", "任务", "待办"], "task_manager"),
            (
                &["defi", "yield", "liquidity", "farm", "挖矿", "流动性"],
                "yield_farmer",
            ),
            (&["nft", "mint", "token", "数字藏品"], "nft_minter"),
            (
                &["health", "medical", "doctor", "健康", "医疗", "医生"],
                "health_advisor",
            ),
            (
                &["learn", "study", "tutor", "lesson", "学习", "课程", "辅导"],
                "tutor",
            ),
            (
                &["research", "paper", "survey", "研究", "论文", "调查"],
                "code_researcher",
            ),
            (
                &[
                    "dao",
                    "governance",
                    "proposal",
                    "vote",
                    "治理",
                    "提案",
                    "投票",
                ],
                "governance_analyst",
            ),
            (
                &["finance", "portfolio", "invest", "理财", "投资", "组合"],
                "portfolio_manager",
            ),
            (
                &["social", "community", "content", "社媒", "社群", "内容"],
                "content_creator",
            ),
            (
                &["security", "audit", "vulnerability", "安全", "审计", "漏洞"],
                "auditor",
            ),
            (
                &[
                    "crypto",
                    "cryptocurrency",
                    "比特币",
                    "btc",
                    "eth",
                    "以太坊",
                    "crypto order",
                    "加密货币",
                ],
                "mcp:alpaca/place_crypto_order",
            ),
            (
                &[
                    "stock",
                    "股票",
                    "aapl",
                    "tsla",
                    "equity",
                    "shares",
                    "stock order",
                ],
                "mcp:alpaca/place_stock_order",
            ),
            (
                &[
                    "order",
                    "下单",
                    "购买",
                    "买入",
                    "卖出",
                    "buy",
                    "sell",
                    "place order",
                    "trade",
                ],
                "mcp:alpaca/place_crypto_order",
            ),
            (
                &[
                    "crypto snapshot",
                    "crypto quote",
                    "crypto price",
                    "crypto bars",
                    "crypto trades",
                    "比特币行情",
                    "btc价格",
                    "加密货币价格",
                    "btc",
                    "bitcoin",
                    "eth",
                    "ethereum",
                    "crypto market",
                ],
                "mcp:alpaca/get_crypto_snapshot",
            ),
            (
                &[
                    "stock snapshot",
                    "stock quote",
                    "stock price",
                    "stock bars",
                    "stock trades",
                    "股票价格",
                    "aapl价格",
                    "tsla价格",
                    "stock market",
                    "aapl",
                    "tsla",
                ],
                "mcp:alpaca/get_stock_snapshot",
            ),
        ];

        let mut matched_skill_ids = std::collections::HashSet::new();
        let mut all_candidates = Vec::new();

        // 1. Try domain keyword mapping against step description
        for (keywords, skill_id) in domain_keywords {
            if keywords.iter().any(|kw| desc_lower.contains(kw)) {
                if let Some(skill) = registry.get(skill_id).await {
                    if matched_skill_ids.insert(skill_id.to_string()) {
                        all_candidates.push(skill);
                    }
                }
            }
        }

        // 🆕 FIX: 优先用原始用户目标匹配 skill（planning steps 常为英文 generic 描述，
        // 而 goal 包含中文领域关键词，匹配成功率更高）。不再要求 all_candidates 为空，
        // 而是总是把 goal 匹配的 skill 加入候选池。
        if let Some(ref goal) = *self.current_plan_goal.read().await {
            let goal_lower = goal.to_lowercase();
            for (keywords, skill_id) in domain_keywords {
                if keywords.iter().any(|kw| goal_lower.contains(kw)) {
                    if let Some(skill) = registry.get(skill_id).await {
                        if matched_skill_ids.insert(skill_id.to_string()) {
                            info!(
                                "P2 PLANNING: skill '{}' matched via goal '{}' for step '{}'",
                                skill_id,
                                goal.chars().take(30).collect::<String>(),
                                step_description.chars().take(40).collect::<String>()
                            );
                            all_candidates.push(skill);
                        }
                    } else {
                        warn!(
                            "P2 PLANNING: skill '{}' not found in registry (goal match)",
                            skill_id
                        );
                    }
                }
            }
        }

        // 2. Fallback to registry semantic search (name/description)
        let registry_candidates = registry.search(step_description).await;
        for skill in registry_candidates {
            if matched_skill_ids.insert(skill.skill.id.clone()) {
                all_candidates.push(skill);
            }
        }

        // 3. Tag-based search with keywords extracted from description
        let extracted_keywords: Vec<&str> = desc_lower
            .split_whitespace()
            .filter(|w| w.len() >= 3)
            .collect();
        for keyword in extracted_keywords.iter().take(5) {
            let tagged = registry.by_tag(keyword).await;
            for skill in tagged {
                if matched_skill_ids.insert(skill.skill.id.clone()) {
                    all_candidates.push(skill);
                }
            }
        }

        all_candidates
    }

    /// Execute action step
    ///
    /// 🟢 P2 FIX: Before falling back to LLM, attempts to match and execute a
    /// registered skill. This makes planning actually invoke tools instead
    /// of just chaining LLM calls.
    async fn execute_action_step(&self, step: &PlanStep) -> Result<ExecutionResult, AgentError> {
        let start_time = std::time::Instant::now();

        // 🟢 P2 FIX: Try skill registry first with semantic keyword matching
        if let Some(ref registry) = self.skill_registry {
            let enabled_count = registry.list_enabled().await.len();
            info!(
                "P2 PLANNING: skill registry has {} enabled skills for step '{}'",
                enabled_count,
                step.description.chars().take(40).collect::<String>()
            );
            let candidates = self
                .search_skills_for_step(registry, &step.description)
                .await;
            info!(
                "P2 PLANNING: found {} candidates for step '{}'",
                candidates.len(),
                step.description.chars().take(40).collect::<String>()
            );

            if let Some(skill) = candidates.into_iter().find(|s| s.enabled) {
                info!(
                    "P2 PLANNING: matched skill '{}' for step '{}', executing...",
                    skill.skill.name,
                    step.description.chars().take(40).collect::<String>()
                );
                // 🆕 FIX: Include the original user goal in the skill input so knowledge skills
                // don't ask follow-up questions about information the user already provided.
                let enriched_step_input =
                    if let Some(ref goal) = *self.current_plan_goal.read().await {
                        format!("[原始用户请求] {}\n\n[当前步骤] {}", goal, step.description)
                    } else {
                        step.description.clone()
                    };
                match self
                    .execute_registered_skill(&skill, &enriched_step_input, None)
                    .await
                {
                    Ok(result) => {
                        let _ = registry.record_usage(&skill.skill.id).await;
                        return Ok(ExecutionResult {
                            success: result.success,
                            data: Some(serde_json::json!({ "output": result.output })),
                            error: if result.success {
                                None
                            } else {
                                Some(result.output.clone())
                            },
                            duration_ms: start_time.elapsed().as_millis() as u64,
                            attempts: 1,
                        });
                    }
                    Err(e) => {
                        warn!(
                            "P2 PLANNING: skill execution failed for step '{}', falling back to \
                             LLM: {}",
                            step.description.chars().take(40).collect::<String>(),
                            e
                        );
                    }
                }
            } else {
                warn!(
                    "P2 PLANNING: no enabled skill matched for step '{}'",
                    step.description.chars().take(40).collect::<String>()
                );
            }
        } else {
            warn!(
                "P2 PLANNING: skill registry is None, cannot match skills for step '{}'",
                step.description.chars().take(40).collect::<String>()
            );
        }

        // Fallback: use LLM if available
        if let Some(llm) = &self.llm_interface {
            // 🆕 FIX: Planning 步骤调用 LLM 时携带原始用户目标，避免 LLM "盲打"
            // 导致输出质量差、耗时长。
            let mut messages: Vec<communication::Message> = Vec::new();
            if let Some(ref goal) = *self.current_plan_goal.read().await {
                messages.push(communication::Message::new(
                    uuid::Uuid::new_v4(),
                    communication::PlatformType::Custom,
                    format!("[原始用户请求] {}\n\n请基于以上请求完成以下步骤。", goal),
                ));
            }
            messages.push(communication::Message::new(
                uuid::Uuid::new_v4(),
                communication::PlatformType::Custom,
                step.description.clone(),
            ));

            match llm
                .call_llm(self.inject_skill_catalog(messages), None)
                .await
            {
                Ok(response) => Ok(ExecutionResult {
                    success: true,
                    data: Some(serde_json::json!({ "output": response })),
                    error: None,
                    duration_ms: start_time.elapsed().as_millis() as u64,
                    attempts: 1,
                }),
                Err(e) => Ok(ExecutionResult {
                    success: false,
                    data: None,
                    error: Some(format!("Step execution failed: {}", e)),
                    duration_ms: start_time.elapsed().as_millis() as u64,
                    attempts: 1,
                }),
            }
        } else {
            Ok(ExecutionResult {
                success: true,
                data: Some(
                    serde_json::json!({ "output": format!("Step executed successfully: {}", step.description) }),
                ),
                error: None,
                duration_ms: start_time.elapsed().as_millis() as u64,
                attempts: 1,
            })
        }
    }

    /// Execute decision step
    async fn execute_decision_step(&self, step: &PlanStep) -> Result<ExecutionResult, AgentError> {
        // Decision steps evaluate conditions
        self.execute_action_step(step).await
    }

    /// Execute reasoning step
    async fn execute_reasoning_step(&self, step: &PlanStep) -> Result<ExecutionResult, AgentError> {
        // Reasoning steps typically use LLM
        self.execute_action_step(step).await
    }

    /// Execute information gathering step
    async fn execute_information_step(
        &self,
        step: &PlanStep,
    ) -> Result<ExecutionResult, AgentError> {
        // Information steps gather data from various sources
        self.execute_action_step(step).await
    }

    /// Execute validation step
    async fn execute_validation_step(
        &self,
        step: &PlanStep,
    ) -> Result<ExecutionResult, AgentError> {
        // Validation steps verify results
        self.execute_action_step(step).await
    }

    /// Check if should continue on step failure
    fn should_continue_on_failure(&self, plan: &Plan) -> bool {
        plan.metadata
            .get("continue_on_failure")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    }

    /// Handle plan creation task
    pub async fn handle_plan_creation_task(
        &self,
        task: &Task,
    ) -> Result<(String, Vec<Artifact>), AgentError> {
        let planning_engine = self
            .planning_engine
            .as_ref()
            .ok_or_else(|| AgentError::InvalidConfig("Planning engine not configured".into()))?;

        let context = self.create_plan_context(task).await?;
        let strategy = self.select_plan_strategy(task);

        // 🆕 OPTIMIZATION PHASE 3: Use memory-aware plan creation
        let plan = planning_engine
            .create_plan_with_memory(
                &task.input,
                &context,
                Some(strategy),
                self.memory_system.as_deref(),
            )
            .await
            .map_err(|e| AgentError::Planning(format!("Failed to create plan: {}", e)))?;

        // Store plan
        {
            let mut active = self.active_plans.write().await;
            active.insert(plan.id.clone(), plan.clone());
        }

        let output = format!(
            "Created plan '{}' (ID: {}) with {} steps using {:?} strategy",
            plan.name,
            plan.id,
            plan.steps.len(),
            strategy
        );

        Ok((output, vec![]))
    }

    /// Handle plan execution task
    pub async fn handle_plan_execution_task(
        &self,
        task: &Task,
    ) -> Result<(String, Vec<Artifact>), AgentError> {
        let plan_id_str = task
            .parameters
            .get("plan_id")
            .or_else(|| {
                if task.input.is_empty() {
                    None
                } else {
                    Some(&task.input)
                }
            })
            .ok_or_else(|| AgentError::InvalidConfig("Missing 'plan_id' parameter".into()))?;

        let plan_id = PlanId::from_string(plan_id_str);

        // Find plan
        let plan = {
            let active = self.active_plans.read().await;
            active.get(&plan_id).cloned()
        }
        .ok_or_else(|| AgentError::NotFound(format!("Plan not found: {}", plan_id)))?;

        // Execute plan
        let result = self.execute_plan_internal(&plan).await?;

        if result.success {
            let output = result
                .data
                .as_ref()
                .and_then(|d| d.get("output"))
                .and_then(|o| o.as_str())
                .unwrap_or("Plan executed successfully")
                .to_string();
            Ok((output, vec![]))
        } else {
            Err(AgentError::Execution(
                result
                    .error
                    .unwrap_or_else(|| "Plan execution failed".to_string()),
            ))
        }
    }

    /// Handle plan adaptation task
    pub async fn handle_plan_adaptation_task(
        &self,
        task: &Task,
    ) -> Result<(String, Vec<Artifact>), AgentError> {
        let plan_id_str = task
            .parameters
            .get("plan_id")
            .ok_or_else(|| AgentError::InvalidConfig("Missing 'plan_id' parameter".into()))?;

        let plan_id = PlanId::from_string(plan_id_str);

        // Find plan
        let plan = {
            let active = self.active_plans.read().await;
            active.get(&plan_id).cloned()
        }
        .ok_or_else(|| AgentError::NotFound(format!("Plan not found: {}", plan_id)))?;

        // Attempt replanning if replanner is available
        if let Some(replanner) = &self.replanner {
            let mut adapted_plan = plan.clone();
            let trigger = crate::planning::RePlanTrigger::GoalChanged {
                new_goal: task.input.clone(),
                reason: "User requested plan adaptation".to_string(),
            };
            match replanner.replan(&mut adapted_plan, &trigger).await {
                Ok(()) => {
                    // Execute adapted plan
                    let result = self.execute_plan_internal(&adapted_plan).await?;
                    if result.success {
                        return Ok(("Plan adapted and executed successfully".to_string(), vec![]));
                    }
                }
                Err(e) => {
                    warn!("Replanning failed: {}", e);
                }
            }
        }

        Err(AgentError::Planning("Plan adaptation failed".to_string()))
    }

    /// Get active plan by ID
    pub async fn get_active_plan(&self, plan_id: &PlanId) -> Option<Plan> {
        let active = self.active_plans.read().await;
        active.get(plan_id).cloned()
    }

    /// List all active plans
    pub async fn list_active_plans(&self) -> Vec<Plan> {
        let active = self.active_plans.read().await;
        active.values().cloned().collect()
    }

    /// Cancel an active plan
    pub async fn cancel_plan(&self, plan_id: &PlanId) -> Result<(), AgentError> {
        let mut active = self.active_plans.write().await;
        if let Some(mut plan) = active.remove(plan_id) {
            plan.status = PlanStatus::Cancelled;
            info!("Cancelled plan: {}", plan_id);
            Ok(())
        } else {
            Err(AgentError::NotFound(format!("Plan not found: {}", plan_id)))
        }
    }

    /// Explicitly create a plan using planning engine
    pub async fn create_plan(
        &self,
        goal: &str,
        strategy: PlanStrategy,
    ) -> Result<Plan, AgentError> {
        let engine = self
            .planning_engine
            .as_ref()
            .ok_or_else(|| AgentError::InvalidConfig("Planning engine not configured".into()))?;

        let context = PlanContext::new(&self.config.id);

        // 🆕 OPTIMIZATION PHASE 3: Use memory-aware plan creation
        let plan = engine
            .create_plan_with_memory(
                goal,
                &context,
                Some(strategy),
                self.memory_system.as_deref(),
            )
            .await
            .map_err(|e| AgentError::Planning(format!("Failed to create plan: {}", e)))?;

        // Store plan in active_plans
        {
            let mut active = self.active_plans.write().await;
            active.insert(plan.id.clone(), plan.clone());
        }

        Ok(plan)
    }

    /// Explicitly execute a plan
    pub async fn execute_plan(&self, plan: &Plan) -> Result<ExecutionResult, AgentError> {
        // Store plan first
        {
            let mut active = self.active_plans.write().await;
            active.insert(plan.id.clone(), plan.clone());
        }

        let result = self.execute_plan_internal(plan).await;

        // Cleanup
        {
            let mut active = self.active_plans.write().await;
            active.remove(&plan.id);
        }

        result
    }

    // 🆕 DEVICE FIX: Device automation task handlers

    /// Handle device automation task
    pub async fn handle_device_automation_task(
        &self,
        task: &Task,
    ) -> Result<(String, Vec<Artifact>), AgentError> {
        let device = self.device.as_ref().ok_or_else(|| {
            AgentError::InvalidConfig("No device configured for automation".into())
        })?;

        let action = task.parameters.get("action").ok_or_else(|| {
            AgentError::InvalidConfig("Missing 'action' parameter for device automation".into())
        })?;

        let result = match action.as_str() {
            "tap" => {
                let x = task
                    .parameters
                    .get("x")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0);
                let y = task
                    .parameters
                    .get("y")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0);
                match device {
                    Device::Node(d) => d.tap(x, y).await,
                    Device::Ios(d) => d.tap(x, y).await,
                    Device::Android(d) => d.tap(x, y).await,
                }?;
                format!("Tapped at ({}, {})", x, y)
            }
            "swipe" => {
                let from_x = task
                    .parameters
                    .get("from_x")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0);
                let from_y = task
                    .parameters
                    .get("from_y")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0);
                let to_x = task
                    .parameters
                    .get("to_x")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0);
                let to_y = task
                    .parameters
                    .get("to_y")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0);
                let duration = task
                    .parameters
                    .get("duration")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(500);
                match device {
                    Device::Node(d) => d.swipe(from_x, from_y, to_x, to_y, duration).await,
                    Device::Ios(d) => d.swipe(from_x, from_y, to_x, to_y, duration).await,
                    Device::Android(d) => d.swipe(from_x, from_y, to_x, to_y, duration).await,
                }?;
                format!(
                    "Swiped from ({}, {}) to ({}, {})",
                    from_x, from_y, to_x, to_y
                )
            }
            "screenshot" => {
                let screenshot = match device {
                    Device::Node(d) => d.take_screenshot().await,
                    Device::Ios(d) => d.take_screenshot().await,
                    Device::Android(d) => d.take_screenshot().await,
                }?;
                format!("Screenshot captured: {} bytes", screenshot.len())
            }
            "press_button" => {
                let button_str = task
                    .parameters
                    .get("button")
                    .unwrap_or(&"home".to_string())
                    .clone();
                let button = match button_str.as_str() {
                    "home" => crate::device::HardwareButton::Home,
                    "back" => crate::device::HardwareButton::Back,
                    "power" => crate::device::HardwareButton::Power,
                    "volume_up" => crate::device::HardwareButton::VolumeUp,
                    "volume_down" => crate::device::HardwareButton::VolumeDown,
                    _ => crate::device::HardwareButton::Home,
                };
                match device {
                    Device::Node(d) => d.press_button(button).await,
                    Device::Ios(d) => d.press_button(button).await,
                    Device::Android(d) => d.press_button(button).await,
                }?;
                format!("Pressed button: {:?}", button)
            }
            "type_text" => {
                let text = &task.input;
                match device {
                    Device::Node(d) => d.type_text(text).await,
                    Device::Ios(d) => d.type_text(text).await,
                    Device::Android(d) => d.type_text(text).await,
                }?;
                format!("Typed text: {}", text)
            }
            "find_element" => {
                let locator_type = task
                    .parameters
                    .get("locator_type")
                    .unwrap_or(&"id".to_string())
                    .clone();
                let locator_value = &task.input;
                let locator = crate::device::ElementLocator::new(
                    match locator_type.as_str() {
                        "id" => crate::device::LocatorType::Id,
                        "xpath" => crate::device::LocatorType::XPath,
                        "accessibility_id" => crate::device::LocatorType::AccessibilityId,
                        "text" => crate::device::LocatorType::Text,
                        _ => crate::device::LocatorType::Id,
                    },
                    locator_value,
                );
                let element = match device {
                    Device::Node(d) => d.find_element(&locator).await,
                    Device::Ios(d) => d.find_element(&locator).await,
                    Device::Android(d) => d.find_element(&locator).await,
                }?;
                format!("Found element: {:?}", element)
            }
            "tap_element" => {
                let locator_type = task
                    .parameters
                    .get("locator_type")
                    .unwrap_or(&"id".to_string())
                    .clone();
                let locator_value = &task.input;
                let locator = crate::device::ElementLocator::new(
                    match locator_type.as_str() {
                        "id" => crate::device::LocatorType::Id,
                        "xpath" => crate::device::LocatorType::XPath,
                        "accessibility_id" => crate::device::LocatorType::AccessibilityId,
                        "text" => crate::device::LocatorType::Text,
                        _ => crate::device::LocatorType::Id,
                    },
                    locator_value,
                );
                match device {
                    Device::Node(d) => d.tap_element(&locator).await,
                    Device::Ios(d) => d.tap_element(&locator).await,
                    Device::Android(d) => d.tap_element(&locator).await,
                }?;
                format!("Tapped element: {}", locator_value)
            }
            _ => {
                return Err(AgentError::InvalidConfig(format!(
                    "Unknown device action: {}",
                    action
                )));
            }
        };

        Ok((result, vec![]))
    }

    /// Handle app lifecycle task
    pub async fn handle_app_lifecycle_task(
        &self,
        task: &Task,
    ) -> Result<(String, Vec<Artifact>), AgentError> {
        let device = self.device.as_ref().ok_or_else(|| {
            AgentError::InvalidConfig("No device configured for app lifecycle".into())
        })?;

        let operation = task
            .parameters
            .get("operation")
            .ok_or_else(|| AgentError::InvalidConfig("Missing 'operation' parameter".into()))?;

        let package = &task.input;

        let result = match operation.as_str() {
            "install" => {
                let app_path = task.parameters.get("app_path").unwrap_or(package);
                match device {
                    Device::Node(d) => d.install_app(app_path).await,
                    Device::Ios(d) => d.install_app(app_path).await,
                    Device::Android(d) => d.install_app(app_path).await,
                }?;
                format!("Installed app: {}", app_path)
            }
            "uninstall" => {
                match device {
                    Device::Node(d) => d.uninstall_app(package).await,
                    Device::Ios(d) => d.uninstall_app(package).await,
                    Device::Android(d) => d.uninstall_app(package).await,
                }?;
                format!("Uninstalled app: {}", package)
            }
            "launch" => {
                match device {
                    Device::Node(d) => d.launch_app(package).await,
                    Device::Ios(d) => d.launch_app(package).await,
                    Device::Android(d) => d.launch_app(package).await,
                }?;
                format!("Launched app: {}", package)
            }
            "close" => {
                match device {
                    Device::Node(d) => d.close_app(package).await,
                    Device::Ios(d) => d.close_app(package).await,
                    Device::Android(d) => d.close_app(package).await,
                }?;
                format!("Closed app: {}", package)
            }
            "is_installed" => {
                let installed = match device {
                    Device::Node(d) => d.is_app_installed(package).await,
                    Device::Ios(d) => d.is_app_installed(package).await,
                    Device::Android(d) => d.is_app_installed(package).await,
                }?;
                format!("App {} installed: {}", package, installed)
            }
            "clear_data" => {
                match device {
                    Device::Node(d) => d.clear_app_data(package).await,
                    Device::Ios(d) => d.clear_app_data(package).await,
                    Device::Android(d) => d.clear_app_data(package).await,
                }?;
                format!("Cleared app data: {}", package)
            }
            _ => {
                return Err(AgentError::InvalidConfig(format!(
                    "Unknown app lifecycle operation: {}",
                    operation
                )));
            }
        };

        Ok((result, vec![]))
    }

    /// 🟢 P1 FIX: Handle workflow execution tasks
    pub async fn handle_workflow_task(
        &self,
        task: &Task,
    ) -> Result<(String, Vec<Artifact>), AgentError> {
        let workflow_id = task
            .parameters
            .get("workflow_id")
            .ok_or_else(|| AgentError::InvalidConfig("Missing 'workflow_id' parameter".into()))?;

        let registry = self
            .workflow_registry
            .as_ref()
            .ok_or_else(|| AgentError::InvalidConfig("Workflow registry not configured".into()))?;

        let definition = registry
            .get(workflow_id)
            .ok_or_else(|| {
                AgentError::SkillNotFound(format!("Workflow '{}' not found", workflow_id))
            })?
            .clone();

        let engine = crate::workflow::WorkflowEngine::new();
        let instance = engine
            .execute(&definition, self, serde_json::Value::Null, None)
            .await?;

        let mut notification = String::new();
        if definition.config.notify_on_complete {
            let notify_prompt = format!(
                "请生成一条简洁的工作流完成通知：工作流 '{}' 已执行完毕，状态：{}，共 {} \
                 个步骤，完成度 {}%，耗时 {} 秒。",
                workflow_id,
                instance.status,
                instance.step_states.len(),
                instance.completion_pct(),
                instance.duration_secs()
            );
            match self
                .call_llm_prompt(
                    notify_prompt,
                    Some::<String>(
                        "You are a workflow notification assistant. Generate only a concise \
                         completion message, no more than two sentences."
                            .into(),
                    ),
                )
                .await
            {
                Ok(notify_text) => {
                    info!(
                        "Workflow {} notification generated: {}",
                        workflow_id, notify_text
                    );
                    notification = format!("\n\n📢 通知: {}", notify_text);
                }
                Err(e) => {
                    warn!(
                        "Failed to generate notification for workflow {}: {}",
                        workflow_id, e
                    );
                }
            }
        }

        let result = format!(
            "Workflow '{}' executed with status: {} ({} steps, {}% complete, {}s){}",
            workflow_id,
            instance.status,
            instance.step_states.len(),
            instance.completion_pct(),
            instance.duration_secs(),
            notification
        );

        let artifacts = vec![Artifact {
            id: instance.id.clone(),
            artifact_type: "workflow_instance".to_string(),
            content: serde_json::to_vec_pretty(&instance).unwrap_or_default(),
            mime_type: "application/json".to_string(),
        }];

        Ok((result, artifacts))
    }

    /// Handle foreign runtime tasks (Python/Node.js)
    pub async fn handle_foreign_runtime_task(
        &self,
        task: &crate::Task,
    ) -> crate::error::Result<(String, Vec<crate::Artifact>)> {
        use crate::runtime::executor::TaskExecutor;
        use crate::runtime::foreign_executor::ForeignTaskExecutor;

        let manager = self.foreign_rt_manager.as_ref().ok_or_else(|| {
            crate::error::AgentError::InvalidConfig(
                "Foreign runtime manager not configured for this agent".to_string(),
            )
        })?;

        let executor = ForeignTaskExecutor::with_manager(manager.clone());

        let result = executor.execute(task.clone()).await.map_err(|e| {
            crate::error::AgentError::Execution(format!("Foreign runtime execution failed: {}", e))
        })?;

        let output = if result.success {
            result.output
        } else {
            format!("Error: {}", result.output)
        };

        let artifacts = result
            .artifacts
            .into_iter()
            .map(|a| crate::Artifact {
                id: a.id,
                artifact_type: a.artifact_type,
                content: a.content,
                mime_type: a.mime_type,
            })
            .collect();

        Ok((output, artifacts))
    }
}

/// Task complexity level for determining execution strategy
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskComplexity {
    /// Simple task - can be executed directly
    Simple,
    /// Complex task - requires planning
    Complex,
}

// ============================================================================
// 🆕 FIX: Structured output formatters for known MCP skills
// ============================================================================

fn format_known_skill_output(skill_id: &str, raw_output: &str) -> Option<String> {
    if let Some(error) = format_structured_mcp_error(skill_id, raw_output) {
        return Some(error);
    }

    match skill_id {
        "mcp:alpaca/get_all_positions" | "mcp:alpaca/get_open_position" => {
            format_alpaca_positions(raw_output)
        }
        "mcp:alpaca/get_crypto_latest_trade" => format_crypto_latest_trade(raw_output),
        "mcp:alpaca/get_crypto_latest_quote" | "mcp:alpaca/get_crypto_quotes" => {
            format_crypto_quote(raw_output)
        }
        "mcp:alpaca/get_crypto_latest_orderbook" => format_crypto_orderbook(raw_output),
        "mcp:alpaca/get_crypto_snapshot" | "mcp:alpaca/get_crypto_bars" => {
            format_crypto_snapshot(raw_output)
        }
        "mcp:alpaca/get_stock_snapshot"
        | "mcp:alpaca/get_stock_quote"
        | "mcp:alpaca/get_stock_bars" => format_crypto_snapshot(raw_output),
        _ => None,
    }
}

fn format_structured_mcp_error(skill_id: &str, raw_output: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(raw_output).ok()?;
    if v.get("isError").and_then(|v| v.as_bool()).unwrap_or(false)
        || v.get("is_error").and_then(|v| v.as_bool()).unwrap_or(false)
    {
        let message = v
            .get("content")
            .and_then(|content| content.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.get("text").and_then(|text| text.as_str()))
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .filter(|text| !text.trim().is_empty())
            .or_else(|| {
                v.get("error")
                    .and_then(|error| error.get("message").or(Some(error)))
                    .and_then(|message| message.as_str())
                    .map(|message| message.to_string())
            })
            .unwrap_or_else(|| raw_output.to_string());
        return Some(format_mcp_call_error(skill_id, &message));
    }

    if let Some(error) = v.get("error") {
        let message = error
            .get("message")
            .and_then(|message| message.as_str())
            .unwrap_or_else(|| error.as_str().unwrap_or("unknown error"));
        return Some(format_mcp_call_error(skill_id, message));
    }

    None
}

fn format_mcp_call_error(skill_id: &str, message: &str) -> String {
    let clean = message.trim();
    if skill_id.starts_with("mcp:alpaca/") {
        if clean.contains("ConnectError") || clean.contains("connection attempts failed") {
            return "Alpaca MCP 调用失败：已找到 Alpaca 凭证和工具，但当前运行环境无法连接 Alpaca \
                    API。请确认 BeeBotOS 进程所在环境允许访问 Alpaca \
                    域名，或检查网络/代理配置后重试。"
                .to_string();
        }
        return format!(
            "Alpaca MCP 调用失败：{}",
            if clean.is_empty() {
                "unknown error"
            } else {
                clean
            }
        );
    }

    format!(
        "MCP tool 调用失败：{}",
        if clean.is_empty() {
            "unknown error"
        } else {
            clean
        }
    )
}

fn format_alpaca_positions(raw_output: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(raw_output).ok()?;
    let positions = if let Some(arr) = v.as_array() {
        arr
    } else if let Some(arr) = v.get("positions").and_then(|p| p.as_array()) {
        arr
    } else if let Some(arr) = v.get("result").and_then(|p| p.as_array()) {
        arr
    } else if v.is_object() {
        return is_alpaca_position(&v).then(|| format_single_position(&v));
    } else {
        return None;
    };

    if positions.is_empty() {
        return Some("当前 Alpaca 账户没有持仓。".to_string());
    }

    let mut lines = vec!["📌 Alpaca 当前持仓".to_string()];
    for position in positions {
        if !is_alpaca_position(position) {
            continue;
        }
        let symbol = position
            .get("symbol")
            .and_then(|s| s.as_str())
            .unwrap_or("UNKNOWN");
        let qty = json_text(position, &["qty", "quantity"]).unwrap_or_else(|| "-".to_string());
        let side = json_text(position, &["side"]).unwrap_or_default();
        let market_value = json_text(position, &["market_value", "marketValue"]);
        let avg_entry = json_text(position, &["avg_entry_price", "avgEntryPrice"]);
        let current = json_text(position, &["current_price", "currentPrice"]);
        let unrealized = json_text(position, &["unrealized_pl", "unrealizedPl"]);
        let unrealized_pct = json_text(position, &["unrealized_plpc", "unrealizedPlpc"]);

        lines.push(format!(
            "\n【{}】数量: {}{}",
            symbol,
            qty,
            if side.is_empty() {
                String::new()
            } else {
                format!(" ({})", side)
            }
        ));
        if let Some(value) = market_value {
            lines.push(format!("  市值: {} USD", value));
        }
        if let Some(value) = avg_entry {
            lines.push(format!("  成本均价: {} USD", value));
        }
        if let Some(value) = current {
            lines.push(format!("  当前价格: {} USD", value));
        }
        if let Some(value) = unrealized {
            lines.push(format!("  未实现盈亏: {} USD", value));
        }
        if let Some(value) = unrealized_pct {
            if let Ok(n) = value.parse::<f64>() {
                lines.push(format!("  未实现盈亏率: {:+.2}%", n * 100.0));
            } else {
                lines.push(format!("  未实现盈亏率: {}", value));
            }
        }
    }
    if lines.len() == 1 {
        None
    } else {
        Some(lines.join("\n"))
    }
}

fn format_single_position(position: &serde_json::Value) -> String {
    let symbol = position
        .get("symbol")
        .and_then(|s| s.as_str())
        .unwrap_or("UNKNOWN");
    let qty = json_text(position, &["qty", "quantity"]).unwrap_or_else(|| "-".to_string());
    let mut lines = vec![format!("📌 Alpaca 持仓：{} 数量 {}", symbol, qty)];
    for (label, keys) in [
        ("市值", ["market_value", "marketValue"]),
        ("成本均价", ["avg_entry_price", "avgEntryPrice"]),
        ("当前价格", ["current_price", "currentPrice"]),
        ("未实现盈亏", ["unrealized_pl", "unrealizedPl"]),
    ] {
        if let Some(value) = json_text(position, &keys) {
            lines.push(format!("{}: {} USD", label, value));
        }
    }
    lines.join("\n")
}

fn is_alpaca_position(value: &serde_json::Value) -> bool {
    value.is_object()
        && value.get("symbol").and_then(|s| s.as_str()).is_some()
        && (value.get("qty").is_some()
            || value.get("quantity").is_some()
            || value.get("market_value").is_some()
            || value.get("marketValue").is_some()
            || value.get("asset_class").is_some())
}

fn json_text(value: &serde_json::Value, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(v) = value.get(key) {
            if let Some(s) = v.as_str() {
                return Some(s.to_string());
            }
            if let Some(n) = v.as_f64() {
                return Some(
                    format!("{:.6}", n)
                        .trim_end_matches('0')
                        .trim_end_matches('.')
                        .to_string(),
                );
            }
        }
    }
    None
}

fn format_crypto_latest_trade(raw_output: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(raw_output).ok()?;
    // Alpaca may return trades under a "trades" key, or as a top-level object keyed
    // by symbol.
    let trades = v
        .get("trades")
        .and_then(|s| s.as_object())
        .or_else(|| v.as_object())?;
    let mut lines = vec!["📊 最新成交".to_string()];
    for (symbol, data) in trades {
        if symbol == "trades" {
            continue;
        }
        if let Some(p) = data.get("p").and_then(|p| p.as_f64()) {
            lines.push(format!("• {} 最新成交价: {:.2} USD", symbol, p));
        } else if let Some(price) = data.as_f64() {
            // Scalar price fallback (rare)
            lines.push(format!("• {} 最新成交价: {:.2} USD", symbol, price));
        } else {
            continue;
        }
        let s = data.get("s").and_then(|s| s.as_f64()).unwrap_or(0.0);
        let t = data.get("t").and_then(|t| t.as_str()).unwrap_or("");
        if s > 0.0 {
            lines.push(format!("  成交量: {:.6}", s));
        }
        if !t.is_empty() {
            lines.push(format!("  成交时间: {}", t));
        }
    }
    if lines.len() == 1 {
        // No trade data extracted — fall back to generic JSON
        return None;
    }
    Some(lines.join("\n"))
}

fn format_crypto_quote(raw_output: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(raw_output).ok()?;
    let quotes = v
        .get("quotes")
        .and_then(|s| s.as_object())
        .or_else(|| v.as_object())?;
    let mut lines = vec!["📊 最新报价".to_string()];
    let mut any_data = false;

    for (symbol, data) in quotes {
        if symbol == "quotes" {
            continue;
        }
        let bid = data
            .get("bp")
            .or_else(|| data.get("bidPrice"))
            .or_else(|| data.get("bid"))
            .and_then(|v| v.as_f64());
        let ask = data
            .get("ap")
            .or_else(|| data.get("askPrice"))
            .or_else(|| data.get("ask"))
            .and_then(|v| v.as_f64());
        if let (Some(bid), Some(ask)) = (bid, ask) {
            any_data = true;
            lines.push(format!("\n【{}】", symbol));
            lines.push(format!("  买一 / 卖一: {:.2} / {:.2} USD", bid, ask));
            lines.push(format!("  中间价: {:.2} USD", (bid + ask) / 2.0));
        }
        if let Some(t) = data.get("t").and_then(|t| t.as_str()) {
            lines.push(format!("  时间: {}", t));
        }
    }

    any_data.then(|| lines.join("\n"))
}

fn format_crypto_orderbook(raw_output: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(raw_output).ok()?;
    let book = v
        .get("orderbook")
        .or_else(|| v.get("orderBook"))
        .unwrap_or(&v);
    let symbol = v
        .get("symbol")
        .and_then(|s| s.as_str())
        .or_else(|| book.get("symbol").and_then(|s| s.as_str()))
        .unwrap_or("BTC/USD");
    let bids = book.get("bids").and_then(|b| b.as_array())?;
    let asks = book.get("asks").and_then(|a| a.as_array())?;
    let best_bid = bids.first().and_then(orderbook_level);
    let best_ask = asks.first().and_then(orderbook_level);

    let mut lines = vec![format!("📚 {} 订单簿", symbol)];
    if let Some((price, size)) = best_bid {
        lines.push(format!("买一: {:.2} USD，数量 {:.6}", price, size));
    }
    if let Some((price, size)) = best_ask {
        lines.push(format!("卖一: {:.2} USD，数量 {:.6}", price, size));
    }
    if let (Some((bid, _)), Some((ask, _))) = (best_bid, best_ask) {
        lines.push(format!("中间价: {:.2} USD", (bid + ask) / 2.0));
        lines.push(format!("价差: {:.2} USD", ask - bid));
    }
    if lines.len() == 1 {
        None
    } else {
        Some(lines.join("\n"))
    }
}

fn orderbook_level(value: &serde_json::Value) -> Option<(f64, f64)> {
    let price = value
        .get("p")
        .or_else(|| value.get("price"))
        .and_then(|v| v.as_f64())?;
    let size = value
        .get("s")
        .or_else(|| value.get("size"))
        .and_then(|v| v.as_f64())?;
    Some((price, size))
}

fn format_crypto_snapshot(raw_output: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(raw_output).ok()?;
    // Alpaca may wrap snapshots under a "snapshots" key or return them directly.
    let snapshots = v
        .get("snapshots")
        .and_then(|s| s.as_object())
        .or_else(|| v.as_object())?;
    let mut lines = vec!["📈 市场行情快照".to_string()];
    let mut any_data = false;

    for (symbol, data) in snapshots {
        // Skip non-symbol wrapper keys at the top level when falling back to
        // v.as_object()
        if symbol == "snapshots" {
            continue;
        }
        lines.push(format!("\n【{}】", symbol));
        let mut symbol_has_data = false;

        // Helper: try multiple common price field names
        let get_price = |obj: &serde_json::Value, keys: &[&str]| -> Option<f64> {
            for k in keys {
                if let Some(p) = obj.get(k).and_then(|p| p.as_f64()) {
                    return Some(p);
                }
            }
            None
        };

        if let Some(lt) = data.get("latestTrade") {
            if let Some(p) = get_price(lt, &["p", "price", "P"]) {
                lines.push(format!("  最新成交价: {:.2} USD", p));
                symbol_has_data = true;
            }
            if let Some(s) = lt
                .get("s")
                .and_then(|s| s.as_f64())
                .or_else(|| lt.get("size").and_then(|s| s.as_f64()))
            {
                lines.push(format!("  最新成交量: {:.6}", s));
                symbol_has_data = true;
            }
        }

        if let Some(q) = data.get("latestQuote") {
            let bid = get_price(q, &["bp", "bidPrice", "bid"])
                .or_else(|| q.get("b").and_then(|p| p.as_f64()));
            let ask = get_price(q, &["ap", "askPrice", "ask"])
                .or_else(|| q.get("a").and_then(|p| p.as_f64()));
            if let (Some(bid), Some(ask)) = (bid, ask) {
                lines.push(format!("  买一 / 卖一: {:.2} / {:.2} USD", bid, ask));
                symbol_has_data = true;
            }
        }

        if let Some(db) = data.get("dailyBar") {
            let o = get_price(db, &["o", "open"]);
            let h = get_price(db, &["h", "high"]);
            let l = get_price(db, &["l", "low"]);
            let c = get_price(db, &["c", "close"]);
            let vol = db
                .get("v")
                .and_then(|p| p.as_f64())
                .or_else(|| db.get("volume").and_then(|p| p.as_f64()));
            if o.is_some() && h.is_some() && l.is_some() && c.is_some() {
                lines.push(format!(
                    "  日K线: 开 {:.2} / 高 {:.2} / 低 {:.2} / 收 {:.2}",
                    o.unwrap(),
                    h.unwrap(),
                    l.unwrap(),
                    c.unwrap()
                ));
                symbol_has_data = true;
            }
            if let Some(vol) = vol {
                lines.push(format!("  日成交量: {:.4}", vol));
                symbol_has_data = true;
            }
        }

        if let Some(pb) = data.get("prevDailyBar") {
            let prev_c = get_price(pb, &["c", "close"]);
            let curr_c = data
                .get("dailyBar")
                .and_then(|db| get_price(db, &["c", "close"]));
            if let (Some(prev_c), Some(curr_c)) = (prev_c, curr_c) {
                let change = ((curr_c - prev_c) / prev_c) * 100.0;
                lines.push(format!("  较昨日收盘: {:+.2}%", change));
                symbol_has_data = true;
            }
        }

        if let Some(mb) = data.get("minuteBar") {
            if let Some(c) = get_price(mb, &["c", "close"]) {
                lines.push(format!("  最新分钟线收盘价: {:.2} USD", c));
                symbol_has_data = true;
            }
        }

        if symbol_has_data {
            any_data = true;
        }
    }

    if !any_data {
        return None;
    }
    Some(lines.join("\n"))
}

fn format_generic_json(raw_output: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(raw_output).ok()?;
    let lines = flatten_json_value("", &v, 0);
    Some(lines.join("\n"))
}

fn flatten_json_value(key: &str, value: &serde_json::Value, depth: usize) -> Vec<String> {
    let indent = "  ".repeat(depth);
    match value {
        serde_json::Value::Object(map) => {
            let mut lines = vec![];
            for (k, v) in map {
                let full_key = if key.is_empty() {
                    k.clone()
                } else {
                    format!("{} > {}", key, k)
                };
                lines.extend(flatten_json_value(&full_key, v, depth + 1));
            }
            lines
        }
        serde_json::Value::Array(arr) => {
            let mut lines = vec![];
            for (i, v) in arr.iter().enumerate() {
                let item_key = format!("{}[{}]", key, i);
                lines.extend(flatten_json_value(&item_key, v, depth + 1));
            }
            lines
        }
        serde_json::Value::String(s) => vec![format!("{}- {}: {}", indent, key, s)],
        serde_json::Value::Number(n) => vec![format!("{}- {}: {}", indent, key, n)],
        serde_json::Value::Bool(b) => vec![format!("{}- {}: {}", indent, key, b)],
        serde_json::Value::Null => vec![format!("{}- {}: null", indent, key)],
    }
}

// ============================================================================
// 🆕 OPTIMIZATION: Unit Tests for Planning Integration
// ============================================================================

#[cfg(test)]
mod planning_integration_tests {
    use std::sync::Mutex;

    use super::*;
    use crate::planning::{PlanExecutor, PlanningEngine};

    static WORKSPACE_ENV_LOCK: Mutex<()> = Mutex::new(());

    /// Helper function to create a test agent without planning
    fn create_test_agent() -> Agent {
        Agent::new(AgentConfig::default())
    }

    /// Helper function to create a test agent with planning capabilities
    fn create_test_agent_with_planning() -> Agent {
        Agent::new(AgentConfig::default())
            .with_planning_engine(Arc::new(PlanningEngine::new()))
            .with_plan_executor(Arc::new(PlanExecutor::new()))
    }

    fn test_loaded_skill(id: &str) -> LoadedSkill {
        LoadedSkill {
            id: id.to_string(),
            name: "Test Skill".to_string(),
            version: Version::new(1, 0, 0),
            wasm_path: PathBuf::new(),
            source_path: PathBuf::new(),
            manifest: SkillManifest {
                id: id.to_string(),
                name: "Test Skill".to_string(),
                version: Version::new(1, 0, 0),
                description: "A test skill for context activation".to_string(),
                author: "BeeBotOS".to_string(),
                capabilities: vec!["testing".to_string()],
                permissions: vec![],
                entry_point: String::new(),
                license: String::new(),
                functions: vec![],
                prompt_template: "Use this skill context for testing.".to_string(),
                examples: String::new(),
                when_to_use: String::new(),
                when_not_to_use: None,
                activation_examples: vec![],
                activation_negative_examples: vec![],
                dependencies: vec![],
                ..Default::default()
            },
        }
    }

    #[test]
    fn test_mcp_display_tool_name_round_trip() {
        let display = Agent::mcp_display_tool_name("alpaca", "get_crypto_snapshot");
        assert_eq!(display, "mcp:alpaca/get_crypto_snapshot");

        let parsed = Agent::parse_mcp_display_tool_name(&display).unwrap();
        assert_eq!(parsed.server_name, "alpaca");
        assert_eq!(parsed.tool_name, "get_crypto_snapshot");

        let dynamic = Agent::mcp_dynamic_tool_name(&parsed.server_name, &parsed.tool_name);
        assert!(dynamic.starts_with("mcp__"));
    }

    #[test]
    fn test_mcp_tool_search_definition_includes_tool_name() {
        let def = Agent::mcp_tool_search_definition();
        let properties = def.parameters.get("properties").unwrap();
        assert!(properties.get("tool_name").is_some());
        assert!(properties.get("query").is_some());
        assert!(def.parameters.get("required").is_none());
    }

    #[test]
    fn test_confirmation_message_detection() {
        assert!(is_confirmation_message("确认"));
        assert!(is_confirmation_message("confirm"));
        assert!(!is_confirmation_message("确认买入 0.01 手黄金"));
    }

    #[test]
    fn test_approval_confirmation_guard_detection() {
        assert!(has_side_effect_execution_intent(
            "利用metatrader帮我买0.01手黄金"
        ));
        assert!(has_side_effect_execution_intent("删除这个文件"));
        assert!(looks_like_unbacked_approval_confirmation(
            "请回复「确认」来执行此交易，真实账户市价单将立即成交。"
        ));
        assert!(looks_like_unbacked_approval_confirmation(
            "这是高风险删除操作，请回复 confirm 继续。"
        ));
        assert!(!looks_like_unbacked_approval_confirmation(
            "当前黄金价格约为 4484.93。"
        ));
    }

    #[test]
    fn test_alpaca_position_formatter_handles_result_wrapper() {
        let raw = r#"{"result":[{"symbol":"BTCUSD","asset_class":"crypto","qty":"0.001217063","side":"long","market_value":"99.03311","avg_entry_price":"80349","current_price":"81370.57","unrealized_pl":"1.243315","unrealized_plpc":"0.01271"}]}"#;
        let formatted = format_alpaca_positions(raw).expect("formatted position");
        assert!(formatted.contains("Alpaca 当前持仓"));
        assert!(formatted.contains("BTCUSD"));
        assert!(formatted.contains("数量: 0.001217063"));
        assert!(!formatted.contains("UNKNOWN"));
    }

    #[test]
    fn test_alpaca_error_formatter_does_not_render_unknown_position() {
        let raw = r#"{"error":{"code":-32000,"message":"Error calling tool 'get_crypto_latest_quote': Request error (ConnectError): All connection attempts failed"}}"#;
        let formatted = format_known_skill_output("mcp:alpaca/get_all_positions", raw)
            .expect("formatted error");
        assert!(formatted.contains("Alpaca MCP 调用失败"));
        assert!(formatted.contains("无法连接 Alpaca API"));
        assert!(!formatted.contains("UNKNOWN"));
    }

    #[test]
    fn test_react_web_url_policy_blocks_private_targets() {
        assert!(Agent::validate_public_web_url("file:///etc/passwd").is_err());
        assert!(Agent::validate_public_web_url("http://localhost:8080").is_err());
        assert!(Agent::validate_public_web_url("http://127.0.0.1:8080").is_err());
        assert!(Agent::validate_public_web_url("http://10.0.0.1").is_err());
        assert!(Agent::validate_public_web_url("https://example.com").is_ok());
    }

    #[test]
    fn test_react_controlled_tools_are_exposed_by_default_and_can_be_disabled() {
        std::env::remove_var("BEEBOTOS_ENABLE_CONTROLLED_WORKSPACE_TOOLS");
        let agent = create_test_agent();
        let default_tools: std::collections::HashSet<String> = agent
            .builtin_react_tools()
            .into_iter()
            .map(|tool| tool.name)
            .collect();
        assert!(default_tools.contains("write_file"));
        assert!(default_tools.contains("edit_file"));
        assert!(default_tools.contains("exec"));

        std::env::set_var("BEEBOTOS_ENABLE_CONTROLLED_WORKSPACE_TOOLS", "0");
        let disabled_agent = create_test_agent();
        let disabled_tools: std::collections::HashSet<String> = disabled_agent
            .builtin_react_tools()
            .into_iter()
            .map(|tool| tool.name)
            .collect();
        assert!(!disabled_tools.contains("write_file"));
        assert!(!disabled_tools.contains("edit_file"));
        assert!(!disabled_tools.contains("exec"));
        std::env::remove_var("BEEBOTOS_ENABLE_CONTROLLED_WORKSPACE_TOOLS");
    }

    #[test]
    fn test_react_duckduckgo_result_parser() {
        let html = r#"
            <a class="result__a" href="https://duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com%2Fa">Example &amp; Result</a>
        "#;
        let results = Agent::parse_duckduckgo_results(html, 5);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Example & Result");
        assert_eq!(results[0].url, "https://example.com/a");
    }

    #[test]
    fn test_react_bing_result_parser() {
        let html = r#"
            <li class="b_algo">
                <h2><a href="https://example.com/b">Bing &amp; Result</a></h2>
                <div class="b_caption"><p>Useful snippet.</p></div>
            </li>
        "#;
        let results = Agent::parse_bing_results(html, 5);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Bing & Result");
        assert_eq!(results[0].url, "https://example.com/b");
        assert_eq!(results[0].snippet, "Useful snippet.");
    }

    #[test]
    fn test_react_workspace_glob_and_grep() {
        let _guard = WORKSPACE_ENV_LOCK.lock().expect("workspace env lock");
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(dir.path().join("src")).expect("mkdir");
        std::fs::write(
            dir.path().join("src/lib.rs"),
            "pub struct ToolAwareResponse;\n",
        )
        .expect("write");
        std::env::set_var("BEEBOTOS_WORKSPACE", dir.path());
        let agent = create_test_agent();

        let glob = agent
            .execute_workspace_glob("**/*.rs", ".", 20)
            .expect("glob");
        assert!(glob.contains("src/lib.rs"));

        let grep = agent
            .execute_workspace_grep("ToolAwareResponse", ".", Some("**/*.rs"), true, 20)
            .expect("grep");
        assert!(grep.contains("src/lib.rs:1:pub struct ToolAwareResponse;"));

        let outside = agent
            .resolve_tool_path("../outside.txt")
            .expect("outside path");
        assert!(outside.ends_with("outside.txt"));
        std::env::remove_var("BEEBOTOS_WORKSPACE");
    }

    #[test]
    fn test_react_workspace_exec_allows_paths_outside_workspace() {
        let _guard = WORKSPACE_ENV_LOCK.lock().expect("workspace env lock");
        let dir = tempfile::tempdir().expect("tempdir");
        std::env::set_var("BEEBOTOS_WORKSPACE", dir.path());
        let agent = create_test_agent();

        assert!(agent.validate_workspace_command("cat /etc/passwd").is_ok());
        assert!(agent
            .validate_workspace_command("cat ../outside.txt")
            .is_ok());

        let inside = dir.path().join("ok.txt");
        let command = format!("cat {}", inside.display());
        assert!(agent.validate_workspace_command(&command).is_ok());
        std::env::remove_var("BEEBOTOS_WORKSPACE");
    }

    #[tokio::test]
    async fn test_react_activate_skill_persists_session_context() {
        let registry = Arc::new(skills::SkillRegistry::new());
        registry
            .register_with_levels(
                test_loaded_skill("test-skill"),
                "test",
                vec!["testing".to_string()],
                Some("Test Skill: short".to_string()),
                Some("Test Skill summary".to_string()),
                Some("Full test skill context".to_string()),
            )
            .await;
        let agent = create_test_agent().with_skill_registry(registry);

        let result = agent
            .activate_skill_context("session-1", "test-skill")
            .await
            .expect("activate");
        assert!(result.contains("Activated skill"));

        let active = agent
            .active_skill_context_for_session("session-1")
            .await
            .expect("active context");
        assert!(active.contains("Full test skill context"));
        assert!(active.contains("`test-skill`"));
    }

    struct FakeSessionSearchProvider;

    #[async_trait::async_trait]
    impl crate::system_info::SystemInfoProvider for FakeSessionSearchProvider {
        async fn list_gateway_cron_jobs(
            &self,
        ) -> Result<Vec<crate::system_info::GatewayCronJobInfo>, String> {
            Ok(Vec::new())
        }

        async fn search_sessions(
            &self,
            query: &str,
            user_id: &str,
            limit: usize,
            exclude_session_id: Option<&str>,
        ) -> Result<Vec<crate::system_info::SessionSearchHit>, String> {
            assert_eq!(query, "memory layering");
            assert_eq!(user_id, "user-a");
            assert_eq!(limit, 5);
            assert_eq!(exclude_session_id, Some("current-session"));
            Ok(vec![crate::system_info::SessionSearchHit {
                message_id: "message-1".to_string(),
                session_id: "old-session".to_string(),
                session_title: "Old Chat".to_string(),
                role: "assistant".to_string(),
                content: "Use SessionDB for complete history and keep MEMORY.md stable."
                    .to_string(),
                created_at: "2026-05-21T10:00:00Z".to_string(),
                score: 0.91,
            }])
        }
    }

    #[tokio::test]
    async fn test_session_search_formats_provider_hits() {
        let agent =
            create_test_agent().with_system_info_provider(Arc::new(FakeSessionSearchProvider));

        let output = agent
            .execute_session_search("memory layering", "user-a", Some("current-session"), 5)
            .await
            .expect("session search");

        assert!(output.contains("old-session"));
        assert!(output.contains("assistant"));
        assert!(output.contains("SessionDB for complete history"));
        assert!(output.contains("0.91"));
    }

    // ============================================================================
    // Task Complexity Analysis Tests
    // ============================================================================

    #[tokio::test]
    async fn test_analyze_task_complexity_simple() {
        let agent = create_test_agent();
        let task = Task {
            id: "test-1".to_string(),
            task_type: TaskType::LlmChat,
            input: "Hello".to_string(),
            parameters: HashMap::new(),
            stream_tx: None,
        };

        assert_eq!(
            agent.analyze_task_complexity(&task).await,
            TaskComplexity::Simple
        );
    }

    #[tokio::test]
    async fn test_analyze_task_complexity_long_input() {
        let agent = create_test_agent();
        let task = Task {
            id: "test-2".to_string(),
            task_type: TaskType::LlmChat,
            input: "x".repeat(201), // > 200 chars
            parameters: HashMap::new(),
            stream_tx: None,
        };

        assert_eq!(
            agent.analyze_task_complexity(&task).await,
            TaskComplexity::Complex
        );
    }

    #[tokio::test]
    async fn test_analyze_task_complexity_multi_step_flag() {
        let agent = create_test_agent();
        let mut params = HashMap::new();
        params.insert("multi_step".to_string(), "true".to_string());

        let task = Task {
            id: "test-3".to_string(),
            task_type: TaskType::SkillExecution,
            input: "Short".to_string(),
            parameters: params,
            stream_tx: None,
        };

        assert_eq!(
            agent.analyze_task_complexity(&task).await,
            TaskComplexity::Complex
        );
    }

    #[tokio::test]
    async fn test_analyze_task_complexity_planning_types() {
        let agent = create_test_agent();

        // PlanCreation should always be Complex
        let plan_task = Task {
            id: "test-4".to_string(),
            task_type: TaskType::PlanCreation,
            input: "Short".to_string(),
            parameters: HashMap::new(),
            stream_tx: None,
        };

        assert_eq!(
            agent.analyze_task_complexity(&plan_task).await,
            TaskComplexity::Complex
        );

        // PlanExecution should always be Complex
        let exec_task = Task {
            id: "test-5".to_string(),
            task_type: TaskType::PlanExecution,
            input: "".to_string(),
            parameters: HashMap::new(),
            stream_tx: None,
        };

        assert_eq!(
            agent.analyze_task_complexity(&exec_task).await,
            TaskComplexity::Complex
        );

        // PlanAdaptation should always be Complex
        let adapt_task = Task {
            id: "test-6".to_string(),
            task_type: TaskType::PlanAdaptation,
            input: "Adapt".to_string(),
            parameters: HashMap::new(),
            stream_tx: None,
        };

        assert_eq!(
            agent.analyze_task_complexity(&adapt_task).await,
            TaskComplexity::Complex
        );
    }

    // ============================================================================
    // Should Use Planning Tests
    // ============================================================================

    #[tokio::test]
    async fn test_should_use_planning_not_ready() {
        let agent = create_test_agent(); // No planning engine
        let task = Task {
            id: "test-7".to_string(),
            task_type: TaskType::LlmChat,
            input: "x".repeat(300), // Complex task
            parameters: HashMap::new(),
            stream_tx: None,
        };

        // Should not use planning if not configured
        assert!(!agent.should_use_planning(&task).await);
    }

    #[tokio::test]
    async fn test_should_use_planning_complex_task() {
        let agent = create_test_agent_with_planning();
        let task = Task {
            id: "test-8".to_string(),
            task_type: TaskType::LlmChat,
            input: "x".repeat(300), // Complex task
            parameters: HashMap::new(),
            stream_tx: None,
        };

        // Should use planning for complex tasks
        assert!(agent.should_use_planning(&task).await);
    }

    #[tokio::test]
    async fn test_should_use_planning_explicit_override() {
        let agent = create_test_agent_with_planning();
        let mut params = HashMap::new();
        params.insert("use_planning".to_string(), "true".to_string());

        let task = Task {
            id: "test-9".to_string(),
            task_type: TaskType::LlmChat,
            input: "Short".to_string(), // Simple task
            parameters: params,
            stream_tx: None,
        };

        // Should use planning if explicitly requested
        assert!(agent.should_use_planning(&task).await);
    }

    // ============================================================================
    // Agent Configuration Tests
    // ============================================================================

    #[test]
    fn test_is_planning_ready_without_components() {
        let agent = create_test_agent();
        assert!(!agent.is_planning_ready());
        assert!(!agent.has_planning_engine());
        assert!(!agent.has_plan_executor());
        assert!(!agent.has_replanner());
    }

    #[test]
    fn test_is_planning_ready_with_components() {
        let agent = Agent::new(AgentConfig::default())
            .with_planning_engine(Arc::new(PlanningEngine::new()))
            .with_plan_executor(Arc::new(PlanExecutor::new()));

        assert!(agent.is_planning_ready());
        assert!(agent.has_planning_engine());
        assert!(agent.has_plan_executor());
    }

    // ============================================================================
    // Plan Management Tests
    // ============================================================================

    #[tokio::test]
    async fn test_create_and_get_plan() {
        let agent = create_test_agent_with_planning();

        let plan = agent
            .create_plan("Test goal", PlanStrategy::ReAct)
            .await
            .expect("Failed to create plan");

        // Verify plan exists
        let retrieved = agent.get_active_plan(&plan.id).await;
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().id, plan.id);
    }

    #[tokio::test]
    async fn test_list_active_plans() {
        let agent = create_test_agent_with_planning();

        // Initially empty
        let plans = agent.list_active_plans().await;
        assert!(plans.is_empty());

        // Create some plans
        let _plan1 = agent
            .create_plan("Goal 1", PlanStrategy::ReAct)
            .await
            .unwrap();
        let _plan2 = agent
            .create_plan("Goal 2", PlanStrategy::Hybrid)
            .await
            .unwrap();

        let plans = agent.list_active_plans().await;
        assert_eq!(plans.len(), 2);
    }

    #[tokio::test]
    async fn test_cancel_plan() {
        let agent = create_test_agent_with_planning();

        let plan = agent
            .create_plan("Test goal", PlanStrategy::ReAct)
            .await
            .unwrap();

        // Cancel plan
        agent
            .cancel_plan(&plan.id)
            .await
            .expect("Failed to cancel plan");

        // Verify removed
        assert!(agent.get_active_plan(&plan.id).await.is_none());
    }

    #[tokio::test]
    async fn test_cancel_nonexistent_plan() {
        let agent = create_test_agent_with_planning();

        let fake_id = PlanId::new();
        let result = agent.cancel_plan(&fake_id).await;

        assert!(result.is_err());
        match result.unwrap_err() {
            AgentError::NotFound(_) => (), // Expected
            other => panic!("Expected NotFound error, got {:?}", other),
        }
    }

    // ============================================================================
    // Error Handling Tests
    // ============================================================================

    #[tokio::test]
    async fn test_create_plan_without_engine() {
        let agent = create_test_agent(); // No planning engine

        let result = agent.create_plan("Test goal", PlanStrategy::ReAct).await;

        assert!(result.is_err());
        match result.unwrap_err() {
            AgentError::InvalidConfig(msg) => {
                assert!(msg.contains("Planning engine not configured"));
            }
            other => panic!("Expected InvalidConfig error, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_get_active_plan_nonexistent() {
        let agent = create_test_agent_with_planning();

        let fake_id = PlanId::new();
        let plan = agent.get_active_plan(&fake_id).await;

        assert!(plan.is_none());
    }

    // ============================================================================
    // Plan Strategy Selection Tests
    // ============================================================================

    #[test]
    fn test_select_plan_strategy_react() {
        let agent = create_test_agent();
        let mut params = HashMap::new();
        params.insert("strategy".to_string(), "react".to_string());

        let task = Task {
            id: "test-strategy".to_string(),
            task_type: TaskType::LlmChat,
            input: "Test".to_string(),
            parameters: params,
            stream_tx: None,
        };

        assert_eq!(agent.select_plan_strategy(&task), PlanStrategy::ReAct);
    }

    #[test]
    fn test_select_plan_strategy_cot() {
        let agent = create_test_agent();
        let mut params = HashMap::new();
        params.insert("strategy".to_string(), "cot".to_string());

        let task = Task {
            id: "test-strategy".to_string(),
            task_type: TaskType::LlmChat,
            input: "Test".to_string(),
            parameters: params,
            stream_tx: None,
        };

        assert_eq!(
            agent.select_plan_strategy(&task),
            PlanStrategy::ChainOfThought
        );
    }

    #[test]
    fn test_select_plan_strategy_default() {
        let agent = create_test_agent();
        let task = Task {
            id: "test-strategy".to_string(),
            task_type: TaskType::LlmChat,
            input: "Test".to_string(),
            parameters: HashMap::new(),
            stream_tx: None,
        };

        assert_eq!(agent.select_plan_strategy(&task), PlanStrategy::Hybrid);
    }
}
