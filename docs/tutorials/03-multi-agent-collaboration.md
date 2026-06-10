# Tutorial 3: Multi-Agent Collaboration

Learn how to create systems where multiple agents work together to accomplish complex tasks.

## Overview

By the end of this tutorial, you will:
- Understand multi-agent architecture
- Implement agent-to-agent (A2A) communication
- Design agent swarms for distributed tasks
- Use the Social Brain for coordination
- Handle conflicts and consensus

## Prerequisites

- Completed [Tutorial 2: Create Your First Agent](02-create-first-agent.md)
- Understanding of async/await in Rust or JavaScript
- Basic knowledge of distributed systems concepts

## Multi-Agent Concepts

### Agent Roles

```
┌─────────────────────────────────────────────────────────┐
│                    Agent Ecosystem                       │
├─────────────────────────────────────────────────────────┤
│                                                          │
│   ┌──────────┐  ┌──────────┐  ┌──────────┐             │
│   │  Leader  │  │ Planner  │  │ Executor │             │
│   │  Agent   │──│  Agent   │──│  Agent   │             │
│   └──────────┘  └──────────┘  └──────────┘             │
│        │                                              │
│        ▼                                              │
│   ┌──────────────────────────────────────┐          │
│   │         Specialized Agents            │          │
│   │  ┌─────┐ ┌─────┐ ┌─────┐ ┌─────┐   │          │
│   │  │Data │ │Code │ │Comms│ │Audit│   │          │
│   │  │Agent│ │Agent│ │Agent│ │Agent│   │          │
│   │  └─────┘ └─────┘ └─────┘ └─────┘   │          │
│   └──────────────────────────────────────┘          │
│                                                          │
└─────────────────────────────────────────────────────────┘
```

### Communication Patterns

1. **Direct Message**: One-to-one communication
2. **Broadcast**: One-to-many communication
3. **Publish-Subscribe**: Topic-based messaging
4. **Request-Reply**: Synchronous request/response
5. **Event-Driven**: Asynchronous event handling

## Step 1: Create a Multi-Agent System

### Initialize Project

```bash
beebotos system init research-team
cd research-team
```

### Define System Architecture

Create `system.yaml`:

```yaml
name: research-team
version: 1.0.0
description: Multi-agent system for research collaboration

agents:
  - name: coordinator
    type: leader
    capabilities:
      - task-delegation
      - conflict-resolution
      - progress-tracking
    config: agents/coordinator.yaml
    
  - name: researcher
    type: worker
    count: 3
    capabilities:
      - data-analysis
      - literature-review
      - hypothesis-generation
    config: agents/researcher.yaml
    
  - name: reviewer
    type: specialist
    capabilities:
      - quality-assurance
      - peer-review
      - validation
    config: agents/reviewer.yaml
    
  - name: writer
    type: specialist
    capabilities:
      - report-generation
      - summarization
      - documentation
    config: agents/writer.yaml

channels:
  - name: task-distribution
    type: broadcast
    participants: [coordinator, researcher]
    
  - name: results
    type: publish-subscribe
    participants: [researcher, reviewer, writer]
    
  - name: coordination
    type: direct
    participants: [coordinator]
```

## Step 2: Implement Coordinator Agent

