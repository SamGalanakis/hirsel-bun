import type { PolymorphicProps } from "@kobalte/core";
import {
  Content,
  List,
  Root,
  type TabsContentProps as TabsContentPrimitiveProps,
  type TabsListProps as TabsListPrimitiveProps,
  type TabsRootProps,
  type TabsTriggerProps as TabsTriggerPrimitiveProps,
  Trigger,
} from "@kobalte/core/tabs";
import { cva, type VariantProps } from "class-variance-authority";
import { type ComponentProps, mergeProps, splitProps, type ValidComponent } from "solid-js";
import { cn } from "@/lib/cn";

type TabsProps<T extends ValidComponent = "div"> = PolymorphicProps<T, TabsRootProps<T>> &
  Pick<ComponentProps<T>, "class" | "children">;

const Tabs = <T extends ValidComponent = "div">(props: TabsProps<T>) => {
  const mergedProps = mergeProps({ orientation: "horizontal" }, props);
  const [local, others] = splitProps(mergedProps, ["class", "orientation"]);
  return (
    <Root
      data-slot="tabs"
      data-orientation={local.orientation}
      orientation={local.orientation}
      class={cn("group/tabs z-tabs flex data-[orientation=horizontal]:flex-col", local.class)}
      {...others}
    />
  );
};

const tabsListVariants = cva(
  "group/tabs-list z-tabs-list inline-flex w-fit items-center justify-center text-muted-foreground",
  {
    variants: {
      variant: {
        default: "z-tabs-list-variant-default border-b border-border",
        line: "z-tabs-list-variant-line gap-1 bg-transparent border-b border-border",
      },
    },
    defaultVariants: {
      variant: "line",
    },
  },
);

type TabsListProps<T extends ValidComponent = "div"> = PolymorphicProps<
  T,
  TabsListPrimitiveProps<T>
> &
  VariantProps<typeof tabsListVariants> &
  Pick<ComponentProps<T>, "class" | "children">;

const TabsList = <T extends ValidComponent = "div">(props: TabsListProps<T>) => {
  const [local, others] = splitProps(props as TabsListProps, ["variant", "class"]);
  return (
    <List
      class={cn(tabsListVariants({ variant: local.variant }), local.class)}
      data-slot="tabs-list"
      data-variant={local.variant}
      {...others}
    />
  );
};

type TabTriggerProps<T extends ValidComponent = "button"> = PolymorphicProps<
  T,
  TabsTriggerPrimitiveProps<T>
> &
  Pick<ComponentProps<T>, "class" | "children">;

const TabsTrigger = <T extends ValidComponent = "button">(props: TabTriggerProps<T>) => {
  const [local, others] = splitProps(props as TabTriggerProps, ["class"]);
  return (
    <Trigger
      data-slot="tabs-trigger"
      class={cn(
        "z-tabs-trigger relative inline-flex items-center justify-center whitespace-nowrap font-mono text-[11px] uppercase tracking-[0.1em]",
        "text-muted-foreground transition-colors hover:text-foreground",
        "border-b-[2px] border-transparent -mb-px",
        "data-[selected]:text-foreground data-[selected]:border-foreground",
        "disabled:pointer-events-none disabled:opacity-50",
        local.class,
      )}
      {...others}
    />
  );
};

type TabsContentProps<T extends ValidComponent = "div"> = PolymorphicProps<
  T,
  TabsContentPrimitiveProps<T>
> &
  Pick<ComponentProps<T>, "class" | "children">;

const TabsContent = <T extends ValidComponent = "div">(props: TabsContentProps<T>) => {
  const [local, others] = splitProps(props as TabsContentProps, ["class"]);
  return (
    <Content
      data-slot="tabs-content"
      class={cn("z-tabs-content flex-1 pt-5 outline-none", local.class)}
      {...others}
    />
  );
};

export default Tabs;
export { Tabs, TabsList, TabsTrigger, TabsContent, tabsListVariants };
