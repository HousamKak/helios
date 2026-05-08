//! `helios` — terminal client for the heliOS entity store.
//!
//! Connects to `helios-store` over its Unix socket and runs one
//! query per invocation. Output is plain text optimised for terminal
//! use; pipe-friendly.
//!
//! ```sh
//! helios ping
//! helios stats
//! helios ps                  # alias: processes
//! helios ps --limit 20
//! helios get-process 1234
//! helios events
//! helios events --limit 50 --source procfs
//! ```
//!
//! Override the store socket path via `HELIOS_STORE_SOCKET`. Default is
//! `/run/helios/store.sock` (the production location). For dev usage:
//!
//! ```sh
//! HELIOS_STORE_SOCKET=/tmp/helios-store.sock helios stats
//! ```

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("helios-cli is Linux-only (it talks to a Unix socket).");
    std::process::exit(2);
}

#[cfg(target_os = "linux")]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    use std::path::PathBuf;

    let args: Vec<String> = std::env::args().collect();
    let socket: PathBuf = std::env::var_os("HELIOS_STORE_SOCKET")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(helios_schema::ipc::DEFAULT_STORE_SOCKET));

    let cmd = args.get(1).map(String::as_str).unwrap_or("");

    let request = match cmd {
        "ping" => helios_schema::ipc::StoreRequest::Ping,
        "stats" => helios_schema::ipc::StoreRequest::Stats,
        "ps" | "processes" => helios_schema::ipc::StoreRequest::ListProcesses {
            limit: parse_limit(&args),
        },
        "get-process" => {
            let pid: i32 = args
                .get(2)
                .and_then(|s| s.parse().ok())
                .ok_or_else(|| anyhow::anyhow!("usage: helios get-process PID"))?;
            helios_schema::ipc::StoreRequest::GetProcess { pid }
        }
        "events" => helios_schema::ipc::StoreRequest::ListRecentEvents {
            limit: parse_limit(&args),
            source: parse_named(&args, "--source").map(String::from),
        },
        "" | "help" | "-h" | "--help" => {
            print_usage();
            return Ok(());
        }
        other => {
            eprintln!("unknown command: {other}\n");
            print_usage();
            std::process::exit(1);
        }
    };

    match call_store(&socket, request).await {
        Ok(response) => {
            print_response(&response);
            Ok(())
        }
        Err(err) => {
            eprintln!("error: {err}");
            eprintln!("hint: is helios-store running and is HELIOS_STORE_SOCKET correct?");
            eprintln!("      socket: {}", socket.display());
            std::process::exit(1);
        }
    }
}

#[cfg(target_os = "linux")]
async fn call_store(
    socket: &std::path::Path,
    request: helios_schema::ipc::StoreRequest,
) -> anyhow::Result<helios_schema::ipc::StoreResponse> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::UnixStream;

    let stream = UnixStream::connect(socket).await?;
    let (read, mut write) = stream.into_split();
    let mut reader = BufReader::new(read);

    let mut req_line = serde_json::to_string(&request)?;
    req_line.push('\n');
    write.write_all(req_line.as_bytes()).await?;
    write.flush().await?;

    let mut response_line = String::new();
    let n = reader.read_line(&mut response_line).await?;
    if n == 0 {
        anyhow::bail!("store socket closed before responding");
    }
    Ok(serde_json::from_str(response_line.trim())?)
}

#[cfg(target_os = "linux")]
fn print_response(response: &helios_schema::ipc::StoreResponse) {
    use helios_schema::ipc::StoreResponse;
    match response {
        StoreResponse::Pong {
            migrations_applied,
            schema_version,
        } => {
            println!("pong  schema {schema_version}  {migrations_applied} migrations applied");
        }
        StoreResponse::Stats {
            process_total,
            process_running,
            events_total,
            events_last_minute,
            last_event_at,
        } => {
            println!("processes:  {process_running:>8}  running ({process_total} total seen)");
            println!("events:     {events_total:>8}  total");
            println!("events/min: {events_last_minute:>8}");
            if let Some(t) = last_event_at {
                println!("last event: {t}");
            }
        }
        StoreResponse::Processes { processes } => {
            if processes.is_empty() {
                println!("(no running processes observed yet)");
                return;
            }
            println!(
                "{:>6} {:>6} {:<16} {:<32} cmdline",
                "pid", "ppid", "comm", "started_at"
            );
            for p in processes {
                let ppid = p.ppid.map(|p| p.to_string()).unwrap_or_else(|| "-".into());
                println!(
                    "{:>6} {:>6} {:<16} {:<32} {}",
                    p.pid, ppid, p.comm, p.started_at, p.cmdline
                );
            }
        }
        StoreResponse::Process { process } => match process {
            Some(p) => {
                println!("pid:        {}", p.pid);
                println!(
                    "ppid:       {}",
                    p.ppid.map(|x| x.to_string()).unwrap_or_else(|| "-".into())
                );
                println!("comm:       {}", p.comm);
                println!("cmdline:    {}", p.cmdline);
                println!("exe:        {}", p.exe.as_deref().unwrap_or("-"));
                println!("uid/gid:    {}/{}", p.uid, p.gid);
                println!("status:     {:?}", p.status);
                println!("started_at: {}", p.started_at);
                if let Some(t) = &p.exited_at {
                    println!("exited_at:  {}", t);
                }
            }
            None => println!("(no such pid)"),
        },
        StoreResponse::Events { events } => {
            if events.is_empty() {
                println!("(no events recorded yet)");
                return;
            }
            for e in events {
                println!(
                    "{}  {:<22}  {:<10}  {}",
                    e.timestamp, e.kind, e.source, e.id
                );
            }
        }
        StoreResponse::CanvasEntities { rows } => {
            if rows.is_empty() {
                println!("(no canvas entities)");
                return;
            }
            for row in rows {
                println!(
                    "{:<10}  {:<32}  desktop={}  pos=({:.1},{:.1})  scale={:.2}",
                    row.entity_kind.as_str(),
                    row.entity_id,
                    row.desktop_id,
                    row.x,
                    row.y,
                    row.scale
                );
            }
        }
        StoreResponse::Error { message } => {
            eprintln!("error: {message}");
            std::process::exit(1);
        }
    }
}

#[cfg(target_os = "linux")]
fn parse_limit(args: &[String]) -> Option<u32> {
    parse_named(args, "--limit").and_then(|s| s.parse().ok())
}

#[cfg(target_os = "linux")]
fn parse_named<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    let i = args.iter().position(|a| a == flag)?;
    args.get(i + 1).map(String::as_str)
}

#[cfg(target_os = "linux")]
fn print_usage() {
    println!("helios — terminal client for the heliOS entity store");
    println!();
    println!("USAGE:");
    println!("  helios <command> [args...]");
    println!();
    println!("COMMANDS:");
    println!("  ping                            verify store reachability + schema version");
    println!("  stats                           aggregate counters: processes, events");
    println!("  ps [--limit N]                  list running processes (alias: processes)");
    println!("  get-process PID                 fetch a single process by PID");
    println!("  events [--limit N] [--source S] list recent events");
    println!("  help                            show this help");
    println!();
    println!("ENVIRONMENT:");
    println!("  HELIOS_STORE_SOCKET   path to helios-store query socket");
    println!(
        "                        (default: {})",
        helios_schema::ipc::DEFAULT_STORE_SOCKET
    );
}
