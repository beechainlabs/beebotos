# 贡献指南

感谢您对 BeeBotOS 项目的关注！本指南将帮助您了解如何有效地参与项目贡献。

---

## 目录

1. [开始之前](#开始之前)
2. [开发环境搭建](#开发环境搭建)
3. [代码规范](#代码规范)
4. [提交规范](#提交规范)
5. [Pull Request 流程](#pull-request-流程)
6. [测试要求](#测试要求)
7. [文档贡献](#文档贡献)
8. [社区交流](#社区交流)

---

## 开始之前

### 行为准则

参与本项目即表示您同意遵守我们的 [行为准则](CODE_OF_CONDUCT.md)。核心原则：

- 互相尊重 - 以专业和尊重的态度对待每个人
- 欢迎新人 - 耐心帮助初学者
- 建设性反馈 - 提供有益的代码审查意见
- 包容多元 - 接纳不同的观点和经验

### 贡献类型

我们欢迎以下类型的贡献：

| 类型 | 说明 | 示例 |
|------|------|------|
| Bug 修复 | 修复代码缺陷 | 修复内存泄漏、逻辑错误 |
| 新功能 | 添加新特性 | 新增调度算法、API 端点 |
| 文档 | 改进文档 | 更新 API 文档、添加教程 |
| 代码风格 | 改进代码可读性 | 重构、格式化 |
| 性能优化 | 提升性能 | 优化内存分配、减少延迟 |
| 测试 | 添加测试用例 | 单元测试、集成测试 |

### 准备工作流程

```bash
# 1. Fork 本仓库到您的 GitHub 账户
# 点击页面右上角的 "Fork" 按钮

# 2. 克隆您的 Fork
git clone https://github.com/YOUR_USERNAME/beebotos.git
cd beebotos

# 3. 添加 upstream 远程仓库
git remote add upstream https://github.com/linc77/beebotos.git

# 4. 创建功能分支
git checkout -b feature/your-feature-name

# 5. 开始开发！
```

---

## 开发环境搭建

### 系统要求

| 组件 | 最低版本 | 推荐版本 |
|------|---------|---------|
| Rust | 1.75.0 | 1.78.0+ |
| Node.js | 18.0.0 | 20.0.0+ |
| Docker | 24.0.0 | 26.0.0+ |
| Git | 2.35.0 | 2.43.0+ |

### 安装依赖

```bash
# 安装 Rust (如果尚未安装)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env

# 安装 Foundry (Solidity 开发)
curl -L https://foundry.paradigm.xyz | bash
foundryup

# 安装 just (命令运行器)
cargo install just

# 验证安装
rustc --version
cargo --version
forge --version
```

### 项目初始化

```bash
# 克隆项目
git clone https://github.com/YOUR_USERNAME/beebotos.git
cd beebotos

# 下载依赖
cargo fetch

# 构建项目
cargo build --release

# 运行测试
cargo test --all
```

### 开发工作流

```bash
# 1. 同步上游代码
git checkout main
git pull upstream main
git push origin main

# 2. 创建功能分支
git checkout -b feature/my-awesome-feature

# 3. 开发过程中持续测试
cargo watch -x test

# 4. 提交前检查
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all

# 5. 提交更改
git add .
git commit -m "feat(kernel): add priority scheduler"

# 6. 推送到您的 Fork
git push origin feature/my-awesome-feature

# 7. 创建 Pull Request (通过 GitHub Web 界面)
```

---

## 代码规范

### Rust 代码规范

我们遵循 [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/) 和以下项目特定规范：

#### 代码格式化

```bash
# 自动格式化所有代码
cargo fmt --all

# 检查格式化 (CI 使用)
cargo fmt --all -- --check
```

#### 静态分析

```bash
# 运行 Clippy (严格模式)
cargo clippy --all-targets --all-features -- -D warnings
```

#### 文档注释规范

```rust
/// 创建一个具有指定优先级的新任务。
///
/// # 参数
///
/// * `name` - 任务的名称，用于日志和调试
/// * `priority` - 任务的优先级，决定调度顺序
///
/// # 返回值
///
/// 返回新创建任务的 TaskId，可用于后续操作
///
/// # 示例
///
/// ```
/// use beebotos::kernel::{Task, Priority};
/// use std::time::Duration;
///
/// let task = Task::new("data_processor", Priority::High, None);
/// ```
pub fn new(name: impl Into<String>, priority: Priority) -> Self {
    // 实现...
}
```

#### 命名规范

| 项目 | 规范 | 示例 |
|------|------|------|
| 类型 | PascalCase | `AgentRuntime`, `TaskScheduler` |
| 函数/方法 | snake_case | `spawn_agent`, `process_task` |
| 变量 | snake_case | `task_count`, `agent_id` |
| 常量 | SCREAMING_SNAKE_CASE | `MAX_AGENTS`, `TIME_SLICE_MS` |
| 模块 | snake_case | `kernel`, `agent_runtime` |

### Solidity 代码规范

我们遵循 [Solidity Style Guide](https://docs.soliditylang.org/en/latest/style-guide.html)：

#### 代码格式化

```bash
# 格式化合约代码
cd contracts/solidity
forge fmt

# 检查格式化
forge fmt --check
```

#### NatSpec 文档规范

```solidity
/// @title Agent Registry
/// @author BeeBotOS Team
/// @notice 管理智能体的注册、更新和查询
contract AgentRegistry {
    /// @notice 注册新智能体
    /// @param agentId 智能体的唯一标识符
    /// @param owner 智能体所有者的地址
    /// @return success 注册是否成功
    function register(bytes32 agentId, address owner) 
        external 
        returns (bool success) 
    {
        // 实现...
    }
}
```

---

## 提交规范

### Conventional Commits

我们使用 [Conventional Commits](https://www.conventionalcommits.org/) 规范：

```
<type>(<scope>): <subject>

<body>

<footer>
```

#### Type 类型

| Type | 说明 | 示例 |
|------|------|------|
| `feat` | 新功能 | `feat(kernel): add priority scheduler` |
| `fix` | Bug 修复 | `fix(contracts): prevent reentrancy` |
| `docs` | 文档更新 | `docs(api): update REST endpoints` |
| `refactor` | 代码重构 | `refactor(agents): simplify state machine` |
| `perf` | 性能优化 | `perf(memory): optimize allocator` |
| `test` | 测试相关 | `test(kernel): add scheduler tests` |
| `chore` | 构建/工具 | `chore(ci): update github actions` |

#### 提交示例

```bash
# 简单提交
git commit -m "feat(kernel): add priority scheduler"

# 详细提交
git commit -m "feat(kernel): add MLFQ priority scheduler

Implement Multi-Level Feedback Queue scheduler with 4 priority
levels (Critical, High, Normal, Low).

Closes #123"
```

---

## Pull Request 流程

### 提交前检查清单

- [ ] 代码已格式化 (`cargo fmt`)
- [ ] Clippy 无警告 (`cargo clippy`)
- [ ] 所有测试通过 (`cargo test`)
- [ ] 文档已更新
- [ ] 基于最新的 main 分支

### PR 标题格式

```
[TYPE][SCOPE] 简短描述

示例:
[FEAT][kernel] Add priority-based task scheduler
[FIX][contracts] Prevent reentrancy in payment
```

### 审查流程

1. **自动检查**: CI 必须全部通过
2. **代码审查**: 至少 2 名维护者批准
3. **解决冲突**: 保持分支与 main 同步
4. **合并**: 使用 "Squash and Merge"

---

## 测试要求

### 测试覆盖率要求

| 模块 | 最低覆盖率 | 目标覆盖率 |
|------|-----------|-----------|
| `kernel` | 85% | 90% |
| `brain` | 80% | 85% |
| `agents` | 80% | 85% |
| `chain` | 75% | 80% |
| `contracts` | 90% | 95% |

### 运行测试

```bash
# 所有测试
cargo test --all

# 特定 crate
cargo test -p beebotos-kernel

# 合约测试
cd contracts/solidity && forge test

# 代码覆盖率
cargo tarpaulin --out Html
```

### 编写测试

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_scheduler_priority() {
        let mut scheduler = Scheduler::new(Config::default());
        let task = Task::new("test", Priority::High);
        let id = scheduler.submit(task).unwrap();
        assert_eq!(scheduler.get_task(id).priority(), Priority::High);
    }
}
```

---

## 文档贡献

### 文档类型

| 类型 | 位置 | 说明 |
|------|------|------|
| API 文档 | 源码中的 `///` | 代码注释自动生成 |
| 用户文档 | `docs/` | 教程、指南 |
| 架构文档 | 根目录 `.md` | 设计说明 |
| README | 各 crate 根目录 | 模块说明 |

### 生成文档

```bash
# 生成并打开文档
cargo doc --all --open
```

---

## 社区交流

### 沟通渠道

| 渠道 | 用途 | 链接 |
|------|------|------|
| Discord | 实时讨论 | https://discord.gg/beebotos |
| GitHub Discussions | 技术讨论 | https://github.com/linc77/beebotos/discussions |
| GitHub Issues | Bug 报告 | https://github.com/linc77/beebotos/issues |
| Email | 私有咨询 | dev@beebotos.io |

### 获取帮助

如果您在贡献过程中遇到问题：

1. 查阅 [文档](https://docs.beebotos.io)
2. 搜索 [Issues](https://github.com/linc77/beebotos/issues)
3. 在 Discord 的 `#help` 频道提问

---

## 许可证

通过参与本项目，您同意您的贡献将按照 [MIT 许可证](LICENSE) 进行授权。

---

**感谢您对 BeeBotOS 的贡献！**
