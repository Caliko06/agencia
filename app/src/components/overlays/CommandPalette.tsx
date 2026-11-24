import { Command } from "cmdk"
import { Filter, Plus, Search } from "lucide-react"
import { Dialog } from "radix-ui"
import { SignalBar, statusToTone } from "@/components/bits"
import { Brand } from "@/components/brand"
import { RUNTIME_FILTERS } from "@/components/views/RedView"
import { kindOf, STATUS, type Repo } from "@/lib/agencia"
import { labelOf } from "@/lib/tone"

export function CommandPalette({
  open,
  onOpenChange,
  repos,
  onWin,
  onCard,
  onLaunch,
  onFilter,
}: {
  open: boolean
  onOpenChange: (o: boolean) => void
  repos: Repo[]
  onWin: (repo: string, win: string) => void
  onCard: (repo: string, win: string, id: string) => void
  onLaunch: () => void
  onFilter: (f: string) => void
}) {
  const run = (fn: () => void) => () => {
    onOpenChange(false)
    fn()
  }

  return (
    <Dialog.Root open={open} onOpenChange={onOpenChange}>
      <Dialog.Portal>
        <Dialog.Overlay className="anim-overlay fixed inset-0 z-50 bg-black/60" />
        <Dialog.Content
          aria-describedby={undefined}
          className="anim-modal fixed top-[16%] left-1/2 z-50 w-[min(92vw,560px)] -translate-x-1/2 overflow-hidden rounded-xl border border-line-strong bg-surface shadow-[var(--shadow-lg)]"
        >
          <Dialog.Title className="sr-only">Paleta de comandos</Dialog.Title>
          <Command className="flex flex-col">
            <div className="flex items-center gap-2 border-b border-line bg-elevated px-3.5 py-2.5">
              <Search className="size-4 shrink-0 text-ink-3" />
              <Command.Input
                autoFocus
                placeholder="Saltar a ventana, agente o acción…"
                className="h-7 min-w-0 flex-1 bg-transparent text-[13.5px] text-ink-1 outline-none placeholder:text-ink-3"
              />
              <kbd className="rounded border border-line bg-inset px-1.5 py-0.5 font-mono text-[10px] text-ink-3">ESC</kbd>
            </div>
            <Command.List className="max-h-[min(380px,60svh)] overflow-y-auto p-2">
              <Command.Empty className="py-8 text-center text-[13px] text-ink-3">Sin coincidencias.</Command.Empty>

              <Command.Group heading="Acciones" className="[&_[cmdk-group-heading]]:px-2 [&_[cmdk-group-heading]]:py-1.5 [&_[cmdk-group-heading]]:text-[11px] [&_[cmdk-group-heading]]:font-semibold [&_[cmdk-group-heading]]:text-ink-3">
                <Command.Item
                  onSelect={run(onLaunch)}
                  className="flex cursor-pointer items-center gap-2 rounded-md px-2.5 py-2 text-[13px] text-ink-1 aria-selected:bg-hover"
                >
                  <Plus className="size-4 text-ink-2" />
                  <span>Lanzar agente en este repo</span>
                  <kbd className="ml-auto rounded border border-line bg-inset px-1.5 py-0.5 font-mono text-[10px] text-ink-3">L</kbd>
                </Command.Item>
                {RUNTIME_FILTERS.map(([k, label]) => (
                  <Command.Item
                    key={k}
                    onSelect={run(() => onFilter(k))}
                    className="flex cursor-pointer items-center gap-2 rounded-md px-2.5 py-2 text-[13px] text-ink-1 aria-selected:bg-hover"
                  >
                    <Filter className="size-3.5 text-ink-3" />
                    <span>Filtrar grafo: {label}</span>
                  </Command.Item>
                ))}
              </Command.Group>

              <Command.Group heading="Ventanas" className="mt-1 [&_[cmdk-group-heading]]:px-2 [&_[cmdk-group-heading]]:py-1.5 [&_[cmdk-group-heading]]:text-[11px] [&_[cmdk-group-heading]]:font-semibold [&_[cmdk-group-heading]]:text-ink-3">
                {repos.flatMap((r) =>
                  r.windows.map((w) => (
                    <Command.Item
                      key={w.id}
                      value={`${r.name} ${w.title} ${w.now}`}
                      onSelect={run(() => onWin(r.name, w.id))}
                      className="flex cursor-pointer items-center gap-2 rounded-md px-2.5 py-2 text-[13px] text-ink-1 aria-selected:bg-hover"
                    >
                      <Brand kind={kindOf(w)} model={w.model} className="size-4" />
                      <span className="min-w-0 flex-1 truncate">{w.title}</span>
                      <span className="flex shrink-0 items-center gap-2 font-mono text-[11px] text-ink-3">
                        {r.name} · {STATUS[w.status]}
                        <SignalBar tone={statusToTone(w.status)} className="h-3" />
                      </span>
                    </Command.Item>
                  )),
                )}
              </Command.Group>

              <Command.Group heading="Agentes" className="mt-1 [&_[cmdk-group-heading]]:px-2 [&_[cmdk-group-heading]]:py-1.5 [&_[cmdk-group-heading]]:text-[11px] [&_[cmdk-group-heading]]:font-semibold [&_[cmdk-group-heading]]:text-ink-3">
                {repos.flatMap((r) =>
                  r.windows.flatMap((w) =>
                    w.children.map((c) => (
                      <Command.Item
                        key={c.id}
                        value={`${c.who} ${c.title} ${c.now} ${r.name}`}
                        onSelect={run(() => onCard(r.name, w.id, c.id))}
                        className="flex cursor-pointer items-center gap-2 rounded-md px-2.5 py-2 text-[13px] text-ink-1 aria-selected:bg-hover"
                      >
                        <Brand kind={kindOf(c)} model={c.model} className="size-4" />
                        <span className="min-w-0 flex-1 truncate">{labelOf(c)}</span>
                        <span className="flex shrink-0 items-center gap-2 font-mono text-[11px] text-ink-3">
                          {STATUS[c.status]}
                          <SignalBar tone={statusToTone(c.status)} className="h-3" />
                        </span>
                      </Command.Item>
                    )),
                  ),
                )}
              </Command.Group>
            </Command.List>
          </Command>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  )
}