```rust
use beebotos_agent::prelude::*;
use beebotos_agent::a2a::{A2AClient, AgentCapability, Task, TaskStatus};
use std::collections::HashMap;

pub struct CoordinatorAgent {
    config: AgentConfig,
    context: AgentContext,
    workers: Vec<AgentId>,
    active_tasks: HashMap<TaskId, TaskStatus>,
}

#[async_trait]
impl Agent for CoordinatorAgent {
    async fn handle_message(&mut self, message: Message) -> Result<Response> {
        match message.intent() {
            Intent::ResearchRequest => self.handle_research(message).await,
            Intent::TaskComplete => self.handle_completion(message).await,
            Intent::Conflict => self.handle_conflict(message).await,
            _ => self.delegate_to_specialist(message).await,
        }
    }
}

impl CoordinatorAgent {
    async fn handle_research(&self, request: Message) -> Result<Response> {
        let research_topic: String = request.content();
        
        // Step 1: Decompose research into subtasks
        let subtasks = self.plan_research(&research_topic).await?;
        
        // Step 2: Find capable agents for each subtask
        let assignments = self.assign_tasks(&subtasks).await?;
        
        // Step 3: Distribute tasks
        for (task, agent_id) in assignments {
            self.context.a2a()
                .send(&agent_id, Task::new(task))
                .await?;
        }
        
        // Step 4: Monitor progress
        let progress = self.monitor_progress().await?;
        
        Ok(Response::success(TaskPlanResponse {
            tasks_distributed: subtasks.len(),
            assigned_agents: assignments.len(),
            estimated_completion: progress.eta(),
        }))
    }

    async fn plan_research(&self, topic: &str) -> Result<Vec<SubTask>> {
        let prompt = format!(
            "Decompose the research topic '{}' into specific subtasks. \
             Each subtask should have a clear objective and deliverable.",
            topic
        );
        
        let plan = self.context.llm()
            .structured::<ResearchPlan>(&prompt)
            .await?;
        
        Ok(plan.subtasks)
    }

    async fn assign_tasks(
        &self,
        subtasks: &[SubTask],
    ) -> Result<Vec<(SubTask, AgentId)>> {
        let mut assignments = Vec::new();
        
        for task in subtasks {
            // Discover agents with required capabilities
            let candidates = self.context.a2a()
                .discover(|cap| task.required_capabilities.iter().all(|c| cap.contains(c)))
                .await?;
            
            // Select best candidate based on workload and performance
            let best = self.select_best_candidate(&candidates, task).await?;
            
            assignments.push((task.clone(), best.id));
        }
        
        Ok(assignments)
    }

    async fn monitor_progress(&self) -> Result<ProgressReport> {
        let mut completed = 0;
        let mut in_progress = 0;
        let mut blocked = 0;
        
        for (task_id, status) in &self.active_tasks {
            match status {
                TaskStatus::Completed => completed += 1,
                TaskStatus::InProgress => in_progress += 1,
                TaskStatus::Blocked => blocked += 1,
                _ => {}
            }
        }
        
        // Handle blocked tasks
        if blocked > 0 {
            self.resolve_blocked_tasks().await?;
        }
        
        Ok(ProgressReport {
            completed,
            in_progress,
            blocked,
            total: self.active_tasks.len(),
        })
    }

    async fn handle_conflict(&self, message: Message) -> Result<Response> {
        let conflict: ConflictReport = message.deserialize()?;
        
        // Analyze conflict type
        let resolution = match conflict.conflict_type {
            ConflictType::ResourceContention => {
                self.resolve_resource_conflict(&conflict).await?
            }
            ConflictType::Disagreement => {
                self.resolve_disagreement(&conflict).await?
            }
            ConflictType::Dependency => {
                self.resolve_dependency(&conflict).await?
            }
        };
        
        Ok(Response::success(resolution))
    }

    async fn resolve_disagreement(&self, conflict: &ConflictReport) -> Result<Resolution> {
        // Gather evidence from all parties
        let evidence = self.gather_evidence(&conflict.parties).await?;
        
        // Use consensus mechanism
        let consensus = self.context.social_brain()
            .consensus()
            .propose(ConsensusProposal {
                topic: conflict.topic.clone(),
                options: evidence,
                threshold: 0.66, // 2/3 majority
            })
            .await?;
        
        Ok(Resolution::Consensus(consensus.result))
    }
}
```

## Step 3: Implement Worker Agent

