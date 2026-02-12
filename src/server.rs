use std::{
    collections::HashSet,
    io::prelude::*,
    net::{Shutdown, SocketAddr, TcpListener, TcpStream},
    process,
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use regex::Regex;

use crate::definitions::*;
use crate::events::{EventSender, ServerEvent};
use crate::starsdata::StarsData;
use crate::utilities::*;
use crate::{dbprint, lazy_static};

lazy_static! {
    static ref SEARCHFROM: Regex = Regex::new(r"([a-zA-Z_0-9.\-]+)>").expect("Error parsing regex");
    static ref SEARCHTO: Regex =
        Regex::new(r"^([a-zA-Z_0-9.\-]+)\s*").expect("Error parsing regex");
    static ref SEARCHCMD1: Regex = Regex::new(r"^[^@]").expect("Error parsing regex");
    static ref SEARCHCMD2: Regex = Regex::new(r"^[^_]").expect("Error parsing regex");
    static ref SEARCHCMD3: Regex = Regex::new(r"^[_@]").expect("Error parsing regex");
    static ref SEARCHDISCONN: Regex = Regex::new(r"disconnect ").expect("Error parsing regex");
    static ref SEARCHFLGON: Regex = Regex::new(r"flgon ").expect("Error parsing regex");
    static ref SEARCHFLGOFF: Regex = Regex::new(r"flgoff ").expect("Error parsing regex");
    static ref SEARCHSPLIT: Regex = Regex::new(r"\r*\n").expect("Error parsing regex");
    static ref SEARCHEXIT: Regex = Regex::new(r"(?i)^(exit|quit)").expect("Error parsing regex");
    static ref SEARCHPARAM: Regex =
        Regex::new(r"^([a-zA-Z_0-9.\-]+)").expect("Error parsing regex");
}

pub struct ServerConfig {
    pub port: u16,
    pub libdir: String,
    pub keydir: String,
    pub timeout: u64,
}

pub fn run_server(config: ServerConfig, event_tx: EventSender) {
    let tout: Option<Duration> = if config.timeout > 0_u64 {
        Some(Duration::from_millis(config.timeout))
    } else {
        None
    };

    let nodes: Arc<Mutex<NodeList>> = Arc::new(Mutex::new(NodeList::new()));
    let sd: Arc<Mutex<StarsData>> = Arc::new(Mutex::new(StarsData::new(
        &config.libdir,
        &config.keydir,
    )));

    {
        let mut sdata = sd.lock().expect("can't get the lock!");
        startcheck(system_load_commandpermission(&mut sdata));
        startcheck(system_load_aliases(&mut sdata));
        startcheck(system_load_reconnecttable_permission(&mut sdata));
        system_load_shutdown_permission(&mut sdata);
    }

    let addr = SocketAddr::from(([0, 0, 0, 0], config.port));
    let listener = match TcpListener::bind(addr) {
        Ok(listener) => listener,
        Err(err) => {
            panic!("{} {}", "ERROR: Can't create socket for listining! ", err);
        }
    };

    println!("Server started. Time: {}", system_get_time());
    println!();

    loop {
        match listener.accept() {
            Ok((stream, _addr)) => {
                let (host, ip) = system_get_hostname_or_ip(&stream);
                dbprint!((&host, &ip));
                if !system_check_host(HOST_LIST, &host, &ip, false, &config.libdir) {
                    let errmsg = format!("Bad host. {host}\n");
                    {
                        let mut nodes_list = nodes.lock().expect("can't get the lock!");
                        writemsg(
                            &stream.try_clone().expect("stream clone failed!"),
                            errmsg,
                            &mut nodes_list,
                        );
                    }
                    stream
                        .shutdown(Shutdown::Both)
                        .expect("shutdown call failed")
                } else {
                    let nodekey = get_node_id_key();
                    let msg = format!("{nodekey}\n");
                    {
                        let mut nodes_list = nodes.lock().expect("can't get the lock!");
                        writemsg(
                            &stream.try_clone().expect("stream clone failed!"),
                            msg,
                            &mut nodes_list,
                        );
                    }
                    let rmsg = match recvmsg(
                        stream.try_clone().expect("stream clone failed!"),
                        "unknown",
                        tout,
                    ) {
                        Ok(rmsg) => rmsg,
                        Err(err) => {
                            eprintln!("{err}");
                            String::new()
                        }
                    };
                    dbprint!(rmsg);
                    if !rmsg.is_empty() {
                        match addnode(
                            stream.try_clone().expect("stream clone failed!"),
                            rmsg.trim().to_string(),
                            nodekey,
                            &nodes,
                            &mut sd.lock().expect("can't get the lock!"),
                            &event_tx,
                        ) {
                            Some(node) => {
                                let nodes = Arc::clone(&nodes);
                                let sd = Arc::clone(&sd);
                                let tx = event_tx.clone();
                                thread::spawn(move || {
                                    handle_node(
                                        node,
                                        stream.try_clone().expect("stream clone failed!"),
                                        nodes,
                                        sd,
                                        tx,
                                    );
                                });
                                continue;
                            }
                            None => {
                                match stream.shutdown(Shutdown::Both) {
                                    Ok(_) => {}
                                    Err(_) => {
                                        eprintln!("shutdown call failed");
                                    }
                                }
                                continue;
                            }
                        }
                    }
                    match stream.shutdown(Shutdown::Both) {
                        Ok(_) => {}
                        Err(_) => {
                            eprintln!("shutdown call failed");
                        }
                    }
                    continue;
                }
            }
            Err(err) => {
                eprintln!("Couldn't get client: {err:?}");
            }
        }
    }
}

fn handle_node(
    node: String,
    stream: TcpStream,
    nodes: Arc<Mutex<NodeList>>,
    sd: Arc<Mutex<StarsData>>,
    event_tx: EventSender,
) {
    let mut savebuf = String::new();
    'main: loop {
        let mut rmsg = match recvmsg(
            stream.try_clone().expect("stream clone failed!"),
            &node,
            None,
        ) {
            Ok(data) => data,
            Err(err) => {
                eprintln!("{err}");
                break 'main;
            }
        };
        if !savebuf.is_empty() {
            rmsg = format!("{savebuf}{rmsg}");
            savebuf.clear();
        }
        if !rmsg.is_empty() {
            let mut m: Vec<_> = SEARCHSPLIT.split(&rmsg).collect();
            if let Some(pos) = m.iter().position(|x| x.is_empty()) {
                m.remove(pos);
            } else if let Some(data) = m.pop() {
                savebuf = data.to_string();
            }
            for buf in m {
                if SEARCHEXIT.is_match(buf) {
                    break 'main;
                } else {
                    sendmes(
                        &node,
                        &stream,
                        buf,
                        &mut nodes.lock().expect("can't get the lock!"),
                        &sd,
                        &event_tx,
                    );
                }
            }
        } else {
            break 'main;
        }
    }
    {
        let mut nodes_list = nodes.lock().expect("can't get the lock!");
        let mut sdata = sd.lock().expect("can't get the lock!");
        delnode(&node, &mut nodes_list, &mut sdata, &event_tx);
    }
}

