//! agencia: one native binary. `agencia serve` runs the 127.0.0.1:8800 server; the same binary answers as
//! bec / cdx / cla / slice / real-fin / agshot / _run (jobs), sesiones-bus, agy-send and agencia-up (multicall by argv[0] or argv[1]).
mod avance;
mod control;
mod loops;
mod server;
mod shared;
mod snapshot;

use agencia_core::jobs::JobManager;
use std::path::Path;

fn main() {
    let argv: Vec<String> = std::env::args().collect();
    let base = Path::new(argv.first().map(String::as_str).unwrap_or("agencia"))
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("agencia")
        .to_string();
    let sub = argv.get(1).map(String::as_str).unwrap_or("");
    let code = match (base.as_str(), sub) {
        ("sesiones-bus", _) => bus_cli(&argv[1..]),
        ("agencia", "sesiones-bus") => bus_cli(&argv[2..]),
        ("agy-send", _) => agencia_core::agylink::dispatch_cli(&argv),
        ("agencia", "agy-send") => agencia_core::agylink::dispatch_cli(&argv[1..]),
        ("agencia-up", _) | ("agencia", "up") => up(),
        ("agencia", "init") => init_wizard(argv.iter().any(|a| a == "--force")),
        ("agencia", "open") => open_app(),
        ("agencia", "serve") => {
            server::serve();
            0
        }
        ("agencia", "") => {
            let arg0 = argv.first().map(String::as_str).unwrap_or("");
            if arg0.contains("/target/") || std::env::var_os("AGENCIA_PORT").is_some() {
                server::serve();
                0
            } else {
                open_app()
            }
        }
        ("agencia", "mcp") => match agencia_core::bus::Bus::open_default() {
            Ok(bus) => {
                let _ = agencia_core::bus_mcp::run_mcp_stdio(&bus);
                0
            }
            Err(e) => {
                eprintln!("sesiones: {e}");
                1
            }
        },
        _ => JobManager::dispatch_cli(shared::root(), &argv),
    };
    std::process::exit(code);
}

fn bus_cli(args: &[String]) -> i32 {
    match agencia_core::bus::Bus::open_default() {
        Ok(bus) => agencia_core::bus::dispatch_cli(&bus, args),
        Err(e) => {
            eprintln!("sesiones-bus: {e}");
            1
        }
    }
}

fn listening(port: u16) -> bool {
    std::net::TcpStream::connect_timeout(
        &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
        std::time::Duration::from_millis(300),
    )
    .is_ok()
}

/// Starts the server in the background if the port is free. Prints "up" or "started".
fn up() -> i32 {
    let port = shared::port();
    if listening(port) {
        println!("up");
        return 0;
    }
    let root = shared::root();
    let _ = std::fs::create_dir_all(root.join("cache"));
    let log = std::fs::OpenOptions::new().create(true).append(true).open(root.join("cache/server.log"));
    let exe = std::env::current_exe().unwrap_or_else(|_| root.join("target/release/agencia"));
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("serve").current_dir(&root).stdin(std::process::Stdio::null());
    for (k, _) in std::env::vars() {
        if k.starts_with("CLAUDE") || k.starts_with("ANTHROPIC_AUTH") {
            cmd.env_remove(k);
        }
    }
    if let Ok(f) = log {
        if let Ok(f2) = f.try_clone() {
            cmd.stdout(f).stderr(f2);
        }
    }
    use std::os::unix::process::CommandExt;
    cmd.process_group(0);
    if let Err(e) = cmd.spawn() {
        eprintln!("agencia-up: {e}");
        return 1;
    }
    for _ in 0..60 {
        if listening(port) {
            println!("started");
            return 0;
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    eprintln!("la Agencia no levantó en 15 s; revisa ~/Agencia/cache/server.log");
    1
}

/// Ensures the server is running and opens http://127.0.0.1:<port> in the default browser.
fn open_app() -> i32 {
    let port = shared::port();
    if !listening(port) && up() != 0 {
        return 1;
    }
    let url = format!("http://127.0.0.1:{port}");
    match std::process::Command::new("/usr/bin/open").arg(&url).status() {
        Ok(st) if st.success() => {
            println!("{url}");
            0
        }
        Ok(st) => st.code().unwrap_or(1),
        Err(e) => {
            eprintln!("agencia: no pude abrir {url}: {e}");
            1
        }
    }
}

fn which_exists(bin: &str) -> bool {
    std::env::var_os("PATH")
        .map(|paths| {
            std::env::split_paths(&paths).any(|dir| {
                let p = dir.join(bin);
                p.is_file()
            })
        })
        .unwrap_or(false)
}

/// Lightweight setup wizard: detects installed AI CLIs/inference tools and writes ~/.config/agencia/config.json.
fn init_wizard(force: bool) -> i32 {
    let probes = [
        ("claude", "Claude Code"),
        ("codex", "OpenAI Codex"),
        ("opencode", "OpenCode"),
        ("agy", "Antigravity"),
        ("gemini", "Gemini CLI"),
        ("aider", "Aider"),
        ("ollama", "Ollama (local inference)"),
    ];
    let mut found = Vec::new();
    println!("Agencia init — detecting installed agent CLIs:");
    for (bin, label) in probes {
        let ok = which_exists(bin);
        println!("  [{}] {:<10} {}", if ok { "x" } else { " " }, bin, label);
        if ok {
            found.push(bin);
        }
    }

    let cfg_dir = std::path::PathBuf::from(shared::home()).join(".config/agencia");
    let cfg_path = cfg_dir.join("config.json");
    if cfg_path.exists() && !force {
        println!("\nConfig already exists: {} (use `agencia init --force` to overwrite)", cfg_path.display());
    } else {
        let _ = std::fs::create_dir_all(&cfg_dir);
        let cfg = serde_json::json!({
            "detected_clis": found,
            "codex_name": "Codex",
            "bec_model": "",
            "cla_model": ""
        });
        let _ = std::fs::write(&cfg_path, serde_json::to_string_pretty(&cfg).unwrap_or_default());
        println!("\nWrote {}", cfg_path.display());
    }
    let exe = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| shared::root().join("target/release/agencia").display().to_string());
    println!("\nNext steps:");
    println!("  1. Run `agencia` to start http://127.0.0.1:8800 and open the dashboard.");
    println!("  2. To connect any MCP-capable agent to the `sesiones` bus, point it to:");
    println!("     command: {exe}  args: [\"mcp\"]");
    println!("  3. Want your own coding agent to adapt models/providers automatically?");
    println!("     Paste SETUP_AGENT.md into your agent.");
    0
}
