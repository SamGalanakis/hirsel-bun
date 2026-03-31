import { type Component, type JSX, splitProps } from "solid-js";
import { cn } from "@/lib/cn";

type TextareaProps = JSX.TextareaHTMLAttributes<HTMLTextAreaElement>;

const Textarea: Component<TextareaProps> = (props) => {
  const [local, others] = splitProps(props, ["class"]);

  return (
    <textarea
      class={cn(
        "flex min-h-[80px] w-full border border-input bg-background px-3 py-2 text-sm text-foreground placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background disabled:cursor-not-allowed disabled:opacity-50",
        local.class,
      )}
      {...others}
    />
  );
};

export default Textarea;