fn writemsg(stream: &TcpStream, msg: String, nodes: &mut std::sync::MutexGuard<'_, NodeList>) {
    dbprint!(msg);
    sendtonode(stream, &msg);
    sendtodebugger(&msg, nodes);
}

fn recvmsg(mut stream: TcpStream, name: &str, timeout: Option<Duration>) -> GenericResult<String> {
    match stream.set_read_timeout(timeout) {
        Ok(_) => {}
        Err(err) => {
            return Err(GenericError::from(crate::starserror::StarsError {
                message: format!("Set timeout faild! {err}."),
            }));
        }
    }

    let mut datamsg = Vec::new();
    let mut datapiece: [u8; TCP_BUFFER_SIZE] = [0u8; TCP_BUFFER_SIZE];
    loop {
        match stream.read(&mut datapiece) {
            Ok(0) => break,
            Ok(datacount) => {
                datamsg.extend_from_slice(&datapiece[..datacount]);
                if datapiece[..datacount].contains(&b'\n') {
                    break;
                }
            }
            Err(err) => {
                eprintln!("Error reading from client ({name}): {err}");
                break;
            }
        }
    }
    let msg = String::from_utf8_lossy(&datamsg).to_string();

    if msg.is_empty() {
        Err(GenericError::from(crate::starserror::StarsError {
            message: format!("({name}) Connection lost!"),
        }))
    } else {
        Ok(msg)
    }
}

fn sendtonode(stream: &TcpStream, msg: &String) {
    let mut writer = stream;
    match writer.write(msg.as_bytes()) {
        Ok(_success) => {}
        Err(err) => {
            eprintln!("Write Error: {err:?}");
            writer
                .shutdown(Shutdown::Both)
                .expect("shutdown call failed");
        }
    }
}

