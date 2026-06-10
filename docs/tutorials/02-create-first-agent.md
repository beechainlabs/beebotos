# Tutorial 2: Create Your First Agent

Learn how to create, configure, and deploy your first BeeBotOS agent.

## Overview

By the end of this tutorial, you will:
- Understand agent structure
- Create a custom agent configuration
- Implement agent behavior
- Deploy and interact with your agent
- Monitor its performance

## Prerequisites

- Completed [Tutorial 1: Quick Start](01-quick-start.md)
- Basic knowledge of Rust or JavaScript
- BeeBotOS CLI installed

## Step 1: Initialize Agent Project

Create a new agent project using the CLI:

```bash
beebotos agent init my-first-agent
cd my-first-agent
```

This creates the following structure:

```
my-first-agent/
├── agent.yaml          # Agent configuration
├── Cargo.toml          # Rust project file
├── src/
│   └── main.rs         # Agent implementation
├── skills/
│   └── hello.skill.yaml
└── tests/
    └── integration.rs
```

## Step 2: Configure Your Agent

Edit `agent.yaml`:

```yaml
name: my-first-agent
version: 0.1.0
description: My first BeeBotOS agent
author: Your Name

# Agent capabilities
capabilities:
  - text-generation
  - data-processing
  - api-integration

# Resource limits
resources:
  memory_mb: 512
  cpu_cores: 1
  
# LLM configuration
llm:
  provider: openai
  model: gpt-4o-mini
  temperature: 0.7
  max_tokens: 2048

# Personality (PAD model)
personality:
  pleasure: 0.5      # Friendly
  arousal: 0.3       # Moderate energy
  dominance: 0.2     # Cooperative
```

## Step 3: Implement Agent Behavior

### Basic Implementation (Rust)

```rust
use beebotos_agent::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct GreetInput {
    name: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct GreetOutput {
    message: String,
    sentiment: f64,
}

pub struct MyAgent {
    config: AgentConfig,
    context: AgentContext,
}

#[async_trait]
impl Agent for MyAgent {
    fn config(&self) -> &AgentConfig {
        &self.config
    }

    async fn initialize(&mut self, context: AgentContext) -> Result<()> {
        self.context = context;
        info!("MyAgent initialized");
        Ok(())
    }

    async fn handle_message(&mut self, message: Message) -> Result<Response> {
        match message.intent() {
            Intent::Greet => self.handle_greet(message).await,
            Intent::Query => self.handle_query(message).await,
            _ => self.handle_unknown(message).await,
        }
    }
}

impl MyAgent {
    async fn handle_greet(&self, message: Message) -> Result<Response> {
        let input: GreetInput = message.deserialize()?;
        
        // Use LLM to generate personalized greeting
        let prompt = format!(
            "Generate a friendly greeting for {}. Be warm and welcoming.",
            input.name
        );
        
        let llm_response = self.context.llm().complete(&prompt).await?;
        
        let output = GreetOutput {
            message: llm_response.text,
            sentiment: llm_response.sentiment,
        };
        
        Ok(Response::success(output))
    }

    async fn handle_query(&self, message: Message) -> Result<Response> {
        // Implement query handling
        let query: String = message.content();
        
        // Search knowledge base or use LLM
        let answer = self.context.knowledge().query(&query).await?;
        
        Ok(Response::success(answer))
    }

    async fn handle_unknown(&self, message: Message) -> Result<Response> {
        Ok(Response::clarification(
            "I'm not sure I understand. Could you rephrase that?"
        ))
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let agent = MyAgent {
        config: AgentConfig::from_file("agent.yaml")?,
        context: AgentContext::new(),
    };
    
    run_agent(agent).await
}
```

### Basic Implementation (JavaScript)

```javascript
import { Agent, Message, Response } from '@beebotos/agent-sdk';

export default class MyAgent extends Agent {
    constructor(config) {
        super(config);
        this.context = null;
    }

    async initialize(context) {
        this.context = context;
        console.log('MyAgent initialized');
    }

    async handleMessage(message) {
        const { intent, data } = message;
        
        switch (intent) {
            case 'greet':
                return this.handleGreet(data);
            case 'query':
                return this.handleQuery(data);
            default:
                return Response.clarification(
                    "I'm not sure I understand. Could you rephrase that?"
                );
        }
    }

    async handleGreet({ name }) {
        const prompt = `Generate a friendly greeting for ${name}. Be warm and welcoming.`;
        const { text, sentiment } = await this.context.llm.complete(prompt);
        
        return Response.success({
            message: text,
            sentiment,
        });
    }

    async handleQuery(query) {
        const answer = await this.context.knowledge.query(query);
        return Response.success(answer);
    }
}
```

## Step 4: Build and Test Locally

### Build the Agent

```bash
# Rust
cargo build --release

# JavaScript
npm install
npm run build
```

### Run Unit Tests

```bash
# Rust
cargo test

# JavaScript
npm test
```

### Test Locally

```bash
# Start local development server
beebotos agent serve

# In another terminal, test the agent
beebotos agent test --input '{"name": "Alice"}' --intent greet
```

## Step 5: Register Skills

Create a skill definition in `skills/hello.skill.yaml`:

