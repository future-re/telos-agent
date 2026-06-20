import { describe, expect, it } from "vitest";
import { groupConversationMessages } from "@/conversationView";

describe("groupConversationMessages", () => {
  it("groups thinking and assistant output into one assistant turn", () => {
    const groups = groupConversationMessages([
      { id: "u1", role: "user", content: "改一下 UI" },
      { id: "t1", role: "thinking", content: "我先检查组件。" },
      { id: "a1", role: "assistant", content: "已经调整完成。" },
    ]);

    expect(groups).toEqual([
      {
        id: "u1",
        role: "user",
        content: "改一下 UI",
      },
      {
        id: "assistant-t1",
        role: "assistant",
        thinking: "我先检查组件。",
        content: "已经调整完成。",
        streaming: undefined,
      },
    ]);
  });
});