fn sendtodebugger(msg: &String, nodes: &mut NodeList) {
    if let Some(stream) = nodes.get("Debugger") {
        let mut writer = stream;
        match writer.write(msg.as_bytes()) {
            Ok(_success) => {}
            Err(err) => {
                eprintln!("Write Error: {err:?}");
                match writer.shutdown(Shutdown::Both) {
                    Ok(_) => {}
                    Err(err) => {
                        eprintln!("Shutdown call failed (Debugger): {err}");
                    }
                }
                nodes.remove("Debugger");
            }
        }
    }
}

#[allow(unused_assignments)]
fn sendmes(
    node: &str,
    stream: &TcpStream,
    msg: &str,
    nodes: &mut std::sync::MutexGuard<'_, NodeList>,
    sdata: &Arc<Mutex<StarsData>>,
    event_tx: &EventSender,
) {
    let fromnodes = node.to_string();
    let mut fromnode = fromnodes.clone();
    let mut tonodes = String::new();
    let mut tonode = String::new();
    let mut buf = msg.to_string();
    match SEARCHFROM.captures(&buf) {
        None => {}
        Some(caps) => {
            fromnode = caps.get(1).unwrap().as_str().to_owned();
            buf = buf.replace(caps.get(0).unwrap().as_str(), "");
        }
    }
    match SEARCHTO.captures(&buf) {
        None => {
            let msg = format!("System>{fromnode}> @\n");
            writemsg(stream, msg, nodes);
            return;
        }
        Some(caps) => {
            tonodes = caps.get(1).unwrap().as_str().to_owned();
            buf = buf.replace(caps.get(0).unwrap().as_str(), "");
        }
    }
    let mut sd: std::sync::MutexGuard<'_, StarsData> = sdata.lock().expect("can't get the lock!");
    if let Some(to) = sd.aliasreal.get(&tonodes) {
        tonodes = to.to_string();
    }
    if SEARCHCMD1.is_match(&buf)
        && ((!sd.cmddeny.is_empty()
            && is_deny_checkcmd_deny(&fromnodes, &tonodes, &buf, &sd.cmddeny))
            || (!sd.cmdallow.is_empty()
                && is_deny_checkcmd_allow(&fromnodes, &tonodes, &buf, &sd.cmdallow)))
    {
        if SEARCHCMD2.is_match(&buf) {
            let msg = format!("System>{fromnode} @{buf} Er: Command denied.\n");
            writemsg(stream, msg, nodes);
        }
        return;
    }
    tonode = (tonodes.split(".").map(str::to_string).collect::<Vec<_>>())[0].clone();
    if tonode.contains("System") {
        system_commands(node, stream, &fromnode, &buf, &mut sd, nodes);
        return;
    }
    if let Some(from) = sd.aliasreal.get(&fromnode) {
        fromnode = from.to_string();
    }
    match nodes.get(&tonode) {
        Some(sock) => {
            let msg = format!("{fromnode}>{tonodes} {buf}\n");
            let s = sock.try_clone().expect("stream clone failed!");
            writemsg(&s, msg, nodes);
            let _ = event_tx.send(ServerEvent::MessageRouted {
                from: fromnode.clone(),
                to: tonodes.clone(),
            });
        }
        None => {
            if !SEARCHCMD3.is_match(&buf) {
                let msg = format!("System>{fromnode} @{buf} Er: {tonode} is down.\n");
                writemsg(stream, msg, nodes);
            }
        }
    }
}

