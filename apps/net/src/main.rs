//! `kestrel-net`: Kestrel's only way to the internet (v4 C, blueprint rule 2, §4.8). The core starts it
//! when needed and talks JSON lines over stdio; the first line is the key for approval tokens. It knows
//! a short list of operations (kestrel-net-proto) and nothing else: no notes, no memory.

mod download;
mod fetch;
mod mail;
mod secrets;
mod worker;

use std::io::{BufRead, Write};
use std::process::ExitCode;

fn main() -> ExitCode {
    let stdin = std::io::stdin();
    let mut lines = stdin.lock().lines();
    let Some(Ok(key)) = lines.next() else {
        eprintln!("kestrel-net: no key: this program is started by Kestrel");
        return ExitCode::FAILURE;
    };
    let mut worker = worker::Worker::new(key.trim().to_string(), worker::Real);
    let mut out = std::io::stdout();
    if writeln!(out, "{{\"ready\":true}}").is_err() || out.flush().is_err() {
        return ExitCode::FAILURE;
    }
    for line in lines {
        let Ok(line) = line else { break };
        let reply = worker.answer(&line);
        if writeln!(out, "{reply}").is_err() || out.flush().is_err() {
            break;
        }
    }
    ExitCode::SUCCESS
}
