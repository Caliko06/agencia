import { StrictMode } from "react"
import { createRoot } from "react-dom/client"
import "@fontsource-variable/geist"
import "@fontsource-variable/geist-mono"
import "@xyflow/react/dist/style.css"
import "./index.css"
import App from "./App"

const isDemo =
  import.meta.env.VITE_PORTFOLIO_DEMO === "1" || new URLSearchParams(location.search).get("demo") === "1"

/** Tells the server the OWNER is here: background Haiku and Sonnet only run for 5 min after the last touch or after the tab comes back (throttled to one ping per 20 s, only from a visible tab). */
if (!isDemo) {
  let last = 0
  const pulse = () => {
    const n = Date.now()
    if (document.hidden || n - last < 20000) return
    last = n
    fetch("/api/pulse", { method: "POST", body: "{}" }).catch(() => {})
  }
  for (const e of ["pointermove", "pointerdown", "keydown", "wheel"]) addEventListener(e, pulse, { passive: true })
  document.addEventListener("visibilitychange", pulse)
  pulse()
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
)
