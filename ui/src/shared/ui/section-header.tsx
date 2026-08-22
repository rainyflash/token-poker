import type { ReactNode } from "react";

interface SectionHeaderProps {
  readonly eyebrow: string;
  readonly title: string;
  readonly description: string;
  readonly action?: ReactNode;
}

export function SectionHeader({ eyebrow, title, description, action }: SectionHeaderProps) {
  return (
    <header className="flex flex-col items-stretch gap-4 min-[720px]:flex-row min-[720px]:items-end min-[720px]:justify-between min-[720px]:gap-8">
      <div className="min-w-0">
        <p className="mb-2 text-[11px] font-semibold uppercase tracking-[0.12em] text-[var(--muted-light)]">
          {eyebrow}
        </p>
        <h1 className="text-[28px] font-semibold tracking-[-0.04em] text-[var(--ink)]">{title}</h1>
        <p className="mt-2 max-w-2xl text-[13px] leading-6 text-[var(--muted)]">{description}</p>
      </div>
      {action ? <div className="flex shrink-0 flex-wrap items-center gap-2">{action}</div> : null}
    </header>
  );
}
