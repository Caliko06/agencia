import { useEffect, useState, type ReactNode } from "react"
import { X } from "lucide-react"
import { Dialog } from "radix-ui"
import Markdown from "react-markdown"
import remarkGfm from "remark-gfm"

const IMG = /\.(png|jpe?g|webp|gif)(\?|$)/i
const base = (p: string) => p.split("/").pop() || p

export const urlOf = (src: string, cwd?: string) =>
  src.startsWith("/api/") ? src : `/api/file?p=${encodeURIComponent(src)}${cwd ? `&base=${encodeURIComponent(cwd)}` : ""}`

export function Viewer({
  src,
  cwd,
  title,
  children,
  className,
}: {
  src: string
  cwd?: string
  title?: string
  children: ReactNode
  className?: string
}) {
  const url = urlOf(src, cwd)
  const path = src.startsWith("/api/") ? decodeURIComponent(src.match(/[?&]p=([^&]+)/)?.[1] ?? src) : src
  const img = IMG.test(path)

  return (
    <Dialog.Root>
      <Dialog.Trigger asChild>
        <button type="button" className={className} title={path}>
          {children}
        </button>
      </Dialog.Trigger>
      <Dialog.Portal>
        <Dialog.Overlay className="anim-overlay fixed inset-0 z-50 bg-black/60" />
        <Dialog.Content
          aria-describedby={undefined}
          onKeyDown={(e) => e.stopPropagation()}
          className="anim-modal fixed top-1/2 left-1/2 z-50 flex max-h-[86vh] w-[min(92vw,860px)] -translate-x-1/2 -translate-y-1/2 flex-col overflow-hidden rounded-xl border border-line-strong bg-surface shadow-[var(--shadow-lg)]"
        >
          <div className="flex items-center justify-between gap-3 border-b border-line bg-elevated px-4 py-3">
            <Dialog.Title className="truncate text-[14px] font-semibold text-ink-1">
              {title || base(path)}
            </Dialog.Title>
            <Dialog.Close asChild>
              <button
                type="button"
                aria-label="Cerrar visor"
                className="pressable grid size-7 place-items-center rounded-md border border-line bg-surface text-ink-2 hover:bg-hover hover:text-ink-1"
              >
                <X className="size-4" />
              </button>
            </Dialog.Close>
          </div>
          {img ? (
            <div className="grid min-h-[240px] flex-1 place-items-center overflow-auto bg-inset p-4">
              <img src={url} alt={base(path)} className="max-h-[70vh] max-w-full rounded border border-line object-contain" />
            </div>
          ) : (
            <FileBody url={url} md={/\.md$/i.test(path)} />
          )}
          <div className="border-t border-line bg-inset px-4 py-2 font-mono text-[11px] text-ink-3 truncate">
            {path}
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  )
}

function FileBody({ url, md }: { url: string; md: boolean }) {
  const [res, setRes] = useState<{ ok: boolean; body: string } | null>(null)
  useEffect(() => {
    let alive = true
    fetch(url)
      .then(async (r) => {
        const b = await r.text()
        if (alive) setRes({ ok: r.ok, body: r.ok ? b : JSON.parse(b || "{}").info ?? "No se pudo abrir el archivo" })
      })
      .catch(() => alive && setRes({ ok: false, body: "No se pudo abrir el archivo" }))
    return () => {
      alive = false
    }
  }, [url])

  if (!res) {
    return (
      <div className="flex-1 overflow-auto p-5 text-[13px] text-ink-3" role="status">
        Cargando archivo…
      </div>
    )
  }
  if (!res.ok) {
    return (
      <div className="flex-1 overflow-auto p-5 text-[13px] text-bad" role="alert">
        {res.body}
      </div>
    )
  }
  return (
    <div className="flex-1 overflow-auto p-5">
      {md ? (
        <div className="prose-ag">
          <Markdown
            remarkPlugins={[remarkGfm]}
            components={{
              a: ({ href, children }) => (
                <span className="underline decoration-line-strong" title={href}>
                  {children}
                </span>
              ),
            }}
          >
            {res.body}
          </Markdown>
        </div>
      ) : (
        <pre className="font-mono text-[12px] leading-relaxed text-ink-1 whitespace-pre-wrap break-words">{res.body}</pre>
      )}
    </div>
  )
}
