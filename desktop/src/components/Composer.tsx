import { FormEvent, useLayoutEffect, useRef } from "react";
import { ArrowUp, Square } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";

interface ComposerProps {
  value: string;
  sendDisabled: boolean;
  disabledReason?: string;
  running: boolean;
  onChange: (value: string) => void;
  onStop: () => void;
  onSubmit: (event: FormEvent) => void;
}

export function Composer({
  disabledReason,
  onChange,
  onStop,
  onSubmit,
  running,
  sendDisabled,
  value,
}: ComposerProps) {
  const textareaRef = useRef<HTMLTextAreaElement | null>(null);

  useLayoutEffect(() => {
    const textarea = textareaRef.current;
    if (!textarea) {
      return;
    }

    textarea.style.height = "0px";
    textarea.style.height = `${Math.min(textarea.scrollHeight, 112)}px`;
  }, [value]);

  return (
    <form
      className="w-full shrink-0 bg-transparent px-4 pb-4 pt-2 md:px-8"
      onSubmit={onSubmit}
    >
      <div className="mx-auto w-full max-w-[58rem]">
        <div className="flex min-h-14 items-center gap-3 rounded-xl border bg-card/95 px-3 py-2 shadow-[0_16px_36px_rgba(15,23,42,0.09)] transition-colors focus-within:border-ring/60">
          <Textarea
            ref={textareaRef}
            value={value}
            onChange={(event) => onChange(event.target.value)}
            onKeyDown={(event) => {
              if (
                event.key !== "Enter" ||
                event.shiftKey ||
                event.nativeEvent.isComposing
              ) {
                return;
              }
              event.preventDefault();
              if (!running && !sendDisabled) {
                event.currentTarget.form?.requestSubmit();
              }
            }}
            placeholder={disabledReason ?? "让 telos 检查、解释、修改或验证..."}
            rows={1}
            className="min-h-10 resize-none overflow-y-auto border-0 bg-transparent px-0 py-2 text-[15px] leading-6 shadow-none focus-visible:ring-0"
          />
          <Button
            type={running ? "button" : "submit"}
            disabled={!running && sendDisabled}
            onClick={running ? onStop : undefined}
            size="icon"
            variant={running ? "outline" : "default"}
            className="flex size-10 shrink-0 items-center justify-center rounded-md p-0"
            aria-label={running ? "停止当前任务" : "发送"}
          >
            {running ? (
              <Square className="size-4 fill-current" aria-hidden="true" />
            ) : (
              <ArrowUp className="size-4" aria-hidden="true" />
            )}
          </Button>
        </div>
      </div>
    </form>
  );
}
