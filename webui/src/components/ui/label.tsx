import { type Component, type JSX, splitProps } from "solid-js";
import { cva, type VariantProps } from "class-variance-authority";
import { cn } from "@/lib/cn";

const labelVariants = cva(
  "font-mono text-[10px] uppercase tracking-[0.12em] text-muted-foreground peer-disabled:cursor-not-allowed peer-disabled:opacity-70",
);

type LabelProps = JSX.LabelHTMLAttributes<HTMLLabelElement> &
  VariantProps<typeof labelVariants>;

const Label: Component<LabelProps> = (props) => {
  const [local, others] = splitProps(props, ["class"]);

  return (
    <label
      class={cn(labelVariants(), local.class)}
      {...others}
    />
  );
};

export default Label;
export { labelVariants };
