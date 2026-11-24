import {
  AppWindow,
  Binary,
  Bot,
  FileText,
  GitBranch,
  GitCommitHorizontal,
  Globe,
  Hammer,
  Hourglass,
  MessageSquareText,
  Pencil,
  Search,
  Send,
  Sparkles,
  SquareTerminal,
  Wrench,
  type LucideIcon,
} from "lucide-react"
import { ago, useNow, type Status } from "@/lib/agencia"
import type { Tone } from "@/lib/tone"
import { cn } from "@/lib/utils"

export const STEP_ICON: Record<string, LucideIcon> = {
  read: FileText,
  edit: Pencil,
  search: Search,
  run: SquareTerminal,
  build: Hammer,
  commit: GitCommitHorizontal,
  git: GitBranch,
  browser: AppWindow,
  web: Globe,
  agent: Bot,
  msg: Send,
  skill: Sparkles,
  re: Binary,
  tool: Wrench,
  say: MessageSquareText,
  wait: Hourglass,
}

export function Age({ t }: { t: number }) {
  const now = useNow()
  return <span className="tabular">{t ? ago(t, now) : ""}</span>
}

export function toneClasses(tone: Tone, paused?: boolean) {
  if (paused) {
    return {
      bar: "bg-ink-3",
      badge: "border-line bg-inset text-ink-2",
      text: "text-ink-2",
    }
  }
  switch (tone) {
    case "work":
      return {
        bar: "bg-work animate-sig-pulse",
        badge: "border-[var(--sig-work-border)] bg-[var(--sig-work-soft)] text-work",
        text: "text-work",
      }
    case "need":
      return {
        bar: "bg-need",
        badge: "border-[var(--sig-need-border)] bg-[var(--sig-need-soft)] text-need",
        text: "text-need",
      }
    case "bad":
      return {
        bar: "bg-bad",
        badge: "border-[var(--sig-bad-border)] bg-[var(--sig-bad-soft)] text-bad",
        text: "text-bad",
      }
    default:
      return {
        bar: "bg-idle/50",
        badge: "border-line bg-inset text-ink-3",
        text: "text-ink-3",
      }
  }
}

/** Architectural 3px vertical status bar — replaces generic circular status dots. */
export function SignalBar({ tone, paused, className }: { tone: Tone; paused?: boolean; className?: string }) {
  const c = toneClasses(tone, paused)
  return <span aria-hidden="true" className={cn("inline-block h-3.5 w-[3px] shrink-0 rounded-full", c.bar, className)} />
}

export function StatusTag({ label, tone, paused, className }: { label: string; tone: Tone; paused?: boolean; className?: string }) {
  const c = toneClasses(tone, paused)
  return (
    <span className={cn("inline-flex items-center gap-1.5 rounded-[5px] border px-1.5 py-0.5 text-[11px] font-medium leading-none", c.badge, className)}>
      <span className={cn("h-2 w-[2.5px] rounded-full", c.bar)} aria-hidden="true" />
      {label}
    </span>
  )
}

export function statusToTone(status: Status): Tone {
  if (status === "asks" || status === "waiting") return "need"
  if (status === "failed") return "bad"
  if (status === "working") return "work"
  return "idle"
}

export function Dot({ status, className }: { status: Status; className?: string }) {
  return <SignalBar tone={statusToTone(status)} className={className} />
}
