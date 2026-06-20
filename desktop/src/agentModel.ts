export type AgentKind = "primary" | "subagent";

export interface AgentProfile {
  id: string;
  kind: AgentKind;
  parentId?: string;
  name: string;
  role: string;
  instructions: string;
}

export interface ForkSubagentInput {
  id: string;
  name: string;
  role?: string;
  instructions?: string;
}

export const defaultAgent: AgentProfile = {
  id: "primary",
  kind: "primary",
  name: "Telos Agent",
  role: "负责理解任务、调用工具并维护当前工作区上下文",
  instructions: "",
};

export function forkSubagent(parent: AgentProfile, input: ForkSubagentInput): AgentProfile {
  return {
    id: input.id,
    kind: "subagent",
    parentId: parent.id,
    name: input.name.trim() || `${parent.name} Subagent`,
    role: input.role?.trim() || parent.role,
    instructions: input.instructions?.trim() || parent.instructions,
  };
}
