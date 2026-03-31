import { type Component } from "solid-js";
import { cn } from "@/lib/cn";

interface ProgressProps {
  value: number;
  class?: string;
  indicatorClass?: string;
}

const Progress: Component<ProgressProps> = (props) => {
  const clamped = () => Math.max(0, Math.min(props.value, 100));

  return (
    <div class={cn("h-2 w-full overflow-hidden rounded-full bg-muted", props.class)}>
      <div
        class={cn("h-full transition-all duration-300", props.indicatorClass)}
        style={{ width: `${Math.max(clamped(), 0)}%` }}
      />
    </div>
  );
};

export default Progress;
