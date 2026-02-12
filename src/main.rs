/**
 * STARS server in Rust
 * Based on Perl STARS server from Takashi Kosuge; KEK Tsukuba
 * stars.kek.jp
 */
use std::{sync::mpsc, thread};

use clap::Parser;
use configparser::ini::Ini;

mod definitions;
use definitions::*;
mod utilities;
mod starsdata;
mod starserror;
mod events;
mod server;
mod visualization;

use server::ServerConfig;
use starserror::StarsError;

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Arguments {
    /// Portnumber of the server.
    #[arg(short, long, default_value_t = 6057)]
    port: u16,
    /// Directory with the server .cfg and .key files.
    #[arg(short, long, default_value_t = DEFAULT_LIBDIR.to_string())]
    libdir: String,
    /// Directory with the server .key files. If empty lib directory will be used.
    #[arg(short, long, default_value_t = String::from(""))]
    keydir: String,
    /// Read timeout in msec
    #[arg(short, long, default_value_t = READ_TIMEOUT)]
    timeout: u64,
    /// Enable Bevy node graph visualization window
    #[arg(long, default_value_t = false)]
    visualize: bool,
}

struct Param {
    port: u16,
    libdir: String,
    keydir: String,
    timeout: u64,
}

fn read_parameter(args: &Arguments) -> Param {
    Param {
        port: args.port,
        libdir: args.libdir.clone(),
        keydir: args.keydir.clone(),
        timeout: args.timeout,
    }
}

fn read_config_file(fname: &str) -> GenericResult<Param> {
    let mut config = Ini::new();
    config.load(fname)?;
    let p = config
        .get("param", "starsport")
        .ok_or(GenericError::from(StarsError {
            message: "starsport keyword not found!".to_string(),
        }))?;
    let lb = config
        .get("param", "starslib")
        .ok_or(GenericError::from(StarsError {
            message: "starslib keyword not found!".to_string(),
        }))?;
    let kd = config
        .get("param", "starskey")
        .ok_or(GenericError::from(StarsError {
            message: "starskey keyword not found!".to_string(),
        }))?;
    let to = config
        .get("param", "timeout")
        .ok_or(GenericError::from(StarsError {
            message: "timeout keyword not found!".to_string(),
        }))?;
    let param = Param {
        port: p.parse()?,
        libdir: lb,
        keydir: kd,
        timeout: to.parse()?,
    };
    println!("Config file found.");
    Ok(param)
}

fn main() {
    let args = Arguments::parse();
    let visualize = args.visualize;

    println!();
    println!("STARS Server Version: {VERSION}");
    dbprint!("ON");
    println!();

    let mut param = match read_config_file(CONFIG_FILE) {
        Ok(p) => p,
        Err(err) => {
            let msg = format!("{err}");
            println!(
                "No config file found or error at reading file!\n{msg}\nUsing given or default arguments."
            );
            read_parameter(&args)
        }
    };
    if param.keydir.is_empty() {
        param.keydir = param.libdir.clone();
    }

    println!("--- Parameters ---");
    println!(" Port: {}", param.port);
    println!(" Lib: {}", param.libdir);
    println!(" Key: {}", param.keydir);
    println!(" Timeout: {}", param.timeout);
    println!("------------------");
    println!();

    let server_config = ServerConfig {
        port: param.port,
        libdir: param.libdir,
        keydir: param.keydir,
        timeout: param.timeout,
    };

    let (event_tx, event_rx) = mpsc::channel();

    if visualize {
        // Spawn TCP server on background thread, run Bevy on main thread (macOS requirement)
        thread::spawn(move || {
            server::run_server(server_config, event_tx);
        });
        visualization::run_visualization(event_rx);
    } else {
        // Original behavior: run server on main thread, events are silently dropped
        drop(event_rx);
        server::run_server(server_config, event_tx);
    }
}
