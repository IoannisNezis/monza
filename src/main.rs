//! Native CLI entry point for the qlue-ls language server.
//!
//! This binary provides three subcommands:
//!
//! - `server`: Run the LSP server over stdio (used by editors)
//! - `format`: Format SPARQL files from disk or stdin
//! - `logs`: Tail the server log file (Linux only)
//!
//! # Server Mode
//!
//! When running as a language server, this module sets up a single-threaded tokio
//! runtime with a `LocalSet` for spawning async tasks. Messages are read from stdin
//! via [`StdioMessages`] and responses are written to stdout with Content-Length headers.
//!
//! # Logging
//!
//! Logs are written to `$XDG_DATA_HOME/qlue-ls/qlue-ls.log` (or equivalent on other
//! platforms). The log level can be controlled via the `LOG_LEVEL` environment variable.
//!
//! # Related Modules
//!
//! - [`server`]: Core server implementation
//! - [`stdio_reader`]: LSP message framing for stdin

mod stdio_reader;

use std::{
    env,
    fs::{File, OpenOptions},
    io::{self, Read, Write},
    path::PathBuf,
    process::exit,
    rc::Rc,
};

use clap::{Parser, Subcommand};
use futures::lock::Mutex;
use tokio::{runtime, task::LocalSet};
use tracing_subscriber::EnvFilter;

use qlue_ls::{Server, format_raw, handle_message};

use crate::stdio_reader::StdioMessages;

/// qlue-ls: An SPARQL language server and formatter
#[derive(Debug, Parser)]
#[command(version, about, long_about= None)]
struct Cli {
    #[clap(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Run the language server
    Server,
    /// Run the formatter on a given file OR stdin
    Format {
        /// overwrite given file
        #[arg(short, long)]
        writeback: bool,
        /// Avoid writing formatted file back; instead, exit with a non-zero status code if any files would have been modified, and zero otherwise
        #[arg(short, long)]
        check: bool,
        /// Omit to read from stdin
        paths: Vec<PathBuf>,
    },
    /// Watch the logs (linux users only)
    Logs,
}

fn get_logfile_path() -> PathBuf {
    let mut app_dir = dirs_next::data_dir().expect("Failed to find data directory");
    app_dir.push("qlue-ls");
    if !app_dir.exists() {
        std::fs::create_dir_all(&app_dir).expect("Failed to create app directory");
    }
    app_dir.join("qlue-ls.log")
}

fn configure_logging() {
    let level = env::var("LOG_LEVEL").unwrap_or_else(|_| "info".to_string());
    let logfile_path = get_logfile_path();
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&logfile_path)
        .expect("Failed to open logfile");

    tracing_subscriber::fmt()
        .with_writer(std::sync::Mutex::new(file))
        .with_env_filter(EnvFilter::new(level))
        .with_ansi(false)
        .init();
}

fn send_message(message: String) {
    print!("Content-Length: {}\r\n\r\n{}", message.len(), message);
    io::stdout().flush().expect("No IO errors or EOFs");
}

fn main() {
    #[cfg(not(target_arch = "wasm32"))]
    configure_logging();

    let cli = Cli::parse();
    match cli.command {
        Command::Server => {
            // Start server and listen to stdio
            let rt = runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("Single threaded runtime should be usable");
            let local = LocalSet::new();
            rt.block_on(local.run_until(async {
                let server = Server::new(send_message);
                let server_rc = Rc::new(Mutex::new(server));
                for message in StdioMessages::new() {
                    handle_message(server_rc.clone(), message).await
                }
            }));
        }
        Command::Format {
            paths,
            writeback,
            check,
        } => {
            if paths.is_empty() {
                // No path was given -- read from stdin
                let mut buffer = Vec::new();
                io::stdin()
                    .read_to_end(&mut buffer)
                    .expect("Should read all bytes from stdin");
                // for line in io::stdin().lock().read_to_end(buffer).lines() {
                //     buffer += &line.unwrap();
                //     buffer += "\n";
                // }
                let input = String::from_utf8(buffer).expect("input should be valid UTF8");
                match format_raw(input) {
                    Ok(res) => {
                        print!("{}", res);
                    }
                    Err(e) => panic!("Error during formatting:\n{}", e),
                }
            }
            {
                for path in paths {
                    match File::open(path.clone()) {
                        Ok(mut file) => {
                            let mut contents = String::new();
                            file.read_to_string(&mut contents)
                                .expect("Could not read file");
                            match format_raw(contents.clone()) {
                                Ok(formatted_contents) => {
                                    let unchanged = formatted_contents == contents;
                                    if check {
                                        if unchanged {
                                            println!(
                                                "{} is already formatted",
                                                path.to_string_lossy()
                                            );
                                            exit(0);
                                        } else {
                                            println!(
                                                "{} would be reformatted",
                                                path.to_string_lossy()
                                            );
                                            exit(1);
                                        }
                                    }
                                    if writeback {
                                        if unchanged {
                                            println!("{} left unchanged", path.to_string_lossy());
                                        } else {
                                            let mut file = OpenOptions::new()
                                                .write(true)
                                                .truncate(true)
                                                .append(false)
                                                .open(path.clone())
                                                .expect("Could not write to file");
                                            file.write_all(formatted_contents.as_bytes())
                                                .expect("Unable to write");
                                            println!("{} reformatted", path.to_string_lossy());
                                        }
                                    } else {
                                        println!("{}", formatted_contents);
                                    }
                                }
                                Err(e) => panic!("Error during formatting:\n{}", e),
                            }
                        }
                        Err(e) => {
                            panic!("Could not open file: {}", e)
                        }
                    };
                }
            }
        }
        Command::Logs => {
            let logfile_path = get_logfile_path();
            if !std::process::Command::new("tail")
                .args(["--lines", "0"])
                .arg("-f")
                .arg(logfile_path)
                .status()
                .expect("Failed to start 'tail' process")
                .success()
            {
                println!("tail exited with non-zero status");
            }
        }
    };
}
