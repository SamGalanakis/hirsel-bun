import { type Component, type JSX, splitProps } from "solid-js";
import { cva, type VariantProps } from "class-variance-authority";
import { cn } from "@/lib/cn";

const inputVariants = cva(
  "flex h-[38px] w-full border border-input bg-background px-3 py-2 text-sm text-foreground file:border-0 file:bg-transparent file:text-sm file:font-medium placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background disabled:cursor-not-allowed disabled:opacity-50",
);

type InputProps = JSX.InputHTMLAttributes<HTMLInputElement> &
  VariantProps<typeof inputVariants>;

const Input: Component<InputProps> = (props) => {
  const [local, others] = splitProps(props, ["class"]);

  return (
    <input
      class={cn(inputVariants(), local.class)}
      {...others}
    />
  );
};

export default Input;
export { inputVariants };
