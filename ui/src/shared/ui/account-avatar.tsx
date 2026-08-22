import { cn } from "../lib/cn";

interface AccountAvatarProps {
  readonly name: string;
  readonly src?: string | null;
  readonly className?: string;
}

function avatarInitial(name: string): string {
  return Array.from(name.trim())[0]?.toLocaleUpperCase() ?? "C";
}

export function AccountAvatar({ name, src, className }: AccountAvatarProps) {
  return (
    <span
      className={cn(
        "grid size-8 shrink-0 place-items-center overflow-hidden rounded-full bg-[var(--codex-gray-800)] text-[11px] font-semibold text-white ring-1 ring-black/[.08]",
        className,
      )}
      aria-hidden="true"
    >
      {src ? (
        <img src={src} alt="" className="size-full object-cover" referrerPolicy="no-referrer" />
      ) : (
        avatarInitial(name)
      )}
    </span>
  );
}
