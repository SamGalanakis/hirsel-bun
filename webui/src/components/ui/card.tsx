import { type Component, type JSX, splitProps } from "solid-js";
import { cn } from "@/lib/cn";

type CardProps = JSX.HTMLAttributes<HTMLDivElement>;

const Card: Component<CardProps> = (props) => {
  const [local, others] = splitProps(props, ["class"]);
  return (
    <div
      class={cn("border border-border bg-card text-card-foreground shadow-sm", local.class)}
      {...others}
    />
  );
};

const CardHeader: Component<CardProps> = (props) => {
  const [local, others] = splitProps(props, ["class"]);
  return <div class={cn("flex flex-col space-y-1.5 p-6", local.class)} {...others} />;
};

const CardTitle: Component<CardProps> = (props) => {
  const [local, others] = splitProps(props, ["class"]);
  return (
    <div class={cn("text-lg font-semibold leading-none tracking-tight", local.class)} {...others} />
  );
};

const CardDescription: Component<CardProps> = (props) => {
  const [local, others] = splitProps(props, ["class"]);
  return <div class={cn("text-sm text-muted-foreground", local.class)} {...others} />;
};

const CardContent: Component<CardProps> = (props) => {
  const [local, others] = splitProps(props, ["class"]);
  return <div class={cn("p-6 pt-0", local.class)} {...others} />;
};

const CardFooter: Component<CardProps> = (props) => {
  const [local, others] = splitProps(props, ["class"]);
  return <div class={cn("flex items-center p-6 pt-0", local.class)} {...others} />;
};

export default Card;
export { CardContent, CardDescription, CardFooter, CardHeader, CardTitle };