fn addnode(
    stream: TcpStream,
    msg: String,
    nodekey: u16,
    nodes: &Arc<Mutex<NodeList>>,
    sdata: &mut std::sync::MutexGuard<'_, StarsData>,
    event_tx: &EventSender,
) -> Option<String> {
    let node_id: Vec<String> = msg.split_whitespace().map(str::to_string).collect();
    if node_id.len() != 2 {
        return None;
    }
    let mut node = node_id[0].clone();
    let idmess = &node_id[1];

    let mut nodes_list = nodes.lock().expect("can't get the lock!");

    if let Some(s) = nodes_list.get(&node) {
        let stream_ref = s.try_clone().expect("stream clone failed!");
        if !check_reconnecttable(&node, &stream_ref, sdata) {
            let existmsg = format!("System> Er: {node} already exists.\n");
            writemsg(&stream, existmsg, &mut nodes_list);
            return None;
        } else {
            delnode(&node, &mut nodes_list, sdata, event_tx);
        }
    }
    if !check_term_and_host(&node, &stream, &sdata.libdir) {
        let errmsg = format!("System> Er: Bad host for {}\n", &node);
        writemsg(&stream, errmsg, &mut nodes_list);
        return None;
    }
    if !check_nodekey(&node, nodekey as usize, idmess, &sdata.keydir) {
        let errmsg = "System> Er: Bad node name or key\n".to_string();
        writemsg(&stream, errmsg, &mut nodes_list);
        return None;
    }

    let msg_ok = format!("System>{node} Ok:\n");
    writemsg(
        &stream.try_clone().expect("stream clone failed!"),
        msg_ok,
        &mut nodes_list,
    );
    nodes_list.insert(node.clone(), stream);

    let _ = event_tx.send(ServerEvent::NodeConnected { name: node.clone() });

    if let Some(n) = sdata.realalias.get(&node) {
        node = n.to_string();
    }
    for key_val in &sdata.nodes_flgon {
        if key_val.1.contains(&node) {
            let topre: Vec<String> = key_val.0.split(".").map(str::to_string).collect();
            if let Some(sock) = nodes_list.get(&topre[0]) {
                let s = sock.try_clone().expect("stream clone failed!");
                let msg = format!("{}>{} _Connected\n", node, key_val.0);
                writemsg(&s, msg, &mut nodes_list);
            }
        }
    }
    Some(node)
}

fn delnode(
    node: &str,
    nodes: &mut std::sync::MutexGuard<'_, NodeList>,
    sdata: &mut std::sync::MutexGuard<'_, StarsData>,
    event_tx: &EventSender,
) {
    if let Some(s) = nodes.remove(node) {
        let mut node = node.to_string();

        let _ = event_tx.send(ServerEvent::NodeDisconnected { name: node.clone() });

        let stream_ref = s.try_clone().expect("stream clone failed!");
        match stream_ref.shutdown(Shutdown::Both) {
            Ok(_) => (),
            Err(err) => {
                eprintln!("Shutdown call failed ({}): {}", &node, err);
            }
        }
        sdata.nodes_flgon.remove(&node);
        if let Some(n) = sdata.realalias.get(&node) {
            node = n.to_string();
        }
        for key_val in &sdata.nodes_flgon {
            if key_val.1.contains(&node) {
                let topre: Vec<String> = key_val.0.split(".").map(str::to_string).collect();
                if let Some(sock) = nodes.get(&topre[0]) {
                    let s = sock.try_clone().expect("stream clone failed!");
                    let msg = format!("{}>{} _Disconnected\n", node, key_val.0);
                    writemsg(&s, msg, nodes);
                }
            }
        }
    }
}