```yaml
name: hello
version: 1.0.0
description: Simple greeting skill

triggers:
  - type: intent
    value: greet
  - type: keyword
    value: hello
  - type: keyword
    value: hi

parameters:
  - name: name
    type: string
    required: true
    description: Name of the person to greet

returns:
  type: object
  properties:
    message:
      type: string
    sentiment:
      type: number

examples:
  - input:
      name: Alice
    output:
      message: "Hello, Alice! Welcome!"
      sentiment: 0.85
```

Register the skill:

```bash
beebotos skill register skills/hello.skill.yaml
```

## Step 6: Deploy the Agent

### Deploy to Local Network

```bash
beebotos agent deploy --network local
```

### Deploy to Testnet

```bash
# Configure network
beebotos config set network.testnet.rpc https://testnet.monad.xyz

# Deploy
beebotos agent deploy --network testnet --gas-limit 5000000
```

### Deploy to Mainnet

```bash
beebotos agent deploy --network mainnet --confirmations 3
```

## Step 7: Interact with Your Agent

### Via CLI

```bash
# Send a message
beebotos agent message my-first-agent --input '{"name": "Bob"}'

# Check status
beebotos agent status my-first-agent

# View logs
beebotos agent logs my-first-agent --tail 100
```

### Via API

```bash
curl -X POST http://localhost:8080/api/v1/agents/my-first-agent/messages \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer YOUR_TOKEN" \
  -d '{
    "intent": "greet",
    "data": {
      "name": "Charlie"
    }
  }'
```

### Via WebSocket

```javascript
const ws = new WebSocket('ws://localhost:8080/ws/agents/my-first-agent');

ws.onopen = () => {
    ws.send(JSON.stringify({
        intent: 'greet',
        data: { name: 'Diana' }
    }));
};

ws.onmessage = (event) => {
    const response = JSON.parse(event.data);
    console.log(response.message);
};
```

## Step 8: Monitor Performance

### View Metrics

```bash
# Real-time metrics
beebotos metrics agent my-first-agent

# Historical data
beebotos metrics agent my-first-agent --from 2024-01-01 --to 2024-01-31
```

### Key Metrics

| Metric | Description | Target |
|--------|-------------|--------|
| Response Time | Average time to respond | < 500ms |
| Success Rate | Percentage of successful requests | > 99% |
| Token Usage | LLM tokens per request | < 500 |
| Memory Usage | RAM consumption | < 512 MB |

### Set Up Alerts

```yaml
# alerts.yaml
alerts:
  - name: high_error_rate
    condition: error_rate > 5%
    duration: 5m
    severity: warning
    
  - name: critical_response_time
    condition: response_time > 2000ms
    duration: 2m
    severity: critical
```

## Advanced Features

### Add Memory

```rust
use beebotos_agent::memory::{Memory, MemoryStore};

impl MyAgent {
    async fn remember_conversation(&self, user: &str, message: &str) -> Result<()> {
        let memory = Memory::new()
            .with_context("conversation")
            .with_content(message)
            .with_metadata("user", user);
        
        self.context.memory().store(memory).await?;
        Ok(())
    }

    async fn recall_context(&self, user: &str) -> Result<Vec<Memory>> {
        self.context.memory()
            .query()
            .with_context("conversation")
            .with_metadata("user", user)
            .limit(10)
            .execute()
            .await
    }
}
```

### Use Tools

```rust
use beebotos_agent::tools::ToolRegistry;

impl MyAgent {
    async fn register_tools(&mut self) -> Result<()> {
        self.context.tools().register(calculator::CalculatorTool)?;
        self.context.tools().register(weather::WeatherTool)?;
        self.context.tools().register(web_search::WebSearchTool)?;
        Ok(())
    }

    async fn handle_tool_call(&self, message: Message) -> Result<Response> {
        let result = self.context.tools()
            .execute(&message.tool_name(), message.tool_params())
            .await?;
        
        Ok(Response::success(result))
    }
}
```

### A2A Communication

```rust
use beebotos_agent::a2a::A2AClient;

impl MyAgent {
    async fn collaborate(&self, task: &str) -> Result<Response> {
        // Find suitable agent
        let agents = self.context.a2a()
            .discover(|cap| cap.contains("translation"))
            .await?;
        
        // Send task to agent
        let response = self.context.a2a()
            .send(&agents[0].id, Task::new(task))
            .await?;
        
        Ok(Response::success(response))
    }
}
```

## Troubleshooting

### Common Issues

**Agent fails to start:**
```bash
# Check configuration
beebotos agent validate agent.yaml

# View detailed logs
RUST_LOG=debug beebotos agent serve
```

**LLM API errors:**
```bash
# Verify API key
beebotos config get llm.api_key

# Test LLM connection
beebotos llm test --provider openai
```

**High memory usage:**
- Reduce `max_tokens` in configuration
- Implement memory limits in code
- Use streaming for large responses

## Next Steps

- [Tutorial 3: Multi-Agent Collaboration](03-multi-agent-collaboration.md)
- [Tutorial 4: Building Custom Skills](04-building-skills.md)
- [Agent Development Guide](../guides/agent-development.md)

## Complete Example

View the complete source code:
```bash
git clone https://github.com/beebotos/examples.git
cd examples/basic-agent
```

## Support

- [Documentation](https://docs.beebotos.dev)
- [Discord](https://discord.gg/beebotos)
- [GitHub Issues](https://github.com/linc77/beebotos/issues)

---

Congratulations! You've created your first BeeBotOS agent. Continue to [Tutorial 3](03-multi-agent-collaboration.md) to learn about multi-agent systems.
