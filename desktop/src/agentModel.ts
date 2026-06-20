export type AgentKind = "primary" | "subagent";

export interface AgentProfile {
  id: string;
  kind: AgentKind;
  parentId?: string;
  name: string;
  role: string;
  instructions: string;
}

export const defaultAgent: AgentProfile = {
  id: "primary",
  kind: "primary",
  name: "Telos Agent",
  role: "负责理解任务、调用工具并维护当前工作区上下文",
  instructions: "",
};
