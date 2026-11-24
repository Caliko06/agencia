import { useState } from "react"
import {
  Activity,
  Binary,
  Command,
  Inbox,
  Layers,
  MessagesSquare,
  Moon,
  MoreHorizontal,
  Network,
  Plus,
  Sun,
  Target,
  TerminalSquare,
} from "lucide-react"
import { Popover, Tooltip } from "radix-ui"
import type { LiveStatus } from "@/lib/agencia"
import { cn } from "@/lib/utils"

export const VIEWS = ["bandeja", "consola", "red", "tablero", "slices", "lupa"] as const
export type View = (typeof VIEWS)[number]

const NAV_ITEMS: { id: View; label: string; kbd: string; icon: typeof Inbox }[] = [
  { id: "bandeja", label: "Bandeja de prioridades", kbd: "I", icon: Inbox },
  { id: "consola", label: "Consola de procesos", kbd: "V", icon: TerminalSquare },
  { id: "red", label: "Red y topología", kbd: "G", icon: Network },
  { id: "tablero", label: "Tablero de metas", kbd: "T", icon: Target },
  { id: "slices", label: "Slices (Agencia Real)", kbd: "S", icon: Layers },
  { id: "lupa", label: "Lupa binaria (RE)", kbd: "R", icon: Binary },
]

function RailButton({
  label,
  kbd,
  active,
  badge,
  accent,
  onClick,
  children,
}: {
  label: string
  kbd?: string
  active?: boolean
  badge?: number
  accent?: boolean
  onClick: () => void
  children: React.ReactNode
}) {
  return (
    <Tooltip.Root>
      <Tooltip.Trigger asChild>
        <button
          type="button"
          onClick={onClick}
          aria-label={label}
          aria-pressed={active}
          className={cn(
            "pressable relative grid size-11 place-items-center rounded-xl border",
            accent
              ? "ag-grad-prime border-transparent"
              : active
                ? "ag-grad-elevated border-line-strong text-ink-1 shadow-[var(--shadow-sm)]"
                : "border-transparent bg-transparent text-ink-2 hover:bg-hover hover:text-ink-1",
          )}
        >
          {active && !accent && (
            <span className="absolute -bottom-1 left-2.5 right-2.5 h-[2.5px] rounded-full bg-ink-1 md:top-2 md:right-auto md:bottom-2 md:-left-[9px] md:h-auto md:w-[3px] md:rounded-r-full" aria-hidden="true" />
          )}
          {children}
          {!!badge && badge > 0 && (
            <span className="absolute -top-1 -right-1 grid min-w-[17px] place-items-center rounded-full bg-need px-1 font-mono text-[10px] font-bold leading-[17px] text-[var(--sig-need-ink)] tabular">
              {badge > 99 ? "99" : badge}
            </span>
          )}
        </button>
      </Tooltip.Trigger>
      <Tooltip.Portal>
        <Tooltip.Content
          side="right"
          sideOffset={10}
          className="anim-tooltip z-50 flex items-center gap-2 rounded-md border border-line-strong bg-elevated px-2.5 py-1.5 text-[12px] font-medium text-ink-1 shadow-[var(--shadow-md)]"
        >
          <span>{label}</span>
          {kbd && (
            <kbd className="rounded border border-line bg-inset px-1.5 py-0.5 font-mono text-[10px] text-ink-3">
              {kbd}
            </kbd>
          )}
        </Tooltip.Content>
      </Tooltip.Portal>
    </Tooltip.Root>
  )
}

