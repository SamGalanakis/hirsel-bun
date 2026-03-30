import { type Component, type JSX, splitProps } from "solid-js";
import { cva, type VariantProps } from "class-variance-authority";
import { cn } from "@/lib/cn";

const badgeVariants = cva(
  "inline-flex items-center font-mono text-[10px] uppercase tracking-[0.12em] px-2 py-0.5 font-medium transition-colors",
  {
    variants: {
      variant: {
        default: "border border-border bg-background text-foreground",
        destructive: "border-transparent bg-destructive text-destructive-foreground",
        amber: "border-transparent bg-signal-amber/15 text-signal-amber",
        green: "border-transparent bg-signal-green/15 text-signal-green",
        blue: "border-transparent bg-signal-blue/15 text-signal-blue",
      },
      status: {
        working: "border-transparent bg-signal-amber/15 text-signal-amber",
        done: "border-transparent bg-signal-green/15 text-signal-green",
        failed: "border-transparent bg-destructive/15 text-destructive",
        idle: "border border-border bg-muted text-muted-foreground",
        queued: "border-transparent bg-signal-blue/15 text-signal-blue",
      },
    },
    defaultVariants: {
      variant: "default",
    },
  },
);

type BadgeProps = JSX.HTMLAttributes<HTMLSpanElement> &
  VariantProps<typeof badgeVariants>;

const Badge: Component<BadgeProps> = (props) => {
  const [local, others] = splitProps(props, ["class", "variant", "status"]);

  return (
    <span
      class={cn(
        badgeVariants({ variant: local.variant, status: local.status }),
        local.class,
      )}
      {...others}
    />
  );
};

export default Badge;
export { badgeVariants };
