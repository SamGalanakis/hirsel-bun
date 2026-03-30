import { type Component, type JSX, splitProps } from "solid-js";
import { cva, type VariantProps } from "class-variance-authority";
import { cn } from "@/lib/cn";

const textareaVariants = cva(
  "flex min-h-[80px] w-full border border-input bg-background px-3 py-2 text-sm text-foreground placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background disabled:cursor-not-allowed disabled:opacity-50",
);

type TextareaProps = JSX.TextareaHTMLAttributes<HTMLTextAreaElement> &
  VariantProps<typeof textareaVariants>;

const Textarea: Component<TextareaProps> = (props) => {
  const [local, others] = splitProps(props, ["class"]);

  return (
    <textarea
      class={cn(textareaVariants(), local.class)}
      {...others}
    />
  );
};

export default Textarea;
export { textareaVariants };
