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
    <div
      data-slot="progress"
      class={cn("z-progress-track h-1.5 w-full overflow-hidden bg-border", props.class)}
    >
      <div
        data-slot="progress-indicator"
        class={cn("z-progress-indicator h-full transition-all duration-300", props.indicatorClass)}
        style={{ width: `${Math.max(clamped(), 0)}%` }}
      />
    </div>
  );
};

export default Progress;