fn system_commands(
    node: &str,
    stream: &TcpStream,
    fromnode: &str,
    cmd: &str,
    sdata: &mut std::sync::MutexGuard<'_, StarsData>,
    nodes: &mut std::sync::MutexGuard<'_, NodeList>,
) {
    if cmd.starts_with("_") {
        system_event(node, cmd, nodes, sdata);
    } else if SEARCHDISCONN.is_match(cmd) {
        let msg = cmd.replace("disconnect ", "");
        system_disconnect(stream, fromnode, &msg, sdata, nodes);
    } else if SEARCHFLGON.is_match(cmd) {
        let msg = cmd.replace("flgon ", "");
        system_flgon(stream, fromnode, &msg, sdata, nodes);
    } else if SEARCHFLGOFF.is_match(cmd) {
        let msg = cmd.replace("flgoff ", "");
        system_flgoff(stream, fromnode, &msg, sdata, nodes);
    } else {
        match cmd {
            "loadpermission" => match system_load_commandpermission(sdata) {
                Ok(_) => {
                    let msg = format!(
                        "System>{fromnode} @loadpermission Command permission list has been loaded.\n"
                    );
                    writemsg(stream, msg, nodes);
                }
                Err(_) => {
                    let msg = format!(
                        "System>{fromnode} @loadpermission Er: Command permission list has been NOT loaded!\n"
                    );
                    writemsg(stream, msg, nodes);
                }
            },
            "loadreconnectablepermission" => match system_load_reconnecttable_permission(sdata) {
                Ok(_) => {
                    let msg = format!(
                        "System>{fromnode} @loadreconnectablepermission Reconnectable permission list has been loaded.\n"
                    );
                    writemsg(stream, msg, nodes);
                }
                Err(_) => {
                    let msg = format!(
                        "System>{fromnode} @loadreconnectablepermission Er: Reconnectable permission list has been NOT loaded!\n"
                    );
                    writemsg(stream, msg, nodes);
                }
            },
            "loadaliases" => match system_load_aliases(sdata) {
                Ok(_) => {
                    let msg = format!("System>{fromnode} @loadaliases Aliases has been loaded.\n");
                    writemsg(stream, msg, nodes);
                }
                Err(_) => {
                    let msg = format!(
                        "System>{fromnode} @loadaliases Er: Aliases has been NOT loaded!\n"
                    );
                    writemsg(stream, msg, nodes);
                }
            },
            "listaliases" => {
                let msg = format!(
                    "System>{} @listaliases {}\n",
                    fromnode,
                    system_list_aliases(sdata)
                );
                writemsg(stream, msg, nodes);
            }
            "listnodes" => {
                let msg = format!(
                    "System>{} @listnodes {}\n",
                    fromnode,
                    system_list_nodes(nodes)
                );
                writemsg(stream, msg, nodes);
            }
            "getversion" => {
                let msg =
                    format!("System>{fromnode} @getversion Version: {VERSION} (Rust Server)\n");
                writemsg(stream, msg, nodes)
            }
            "gettime" => {
                let msg = format!("System>{} @gettime {}\n", fromnode, system_get_time());
                writemsg(stream, msg, nodes)
            }
            "hello" => {
                let msg = format!("System>{fromnode} @hello Nice to meet you.\n");
                writemsg(stream, msg, nodes);
            }
            "help" => {
                let msg = format!(
                    "System>{fromnode} @help flgon flgoff loadaliases listaliases loadpermission loadreconnectablepermission listnodes shutdown getversion gettime hello disconnect\n",
                );
                writemsg(stream, msg, nodes);
            }
            "shutdown" => {
                if !sdata.shutallow.is_empty() && is_shutdowncmd_allow(fromnode, &sdata.shutallow) {
                    system_shutdown(nodes);
                } else {
                    let msg = format!("System>{fromnode} @shutdown Er: Command denied.\n");
                    writemsg(stream, msg, nodes);
                }
            }
            _ => {
                let msg = format!(
                    "System>{fromnode} @{cmd} Er: Command is not found or parameter is not enough!\n"
                );
                writemsg(stream, msg, nodes);
            }
        }
    };
}

fn system_event(
    node: &str,
    cmd: &str,
    nodes: &mut std::sync::MutexGuard<'_, NodeList>,
    sdata: &std::sync::MutexGuard<'_, StarsData>,
) {
    let mut frn = node.to_string();
    if let Some(n) = sdata.aliasreal.get(&frn) {
        frn = n.to_string();
    }
    for key_val in &sdata.nodes_flgon {
        if key_val.1.contains(&frn) {
            let topre: Vec<String> = key_val.0.split(".").map(str::to_string).collect();
            let to = &topre[0];
            if let Some(sock) = nodes.get(&topre[0]) {
                let s = sock.try_clone().expect("stream clone failed!");
                let msg = format!("{frn}>{to} {cmd}\n");
                writemsg(&s, msg, nodes);
            }
        }
    }
}

