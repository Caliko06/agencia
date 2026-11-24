import { useState } from "react"
import { ChevronDown, Gauge } from "lucide-react"
import { Popover } from "radix-ui"
import { Brand } from "@/components/brand"
import type { Meter, Quota } from "@/lib/agencia"
import { cn } from "@/lib/utils"

const DAYS = ["dom", "lun", "mar", "mié", "jue", "vie", "sáb"]

function formatReset(r?: string | number) {
  let d: Date | null = null
  if (typeof r === "number") d = new Date(r * 1000)
  else if (r) {
    const m = r.match(/(\d+)\/(\d+) (\d+):(\d+)/)
    if (m) d = new Date(new Date().getFullYear(), +m[1] - 1, +m[2], +m[3], +m[4])
  }
  if (!d || isNaN(+d)) return ""
  const hm = d.toLocaleTimeString("es", { hour: "2-digit", minute: "2-digit", hour12: false })
  return d.toDateString() === new Date().toDateString() ? hm : `${DAYS[d.getDay()]} ${hm}`
}

type Acc = { name: string; s: number; w: number; sr?: string | number; wr?: string | number; tip?: string }
const freeOf = (a: Acc) => Math.round(Math.max(0, Math.min(a.s, a.w)))
const barColor = (n: number) => (n <= 0 ? "bg-bad" : n < 20 ? "ag-bar-need" : "ag-bar-ok")
const backAt = (a: Acc) => (a.w <= 0 ? formatReset(a.wr) : a.s <= 0 ? formatReset(a.sr) : "")
const toAcc = (m: Meter, name: string): Acc => ({
  name,
  s: Math.max(0, 100 - (m.s ?? 0)),
  w: Math.max(0, 100 - (m.w ?? 0)),
  sr: m.sr,
  wr: m.wr,
})

export function QuotaPopover({ quota }: { quota?: Quota }) {
  const [open, setOpen] = useState(false)
  if (!quota) return null

  const wk = quota.go.find((x) => x.label === "semana")
  const h5 = quota.go.find((x) => x.label === "5 h")

  const groups: { kind: "claude" | "codex" | "bec"; title: string; note?: string; list: Acc[] }[] = [
    {
      kind: "claude" as const,
      title: "Claude",
      list: quota.claude.map((m) => toAcc(m, m.name.replace("Cuenta ", "C"))),
    },
    {
      kind: "codex" as const,
      title: "Codex",
      list: quota.codex.map((m) => toAcc(m, m.name)),
    },
    ...(wk
      ? [
          {
            kind: "bec" as const,
            title: "DeepSeek",
            note: `$${(wk.cap - wk.usd).toFixed(0)}/${wk.cap}`,
            list: [
              {
                name: "Go",
                s: 100 - (h5?.pct ?? 0),
                w: 100 - wk.pct,
                tip: quota.go.map((g) => `${g.label}: $${g.usd.toFixed(2)} de $${g.cap}`).join(" · "),
              },
            ],
          },
        ]
      : []),
  ].filter((g) => g.list.length > 0)

  if (!groups.length) return null

  return (
    <Popover.Root open={open} onOpenChange={setOpen}>
      <Popover.Trigger asChild>
        <button
          type="button"
          className="pressable inline-flex h-9 items-center gap-2.5 rounded-lg border border-line-strong bg-surface px-3 text-[13.5px] text-ink-1 hover:bg-hover"
          title="Capacidad disponible por plan (ventana más justa)"
        >
          <Gauge className="size-3.5 text-ink-3" />
          <span className="hidden items-center gap-2.5 md:inline-flex">
            {groups.map((g) => {
              const best = [...g.list].sort((a, b) => freeOf(b) - freeOf(a))[0]
              const f = freeOf(best)
              return (
                <span key={g.title} className="inline-flex items-center gap-1 font-mono text-[11.5px] tabular">
                  <Brand kind={g.kind} className="size-3" />
                  <span className={cn(f <= 0 ? "text-bad" : f < 20 ? "text-need" : "text-ink-1")}>{f}%</span>
                </span>
              )
            })}
          </span>
          <ChevronDown className="size-3 text-ink-3" />
        </button>
      </Popover.Trigger>
      <Popover.Portal>
        <Popover.Content
          align="end"
          sideOffset={6}
          className="anim-modal z-50 w-[340px] rounded-xl border border-line-strong bg-surface p-3 shadow-[var(--shadow-lg)]"
        >
          <div className="mb-2.5 flex items-baseline justify-between border-b border-line pb-2">
            <span className="text-[13px] font-semibold text-ink-1">Capacidad disponible</span>
            <span className="text-[11px] text-ink-3">Ventana 5h y semanal</span>
          </div>
          <div className="flex flex-col gap-3">
            {groups.map((g) => {
              const sorted = [...g.list].sort((a, b) => freeOf(b) - freeOf(a))
              const best = sorted[0]
              const f = freeOf(best)
              return (
                <div key={g.title} className="flex flex-col gap-1.5">
                  <div className="flex items-center justify-between text-[12px]">
                    <span className="inline-flex items-center gap-1.5 font-medium text-ink-1">
                      <Brand kind={g.kind} className="size-3.5" />
                      {g.title}
                    </span>
                    <span className="font-mono text-[11px] text-ink-3">
                      {g.note ? g.note : f > 0 ? `mejor: ${best.name}` : `vuelve ${backAt(best)}`}
                    </span>
                  </div>
                  <div className="flex flex-col gap-1 rounded-md border border-line bg-inset p-2">
                    {sorted.map((a) => {
                      const af = freeOf(a)
                      return (
                          <div
                            key={a.name}
                            title={a.tip ?? `${a.name}: ${Math.round(a.s)}% (5h, renueva ${formatReset(a.sr)}) | ${Math.round(a.w)}% (semana, renueva ${formatReset(a.wr)})`}
                            className="grid grid-cols-[minmax(76px,auto)_1fr_minmax(56px,auto)] items-center gap-2 text-[11.5px]"
                          >
                            <span className="max-w-[110px] truncate font-mono text-ink-2" title={a.name}>{a.name}</span>
                          <div className="flex flex-col gap-1">
                            <div className="h-1 w-full overflow-hidden rounded-full bg-hover">
                              <div className={cn("h-full rounded-full", barColor(a.s))} style={{ width: `${a.s}%` }} />
                            </div>
                            <div className="h-1 w-full overflow-hidden rounded-full bg-hover">
                              <div className={cn("h-full rounded-full opacity-70", barColor(a.w))} style={{ width: `${a.w}%` }} />
                            </div>
                          </div>
                          <span className="text-right font-mono tabular text-ink-1">
                            {af > 0 ? `${af}%` : <span className="text-[10px] text-bad">{backAt(a) || "0%"}</span>}
                          </span>
                        </div>
                      )
                    })}
                  </div>
                </div>
              )
            })}
          </div>
        </Popover.Content>
      </Popover.Portal>
    </Popover.Root>
  )
}
