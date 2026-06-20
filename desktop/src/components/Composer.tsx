import { FormEvent } from "react";
import { Send } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";

interface ComposerProps {
  value: string;
  disabled: boolean;
  disabledReason?: string;
  onChange: (value: string) => void;
  onSubmit: (event: FormEvent) => void;
}

export function Composer({ disabled, disabledReason, onChange, onSubmit, value }: ComposerProps) {
  return (
    <form
      className="grid w-full min-w-0 items-end gap-3 border-t bg-background/95 px-5 py-4 md:grid-cols-[minmax(0,1fr)_auto]"
      onSubmit={onSubmit}
    >
      <Textarea
        value={value}
        onChange={(event) => onChange(event.target.value)}
        placeholder={disabledReason ?? "让 telos 检查、解释、修改或验证..."}
        rows={3}
        className="min-h-24 resize-y bg-card"
      />
      <Button type="submit" disabled={disabled} className="w-full md:w-auto">
        <Send className="size-4" aria-hidden="true" />
        发送
      </Button>
    </form>
  );
}
