/**
 * ProjectIcon - Displays a project's icon (favicon) or fallback initials
 */
import { type Component, Show, createSignal } from 'solid-js';

interface ProjectIconProps {
  name: string;
  icon?: string | null;
  size?: number;
  class?: string;
}

/** Extract up to 2 initials from a project name */
function getInitials(name: string): string {
  const words = name.trim().split(/[\s\-_./]+/).filter(Boolean);
  if (words.length >= 2) {
    return (words[0][0] + words[1][0]).toUpperCase();
  }
  return name.slice(0, 2).toUpperCase();
}

/** Deterministic hue from a string for the initials background */
function nameToHue(name: string): number {
  let hash = 0;
  for (let i = 0; i < name.length; i++) {
    hash = name.charCodeAt(i) + ((hash << 5) - hash);
  }
  return Math.abs(hash) % 360;
}

export const ProjectIcon: Component<ProjectIconProps> = (props) => {
  const [imgError, setImgError] = createSignal(false);
  const size = () => props.size ?? 24;
  const hasIcon = () => !!props.icon && !imgError();
  const hue = () => nameToHue(props.name);

  return (
    <div
      class={`flex items-center justify-center shrink-0 overflow-hidden ${props.class ?? ''}`}
      style={{
        width: `${size()}px`,
        height: `${size()}px`,
        ...(!hasIcon()
          ? {
              background: `hsl(${hue()}, 20%, 18%)`,
              border: `1px solid hsl(${hue()}, 20%, 28%)`,
            }
          : {}),
      }}
    >
      <Show
        when={hasIcon()}
        fallback={
          <span
            class="text-wool-400 font-medium leading-none select-none"
            style={{ 'font-size': `${Math.max(8, size() * 0.38)}px`, 'letter-spacing': '0.02em' }}
          >
            {getInitials(props.name)}
          </span>
        }
      >
        <img
          src={props.icon!}
          alt=""
          class="w-full h-full object-cover"
          onError={() => setImgError(true)}
          loading="lazy"
          referrerpolicy="no-referrer"
        />
      </Show>
    </div>
  );
};
