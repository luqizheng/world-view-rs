/* zcode-workflow
description: dwfrun-75e1eae3-3f98-4a50-9a7a-c7d68a489593
*/
interface RoutingSpec {
  /** One-sentence goal of the routing convention. */
  goal: string;
  /** Tasks routed to GLM, with the reason. */
  glmTasks: { task: string; reason: string }[];
  /** Tasks routed to MiniMax, with the reason. */
  minimaxTasks: { task: string; reason: string }[];
  /** Decision rules for routing at workflow time. */
  decisionRules: string[];
  /** Anti-patterns to avoid. */
  antiPatterns: string[];
  /** Cases where splitting adds no value. */
  whenNotToSplit: string[];
}

interface WrittenDoc {
  /** Workspace-relative path the markdown was written to. */
  path: string;
  /** First 200 chars of the markdown, for verification. */
  preview: string;
  /** Total character count. */
  length: number;
}

log("起一个 2-agent 小 workflow：GLM 设计 + MiniMax 落盘，验证多模型路由可用。");

phase("GLM 设计路由规约");
const architect = agent("路由架构师", {
  model: "account:bigmodel-individual-coding-plan/GLM-5.3$max",
  system:
    "你是 ZCode 多模型编排的架构顾问。回答必须结构化、引用具体的 ZCode 工具与参数名（CreateWorkflow、Agent、subagent_model、Skill、EvalWorkflowSnippet 等）。",
});
const spec = await architect.ask<RoutingSpec>(
  "为以下用户场景设计『模型路由规约 v0.1』：\n" +
    "— 用户希望『规划/设计用 GLM、写代码用 MiniMax』。\n" +
    "— 可用工具：CreateWorkflow（subagent_model 字段 + named agent）、Agent（无显式模型选择）、EvalWorkflowSnippet、AmendWorkflow、Skill。\n" +
    "— 可用模型：account:bigmodel-individual-coding-plan/GLM-5.3（max 推理）、minimax/MiniMax-M3（当前会话模型）。\n" +
    "— 关键约束：会话本身的模型不能由 agent 工具改；要切换模型必须显式派给 workflow 子任务。\n\n" +
    "请输出 JSON，覆盖：目标 / 何时走 GLM / 何时走 MiniMax / 决策规则 / 反模式 / 不应拆分的场景。",
);

phase("MiniMax 落盘规约文档");
const writer = agent("规约撰写人", {
  model: "minimax/MiniMax-M3",
  system:
    "你是技术文档作者。把上面的 JSON 规约写成完整 markdown，落到指定路径。中文，结构化、可直接评审。",
});
const targetPath = "docs/zcode-model-routing-v0.1.md";
const written = await writer.ask<WrittenDoc>(
  "把下面这份 JSON 规约（由上一阶段『路由架构师』subagent 用 GLM-5.3$max 设计）写成完整的 markdown 文档，落到 `" +
    targetPath +
    "`（Write 工具会自动创建父目录）。\n\n" +
    "JSON 规约原文（务必基于这份真实内容落盘，不要自行发挥或编造）：\n" +
    "```json\n" +
    JSON.stringify(spec, null, 2) +
    "\n```\n\n" +
    "建议结构：\n" +
    "1. 背景与目标\n" +
    "2. 模型清单与定位\n" +
    "3. 决策规则（核心）\n" +
    "4. 典型场景示例（GLM 规划 → MiniMax 编码 的 1–2 个工作流片段）\n" +
    "5. 反模式\n" +
    "6. 不应拆分的场景\n" +
    "7. 版本与变更\n\n" +
    "完成后用 Write 工具落盘，返回 { path, preview(前 200 字), length }。",
);

phase("验证并发布交付物");
const md = await files.read(targetPath);
const mdBytes = md.length;
await artifact.markdown("routing-spec", md, {
  title: "模型路由规约 v0.1",
  description: "GLM 规划 + MiniMax 编码的多模型编排规约",
  primary: true,
});

interface Finding {
  where: string;
  what: string;
  evidence: string;
  status: "verified" | "unconfirmed";
  severity: "low" | "medium" | "high";
}
interface WorkflowReport {
  conclusion: string;
  findings: Finding[];
  verified: string[];
  notCovered: string[];
}

const result: WorkflowReport = {
  conclusion: `模型路由规约 v0.1 已生成：GLM 设计路由策略、MiniMax 落盘 markdown 文档。路径：${written.path}（writer 报告 ${written.length} 字，UTF-8 字节 ${mdBytes}）。`,
  findings: [
    {
      where: written.path,
      what: "由 GLM 设计 + MiniMax 落盘的路由规约 v0.1",
      evidence: `路径=${written.path}, writer 报告字符数=${written.length}, UTF-8 字节=${mdBytes}, 预览前 200 字=${written.preview}`,
      status: "verified",
      severity: "low",
    },
  ],
  verified: [
    "GLM-5.3$max 模型已声明用于设计阶段（路由架构师）",
    "MiniMax-M3 模型已声明用于落盘阶段（规约撰写人）",
    `markdown 文档已落盘，UTF-8 字节 ${mdBytes}（writer 报告字符 ${written.length}）`,
    "markdown 已通过 artifact.markdown 发布为主交付物",
  ],
  notCovered: [
    "文档未做独立审读（两阶段 pipeline，未引入第三个 subagent）",
    "未跑构建/测试命令（本次交付物为文档）",
    "GLM 与 MiniMax 各自的成本/时延未量化",
  ],
};
return result;