fn system_disconnect(
    stream: &TcpStream,
    fromnode: &str,
    cmd: &str,
    sdata: &mut std::sync::MutexGuard<'_, StarsData>,
    nodes: &mut std::sync::MutexGuard<'_, NodeList>,
) {
    if !SEARCHPARAM.is_match(cmd) {
        let msg = format!("System>{fromnode} @disconnect Er: Parameter is not enough.\n");
        writemsg(stream, msg, nodes);
        return;
    }
    let mut cmd = cmd.to_string();
    if let Some(v) = sdata.aliasreal.get(&cmd) {
        cmd = v.to_string();
    }
    match nodes.get(&cmd) {
        Some(_) => {}
        None => {
            let msg = format!("System>{fromnode} @disconnect Er: Node {cmd} is down.\n");
            writemsg(stream, msg, nodes);
            return;
        }
    }
    let msg = format!("System>{fromnode} @disconnect {cmd}.\n");
    writemsg(stream, msg, nodes);
    // Note: system_disconnect does not send event_tx because it's called from
    // within system_commands which doesn't have access to event_tx.
    // The node will be cleaned up when its handle_node thread detects the disconnect.
    if let Some(s) = nodes.remove(&cmd) {
        let mut node = cmd.to_string();
        let stream_ref = s.try_clone().expect("stream clone failed!");
        match stream_ref.shutdown(Shutdown::Both) {
            Ok(_) => (),
            Err(err) => {
                eprintln!("Shutdown call failed ({}): {}", &node, err);
            }
        }
        sdata.nodes_flgon.remove(&node);
        if let Some(n) = sdata.realalias.get(&node) {
            node = n.to_string();
        }
        for key_val in &sdata.nodes_flgon {
            if key_val.1.contains(&node) {
                let topre: Vec<String> = key_val.0.split(".").map(str::to_string).collect();
                if let Some(sock) = nodes.get(&topre[0]) {
                    let s = sock.try_clone().expect("stream clone failed!");
                    let msg = format!("{}>{} _Disconnected\n", node, key_val.0);
                    writemsg(&s, msg, nodes);
                }
            }
        }
    }
}

fn system_flgon(
    stream: &TcpStream,
    fromnode: &str,
    cmd: &str,
    sdata: &mut std::sync::MutexGuard<'_, StarsData>,
    nodes: &mut std::sync::MutexGuard<'_, NodeList>,
) {
    if !SEARCHPARAM.is_match(cmd) {
        let msg = format!("System>{fromnode} @disconnect Er: Parameter is not enough.\n");
        writemsg(stream, msg, nodes);
        return;
    }
    match sdata.nodes_flgon.get_mut(fromnode) {
        Some(flg_list) => {
            if flg_list.contains(cmd) {
                let msg =
                    format!("System>{fromnode} @flgon Er: Node {cmd} is allready in the list.\n");
                writemsg(stream, msg, nodes);
                return;
            }
            flg_list.insert(cmd.to_string());
            let msg = format!("System>{fromnode} @flgon Node {cmd} has been registered.\n");
            writemsg(stream, msg, nodes);
        }
        _ => {
            let mut val: HashSet<String> = HashSet::new();
            val.insert(cmd.to_string());
            sdata.nodes_flgon.insert(fromnode.to_string(), val);
            let msg = format!("System>{fromnode} @flgon Node {cmd} has been registered.\n");
            writemsg(stream, msg, nodes);
        }
    }
}

#[allow(unused_assignments)]
fn system_flgoff(
    stream: &TcpStream,
    fromnode: &str,
    cmd: &str,
    sdata: &mut std::sync::MutexGuard<'_, StarsData>,
    nodes: &mut std::sync::MutexGuard<'_, NodeList>,
) {
    if !SEARCHPARAM.is_match(cmd) {
        let msg = format!("System>{fromnode} @disconnect Er: Parameter is not enough.\n");
        writemsg(stream, msg, nodes);
        return;
    }
    match sdata.nodes_flgon.get_mut(fromnode) {
        Some(flg_list) => {
            let mut msg = String::new();
            if flg_list.remove(cmd) {
                msg = format!("System>{fromnode} @flgoff Node {cmd} has been removed.\n");
            } else {
                msg = format!("System>{fromnode} @flgoff Er: Node {cmd} is not in the list.\n");
            }
            writemsg(stream, msg, nodes);
        }
        _ => {
            let msg = format!("System>{fromnode} @flgoff Er: List is void.\n");
            writemsg(stream, msg, nodes);
        }
    }
}

fn system_shutdown(nodes: &mut std::sync::MutexGuard<'_, NodeList>) {
    println!("SYSTEM SHUTDOWN! -> {}", system_get_time());
    for (node, s) in nodes.iter_mut() {
        let stream_ref = s.try_clone().expect("stream clone failed!");
        let msg = format!("System>{} SYSTEMSHUTDOWN\n", node);
        sendtonode(&stream_ref, &msg);
        match stream_ref.shutdown(Shutdown::Both) {
            Ok(_) => (),
            Err(err) => {
                eprintln!("Shutdown call failed ({}): {}", &node, err);
            }
        }
    }
    process::exit(0);
}

fn startcheck(sc: GenericResult<()>) {
    match sc {
        Ok(_) => {}
        Err(err) => {
            eprintln!("Initialization faild! Server will not start!\n{err}");
            process::exit(1);
        }
    }
}
