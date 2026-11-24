import { useState } from "react"
import { Camera, ChevronLeft, ChevronRight, X } from "lucide-react"
import { Dialog } from "radix-ui"
import type { Shot } from "@/lib/agencia"

function ShotImage({ shot }: { shot: Shot }) {
  const [status, setStatus] = useState<"loading" | "ready" | "error">("loading")
  return (
    <div className="grid min-h-[280px] flex-1 place-items-center overflow-auto bg-inset p-4" aria-busy={status === "loading"}>
      {status !== "ready" && (
        <p role="status" className="text-[13px] text-ink-3">
          {status === "error" ? "La captura ya no está disponible en su ruta original." : "Cargando captura…"}
        </p>
      )}
      <img
        src={shot.url}
        alt={shot.caption || "Captura del agente"}
        hidden={status !== "ready"}
        onLoad={() => setStatus("ready")}
        onError={() => setStatus("error")}
        className="max-h-[66vh] max-w-full rounded border border-line object-contain"
      />
    </div>
  )
}

export function Shots({ shots, name, strip }: { shots?: Shot[]; name: string; strip?: boolean }) {
  const [selected, setSelected] = useState<string | null>(null)
  if (!shots?.length) return null
  const index = Math.max(0, shots.findIndex((s) => s.url === selected))
  const shot = shots[index]
  const move = (delta: number) => setSelected(shots[Math.max(0, Math.min(shots.length - 1, index + delta))].url)

  return (
    <span
      className="nodrag nopan nowheel inline-flex items-center gap-1.5"
      onClick={(e) => e.stopPropagation()}
      onKeyDown={(e) => e.stopPropagation()}
    >
      <Dialog.Root onOpenChange={(open) => !open && setSelected(null)}>
        {strip &&
          shots.slice(0, 4).map((s) => (
            <Dialog.Trigger key={s.url} asChild>
              <button
                type="button"
                onClick={() => setSelected(s.url)}
                title={s.caption || "Captura"}
                aria-label={`Abrir captura: ${s.caption || name}`}
                className="pressable h-11 w-16 overflow-hidden rounded border border-line bg-inset hover:border-line-strong"
              >
                <img src={s.url} alt="" loading="lazy" className="size-full object-cover" />
              </button>
            </Dialog.Trigger>
          ))}
        <Dialog.Trigger asChild>
          <button
            type="button"
            aria-label={`Ver ${shots.length} ${shots.length === 1 ? "captura" : "capturas"} de ${name}`}
            title="Ver capturas"
            className="pressable inline-flex h-6 items-center gap-1 rounded border border-line bg-inset px-1.5 font-mono text-[12px] font-medium text-ink-2 hover:bg-hover hover:text-ink-1"
          >
            <Camera aria-hidden="true" className="size-3" />
            <span className="tabular-nums">{shots.length}</span>
          </button>
        </Dialog.Trigger>
        <Dialog.Portal>
          <Dialog.Overlay className="anim-overlay fixed inset-0 z-50 bg-black/70" />
          <Dialog.Content
            data-agshot-viewer
            onKeyDown={(e) => {
              if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
                e.preventDefault()
                move(e.key === "ArrowLeft" ? -1 : 1)
              }
              e.stopPropagation()
            }}
            className="anim-modal fixed top-1/2 left-1/2 z-50 flex max-h-[88vh] w-[min(92vw,920px)] -translate-x-1/2 -translate-y-1/2 flex-col overflow-hidden rounded-xl border border-line-strong bg-surface shadow-[var(--shadow-lg)]"
          >
            <div className="flex items-center justify-between border-b border-line bg-elevated px-4 py-3">
              <Dialog.Title className="text-[14px] font-semibold text-ink-1">Capturas de {name}</Dialog.Title>
              <Dialog.Close asChild>
                <button
                  type="button"
                  aria-label="Cerrar capturas"
                  className="pressable grid size-7 place-items-center rounded-md border border-line bg-surface text-ink-2 hover:bg-hover hover:text-ink-1"
                >
                  <X className="size-4" />
                </button>
              </Dialog.Close>
            </div>
            <ShotImage key={shot.url} shot={shot} />
            <Dialog.Description className="border-t border-line bg-surface px-4 py-2.5 text-[13px] text-ink-2">
              {shot.caption || "Sin descripción"}
            </Dialog.Description>
            <div className="flex items-center justify-between border-t border-line bg-elevated px-4 py-2 text-[12px] text-ink-3">
              <time dateTime={new Date(shot.ts * 1000).toISOString()} className="font-mono tabular-nums">
                {new Date(shot.ts * 1000).toLocaleString("es-MX")}
              </time>
              <div className="flex items-center gap-2">
                <button
                  type="button"
                  disabled={index === 0}
                  aria-label="Captura más reciente"
                  onClick={() => move(-1)}
                  className="pressable grid size-7 place-items-center rounded border border-line bg-surface text-ink-1 hover:bg-hover disabled:opacity-35"
                >
                  <ChevronLeft className="size-4" />
                </button>
                <span aria-live="polite" aria-atomic="true" className="font-mono text-[12px] tabular-nums text-ink-2">
                  {index + 1} / {shots.length}
                </span>
                <button
                  type="button"
                  disabled={index === shots.length - 1}
                  aria-label="Captura anterior"
                  onClick={() => move(1)}
                  className="pressable grid size-7 place-items-center rounded border border-line bg-surface text-ink-1 hover:bg-hover disabled:opacity-35"
                >
                  <ChevronRight className="size-4" />
                </button>
              </div>
            </div>
          </Dialog.Content>
        </Dialog.Portal>
      </Dialog.Root>
    </span>
  )
}