```rust
use beebotos_agent::prelude::*;

pub struct ResearcherAgent {
    config: AgentConfig,
    context: AgentContext,
    specialization: ResearchSpecialization,
}

#[async_trait]
impl Agent for ResearcherAgent {
    async fn handle_message(&mut self, message: Message) -> Result<Response> {
        match message.intent() {
            Intent::Task => self.execute_task(message).await,
            Intent::Query => self.answer_query(message).await,
            Intent::Collaborate => self.collaborate(message).await,
            _ => Ok(Response::error("Unknown intent")),
        }
    }
}

impl ResearcherAgent {
    async fn execute_task(&self, message: Message) -> Result<Response> {
        let task: ResearchTask = message.deserialize()?;
        
        // Acknowledge task receipt
        self.send_status_update(&task.id, TaskStatus::InProgress).await?;
        
        // Execute based on task type
        let result = match task.task_type {
            TaskType::LiteratureReview => {
                self.perform_literature_review(&task).await
            }
            TaskType::DataAnalysis => {
                self.perform_data_analysis(&task).await
            }
            TaskType::HypothesisGeneration => {
                self.generate_hypotheses(&task).await
            }
        };
        
        // Report completion
        match result {
            Ok(output) => {
                self.send_status_update(&task.id, TaskStatus::Completed).await?;
                self.submit_results(&task.id, output).await?;
                Ok(Response::success("Task completed"))
            }
            Err(e) => {
                self.send_status_update(&task.id, TaskStatus::Failed(e.to_string())).await?;
                Ok(Response::error(format!("Task failed: {}", e)))
            }
        }
    }

    async fn collaborate(&self, message: Message) -> Result<Response> {
        let request: CollaborationRequest = message.deserialize()?;
        
        // Check if we have the required expertise
        if !self.has_expertise(&request.required_skills) {
            // Forward to more suitable agent
            let better_agent = self.find_expert(&request.required_skills).await?;
            self.context.a2a()
                .forward(message, &better_agent.id)
                .await?;
            return Ok(Response::success("Forwarded to specialist"));
        }
        
        // Collaborate on the task
        let contribution = self.contribute(&request).await?;
        
        // Share results with requesting agent
        self.context.a2a()
            .send(&request.requester, CollaborationResponse {
                contribution,
                confidence: self.calculate_confidence(&contribution),
            })
            .await?;
        
        Ok(Response::success("Collaboration complete"))
    }

    async fn perform_literature_review(&self, task: &ResearchTask) -> Result<ReviewResult> {
        // Search academic databases
        let papers = self.context.tools()
            .use_tool::<SearchTool>("search_papers", &task.query)
            .await?;
        
        // Analyze and synthesize
        let synthesis = self.synthesize_findings(&papers).await?;
        
        // Identify gaps
        let gaps = self.identify_gaps(&synthesis).await?;
        
        Ok(ReviewResult {
            papers_reviewed: papers.len(),
            synthesis,
            gaps,
        })
    }
}
```

## Step 4: Configure Communication

### Message Protocol

```rust
#[derive(Debug, Serialize, Deserialize)]
pub enum A2AMessage {
    Task(TaskMessage),
    Result(ResultMessage),
    Query(QueryMessage),
    Event(EventMessage),
    Coordination(CoordinationMessage),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TaskMessage {
    pub task_id: TaskId,
    pub task_type: TaskType,
    pub payload: serde_json::Value,
    pub priority: Priority,
    pub deadline: Option<Timestamp>,
    pub dependencies: Vec<TaskId>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CoordinationMessage {
    pub coordination_type: CoordinationType,
    pub proposal: Option<Proposal>,
    pub vote: Option<Vote>,
    pub consensus: Option<Consensus>,
}
```

### Channel Configuration

```rust
use beebotos_agent::messaging::{Channel, ChannelType, MessageFilter};

pub fn setup_channels() -> Vec<Channel> {
    vec![
        Channel {
            name: "high-priority".to_string(),
            channel_type: ChannelType::PriorityQueue,
            filter: MessageFilter::Priority(Priority::High..=Priority::Critical),
            max_size: 1000,
        },
        Channel {
            name: "broadcast".to_string(),
            channel_type: ChannelType::Broadcast,
            filter: MessageFilter::Any,
            max_size: 10000,
        },
        Channel {
            name: "direct".to_string(),
            channel_type: ChannelType::Direct,
            filter: MessageFilter::Destination(DestinationType::Single),
            max_size: 5000,
        },
    ]
}
```

## Step 5: Deploy Multi-Agent System

### Local Testing

```bash
# Start all agents
beebotos system start --config system.yaml

# Monitor interactions
beebotos system monitor research-team
```

### Distributed Deployment

```yaml
# deployment.yaml
version: v1
kind: MultiAgentDeployment

metadata:
  name: research-team
  namespace: production

spec:
  coordinator:
    replicas: 1
    resources:
      memory: 1Gi
      cpu: 1000m
      
  researcher:
    replicas: 3
    resources:
      memory: 2Gi
      cpu: 2000m
      
  reviewer:
    replicas: 1
    resources:
      memory: 1Gi
      cpu: 1000m
      
  writer:
    replicas: 1
    resources:
      memory: 1Gi
      cpu: 1000m

  networking:
    mesh:
      enabled: true
      protocol: a2a-v1
    
  persistence:
    state_store: etcd
    message_queue: kafka
```

Deploy:

```bash
beebotos deploy -f deployment.yaml
```

