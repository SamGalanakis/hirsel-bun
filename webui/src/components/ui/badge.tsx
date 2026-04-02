import { type Component, type JSX, splitProps } from "solid-js";
import { cva, type VariantProps } from "class-variance-authority";
import { cn } from "@/lib/cn";

const badgeVariants = cva(
  "inline-flex items-center border px-2.5 py-1 font-mono text-[10px] uppercase tracking-[0.14em] transition-colors",
  {
    variants: {
      variant: {
        default: "border-border bg-background text-muted-foreground",
        secondary: "border-border bg-muted text-foreground",
        success: "border-signal-green/30 bg-signal-green/10 text-signal-green",
        warning: "border-signal-amber/30 bg-signal-amber/10 text-signal-amber",
        destructive: "border-signal-red/30 bg-signal-red/10 text-signal-red",
      },
    },
    defaultVariants: {
      variant: "default",
    },
  },
);

type BadgeProps = JSX.HTMLAttributes<HTMLDivElement> &
  VariantProps<typeof badgeVariants>;

const Badge: Component<BadgeProps> = (props) => {
  const [local, others] = splitProps(props, ["class", "variant"]);

  return <div class={cn(badgeVariants({ variant: local.variant }), local.class)} {...others} />;
};

export default Badge;
export { badgeVariants };