export function NavRail({
  view,
  setView,
  needAll,
  live,
  dark,
  onToggleTheme,
  onLaunch,
  onConsultor,
  onActivity,
  onPalette,
}: {
  view: View
  setView: (v: View) => void
  needAll: number
  live: LiveStatus
  dark: boolean
  onToggleTheme: () => void
  onLaunch: () => void
  onConsultor: () => void
  onActivity: () => void
  onPalette: () => void
}) {
  const [moreOpen, setMoreOpen] = useState(false)
  const liveLabel =
    live === "connecting"
      ? "Conectando al flujo en vivo"
      : live === "sse"
        ? "Conectado en vivo"
        : live === "poll"
          ? "Sondeo activo"
          : live === "sleep"
            ? "En reposo"
            : "Sin conexión"

  const mobileAct = (fn: () => void) => () => {
    setMoreOpen(false)
    fn()
  }

  return (
    <aside
      aria-label="Navegación principal"
      className="ag-grad-rail order-last flex h-14 w-full shrink-0 flex-row items-center justify-around border-t border-line px-1.5 pb-[env(safe-area-inset-bottom)] md:order-first md:h-auto md:w-[68px] md:flex-col md:justify-start md:border-t-0 md:border-r md:px-0 md:py-3.5 md:pb-3.5"
    >
      {/* Geometric Brand Mark (desktop rail) */}
      <button
        type="button"
        onClick={() => setView("bandeja")}
        title={`Agencia Studio (${liveLabel})`}
        className="pressable ag-grad-elevated group relative mb-4 hidden size-11 place-items-center rounded-xl border border-line-strong font-mono text-[14px] font-bold tracking-tighter text-ink-1 shadow-[var(--shadow-sm)] md:grid"
      >
        AG
        <span
          aria-label={liveLabel}
          className={cn(
            "absolute -right-0.5 -bottom-0.5 size-2.5 rounded-full border-2 border-rail",
            live === "sse"
              ? "bg-ok"
              : live === "poll"
                ? "bg-need"
                : live === "connecting"
                  ? "bg-idle animate-sig-pulse"
                  : live === "sleep"
                    ? "bg-idle"
                    : "bg-bad",
          )}
        />
      </button>

      {/* Primary View Stations */}
      <nav className="flex flex-1 flex-row items-center justify-around gap-0.5 md:flex-none md:flex-col md:justify-start md:gap-1.5">
        {NAV_ITEMS.map((item) => {
          const Icon = item.icon
          return (
            <RailButton
              key={item.id}
              label={item.label}
              kbd={item.kbd}
              active={view === item.id}
              badge={item.id === "bandeja" ? needAll : 0}
              onClick={() => setView(item.id)}
            >
              <Icon className="size-[18px]" strokeWidth={1.75} />
            </RailButton>
          )
        })}

        {/* Compact "Más" Menu (<768px mobile) */}
        <Popover.Root open={moreOpen} onOpenChange={setMoreOpen}>
          <Popover.Trigger asChild>
            <button
              type="button"
              aria-label="Más acciones"
              aria-expanded={moreOpen}
              className="pressable relative grid size-11 place-items-center rounded-xl border border-transparent text-ink-2 hover:bg-hover hover:text-ink-1 md:hidden"
            >
              <MoreHorizontal className="size-[18px]" strokeWidth={1.75} />
            </button>
          </Popover.Trigger>
          <Popover.Portal>
            <Popover.Content
              side="top"
              align="end"
              sideOffset={8}
              className="anim-modal z-50 w-56 rounded-xl border border-line-strong bg-surface p-1.5 shadow-[var(--shadow-lg)] md:hidden"
            >
              <button
                type="button"
                onClick={mobileAct(onLaunch)}
                className="pressable ag-grad-prime flex w-full items-center gap-2.5 rounded-lg px-3 py-2 text-left text-[13px] font-medium"
              >
                <Plus className="size-4 shrink-0" strokeWidth={2.1} />
                <span className="flex-1">Lanzar agente</span>
              </button>
              <button
                type="button"
                onClick={mobileAct(onConsultor)}
                className="pressable mt-1 flex w-full items-center gap-2.5 rounded-lg px-3 py-2 text-left text-[13px] text-ink-1 hover:bg-hover"
              >
                <MessagesSquare className="size-4 shrink-0 text-ink-2" strokeWidth={1.75} />
                <span className="flex-1">Consultor</span>
              </button>
              <button
                type="button"
                onClick={mobileAct(onActivity)}
                className="pressable flex w-full items-center gap-2.5 rounded-lg px-3 py-2 text-left text-[13px] text-ink-1 hover:bg-hover"
              >
                <Activity className="size-4 shrink-0 text-ink-2" strokeWidth={1.75} />
                <span className="flex-1">Actividad 24h</span>
              </button>
              <button
                type="button"
                onClick={mobileAct(onPalette)}
                className="pressable flex w-full items-center gap-2.5 rounded-lg px-3 py-2 text-left text-[13px] text-ink-1 hover:bg-hover"
              >
                <Command className="size-4 shrink-0 text-ink-2" strokeWidth={1.75} />
                <span className="flex-1">Paleta de comandos</span>
              </button>
              <div className="my-1 h-px bg-line" />
              <button
                type="button"
                onClick={mobileAct(onToggleTheme)}
                className="pressable flex w-full items-center gap-2.5 rounded-lg px-3 py-2 text-left text-[13px] text-ink-1 hover:bg-hover"
              >
                {dark ? <Sun className="size-4 shrink-0 text-ink-2" strokeWidth={1.75} /> : <Moon className="size-4 shrink-0 text-ink-2" strokeWidth={1.75} />}
                <span className="flex-1">{dark ? "Tema claro" : "Tema oscuro"}</span>
              </button>
            </Popover.Content>
          </Popover.Portal>
        </Popover.Root>
      </nav>

      {/* Bottom Utility Dock (desktop rail) */}
      <div className="mt-auto hidden flex-col items-center gap-1.5 border-t border-line pt-3 md:flex">
        <RailButton label="Lanzar agente" kbd="L" accent onClick={onLaunch}>
          <Plus className="size-[18px]" strokeWidth={2.2} />
        </RailButton>
        <RailButton label="Consultor independiente" kbd="C" onClick={onConsultor}>
          <MessagesSquare className="size-[17px]" strokeWidth={1.75} />
        </RailButton>
        <RailButton label="Actividad de tokens (24h)" kbd="A" onClick={onActivity}>
          <Activity className="size-[17px]" strokeWidth={1.75} />
        </RailButton>
        <RailButton label="Paleta de comandos" kbd="⌘K" onClick={onPalette}>
          <Command className="size-[17px]" strokeWidth={1.75} />
        </RailButton>
        <RailButton
          label={dark ? "Cambiar a tema claro" : "Cambiar a tema oscuro"}
          kbd="D"
          onClick={onToggleTheme}
        >
          {dark ? <Sun className="size-[17px]" strokeWidth={1.75} /> : <Moon className="size-[17px]" strokeWidth={1.75} />}
        </RailButton>
      </div>
    </aside>
  )
}
