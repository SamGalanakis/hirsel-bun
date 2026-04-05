import { type ButtonRootProps, Root } from "@kobalte/core/button";
import type { PolymorphicProps } from "@kobalte/core/polymorphic";
import { cva, type VariantProps } from "class-variance-authority";
import { type ComponentProps, Show, splitProps, type ValidComponent } from "solid-js";
import { cn } from "@/lib/cn";

const buttonVariants = cva(
  "group/button z-button inline-flex shrink-0 select-none items-center justify-center whitespace-nowrap outline-none transition-all disabled:pointer-events-none disabled:opacity-50 [&_svg]:pointer-events-none [&_svg]:shrink-0",
  {
    variants: {
      variant: {
        default: "z-button-variant-outline",
        primary: "z-button-variant-primary",
        outline: "z-button-variant-outline",
        secondary: "z-button-variant-secondary",
        ghost: "z-button-variant-ghost",
        destructive: "z-button-variant-destructive",
        link: "z-button-variant-link",
      },
      size: {
        default: "z-button-size-default",
        xs: "z-button-size-xs",
        sm: "z-button-size-sm",
        lg: "z-button-size-lg",
        icon: "z-button-size-icon",
        "icon-xs": "z-button-size-icon-xs",
        "icon-sm": "z-button-size-icon-sm",
        "icon-lg": "z-button-size-icon-lg",
      },
    },
    defaultVariants: {
      variant: "default",
      size: "default",
    },
  },
);

type ButtonProps<T extends ValidComponent = "button"> = PolymorphicProps<T, ButtonRootProps<T>> &
  VariantProps<typeof buttonVariants> &
  Pick<ComponentProps<T>, "class"> & {
    loading?: boolean;
  };

const Button = <T extends ValidComponent = "button">(props: ButtonProps<T>) => {
  const [local, others] = splitProps(props as ButtonProps, ["variant", "size", "class", "loading", "disabled", "children"]);
  return (
    <Root
      class={cn(buttonVariants({ variant: local.variant, size: local.size }), local.class)}
      data-slot="button"
      disabled={local.disabled || local.loading}
      {...others}
    >
      <Show when={local.loading}>
        <svg class="mr-1.5 h-3.5 w-3.5 animate-spin" viewBox="0 0 24 24" fill="none">
          <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4" />
          <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8v4a4 4 0 00-4 4H4z" />
        </svg>
      </Show>
      {local.children}
    </Root>
  );
};

export { Button, type ButtonProps, buttonVariants };
export default Button;