## Step 6: Monitor and Debug

### View System State

```bash
# List all agents in system
beebotos system agents research-team

# View message flow
beebotos system messages research-team --channel results

# Check agent health
beebotos system health research-team
```

### Debug Communication

```rust
// Add tracing to messages
impl A2AClient {
    async fn send_traced(&self, message: A2AMessage) -> Result<()> {
        let span = tracing::info_span!(
            "a2a_send",
            message_id = %message.id(),
            from = %self.agent_id,
            to = %message.destination(),
        );
        
        async {
            tracing::info!("Sending A2A message");
            
            let result = self.send(message).await;
            
            match &result {
                Ok(_) => tracing::info!("Message sent successfully"),
                Err(e) => tracing::error!(error = %e, "Failed to send message"),
            }
            
            result
        }
        .instrument(span)
        .await
    }
}
```

## Advanced Topics

### Swarm Intelligence

```rust
pub struct AgentSwarm {
    agents: Vec<Box<dyn Agent>>,
    social_brain: Arc<SocialBrain>,
}

impl AgentSwarm {
    /// Collective decision making
    pub async fn collective_decision(&self, proposal: Proposal) -> Result<Decision> {
        // Gather individual opinions
        let opinions: Vec<Opinion> = futures::future::join_all(
            self.agents.iter().map(|agent| agent.evaluate(proposal.clone()))
        ).await;
        
        // Apply swarm consensus algorithm
        let consensus = self.social_brain
            .swarm_consensus()
            .aggregate(opinions)
            .with_strategy(ConsensusStrategy::WeightedByReputation)
            .execute()
            .await?;
        
        Ok(consensus.decision)
    }

    /// Self-organization for task allocation
    pub async fn self_organize(&mut self, tasks: Vec<Task>) -> Result<()> {
        for task in tasks {
            // Agents bid on tasks based on capability match
            let bids: Vec<Bid> = futures::future::join_all(
                self.agents.iter().map(|agent| agent.bid(&task))
            ).await;
            
            // Assign to best bidder
            let winner = bids.iter().max_by_key(|b| b.score).unwrap();
            winner.agent.accept_task(task).await?;
        }
        
        Ok(())
    }
}
```

### Conflict Resolution

```rust
impl SocialBrain {
    /// Mediate conflicts between agents
    pub async fn mediate(&self, conflict: Conflict) -> Result<Resolution> {
        match conflict.severity {
            Severity::Low => self.negotiate(conflict).await,
            Severity::Medium => self.arbitrate(conflict).await,
            Severity::High => self.escalate_to_human(conflict).await,
        }
    }

    /// Negotiation protocol
    async fn negotiate(&self, conflict: Conflict) -> Result<Resolution> {
        let mut rounds = 0;
        let max_rounds = 10;
        
        while rounds < max_rounds {
            // Each party makes an offer
            let offers = self.gather_offers(&conflict.parties).await?;
            
            // Check for agreement
            if let Some(agreement) = self.find_overlap(&offers) {
                return Ok(Resolution::Agreement(agreement));
            }
            
            // Generate counter-proposals
            self.generate_counters(&mut conflict).await?;
            rounds += 1;
        }
        
        // Fallback to arbitration
        self.arbitrate(conflict).await
    }
}
```

## Best Practices

1. **Design for failure**: Agents may go offline; design for graceful degradation
2. **Use timeouts**: Always set timeouts for inter-agent communication
3. **Idempotent operations**: Ensure messages can be safely retried
4. **Clear responsibilities**: Each agent should have a well-defined role
5. **Monitor closely**: Use distributed tracing for debugging
6. **Rate limiting**: Prevent message storms with rate limiting
7. **Backpressure**: Handle overload gracefully

## Complete Example

View the complete multi-agent system:

```bash
git clone https://github.com/beebotos/examples.git
cd examples/research-team
beebotos system start --config system.yaml
```

## Next Steps

- [Tutorial 4: Building Custom Skills](04-building-skills.md)
- [A2A Protocol Specification](../specs/a2a-protocol.md)
- [Social Brain Documentation](../architecture/social-brain.md)

## Support

- [Discord #multi-agent channel](https://discord.gg/beebotos)
- [GitHub Discussions](https://github.com/linc77/beebotos/discussions)

---

Congratulations! You've built a multi-agent system. Your agents can now collaborate, coordinate, and solve complex problems together.
