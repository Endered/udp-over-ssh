use std::collections::HashMap;
use std::env::args;
use std::error::Error;
use std::net::SocketAddr;
use std::sync::Arc;

use async_std::io::WriteExt;
use async_std::net::UdpSocket;
use async_std::sync::Mutex;
use async_std::{io, task};
use base64::Engine;

use futures::try_join;

fn parse(s: &str) -> Result<(usize, Vec<u8>), Box<dyn Error>> {
    let s: String = s.chars().filter(|c| c != &'\n' && c != &' ').collect();
    let [id, data] = s.split(':').collect::<Vec<_>>()[..] else {
        return Err(format!("Invalid format: {s}").into());
    };

    let id = id.parse()?;
    let data = data
        .chars()
        .filter(|c| c != &'\n' && c != &' ')
        .collect::<String>();
    let data = base64::prelude::BASE64_STANDARD.decode(data)?;

    Ok((id, data))
}

async fn run_send(target: String) -> Result<(), Box<dyn Error>> {
    let mut clients = HashMap::<usize, Arc<UdpSocket>>::new();
    let output = Arc::new(Mutex::new(io::stdout()));

    loop {
        let mut buf = String::new();
        io::stdin().read_line(&mut buf).await?;
        let (id, data) = parse(&buf)?;
        let socket = if let Some(socket) = clients.get(&id) {
            socket.clone()
        } else {
            let socket = UdpSocket::bind("0.0.0.0:0").await?;
            let socket = Arc::new(socket);
            clients.insert(id, socket.clone());
            {
                let output = output.clone();
                let socket = socket.clone();
                task::spawn(async move {
                    let mut buf = [0u8; 65535];
                    loop {
                        let len = socket.recv(&mut buf).await.unwrap();
                        let encoded = base64::prelude::BASE64_STANDARD.encode(&buf[..len]);
                        let data = format!("{id}:{encoded}\n");
                        let mut output = output.lock().await;
                        output.write_all(data.as_bytes()).await.unwrap();
                        output.flush().await.unwrap();
                    }
                });
            }
            socket
        };
        socket.send_to(&data, &target).await.unwrap();
    }
}

async fn run_listen(port: u16) -> Result<(), Box<dyn Error>> {
    let socket = UdpSocket::bind(format!("0.0.0.0:{port}")).await?;

    struct AdddresManager {
        addr2id: HashMap<SocketAddr, usize>,
        id2addr: HashMap<usize, SocketAddr>,
    }

    let clients = Mutex::new(AdddresManager {
        addr2id: HashMap::new(),
        id2addr: HashMap::new(),
    });

    async fn send_task(
        socket: &UdpSocket,
        clients: &Mutex<AdddresManager>,
    ) -> Result<(), Box<dyn Error>> {
        let stdin = io::stdin();

        loop {
            let mut buf = String::new();
            stdin.read_line(&mut buf).await?;
            let (id, data) = parse(&buf)?;
            let addr = clients.lock().await.id2addr.get(&id).unwrap().clone();
            socket.send_to(&data, addr).await?;
        }
    }

    async fn recv_task(
        socket: &UdpSocket,
        clients: &Mutex<AdddresManager>,
    ) -> Result<(), Box<dyn Error>> {
        let mut buf = [0u8; 65535];
        let mut stdout = io::stdout();

        loop {
            let (len, addr) = socket.recv_from(&mut buf).await?;

            let id = {
                let mut clients = clients.lock().await;
                if let Some(id) = clients.addr2id.get(&addr) {
                    *id
                } else {
                    let new_id = clients.addr2id.len();
                    clients.addr2id.insert(addr, new_id);
                    clients.id2addr.insert(new_id, addr);
                    new_id
                }
            };

            let encoded = base64::prelude::BASE64_STANDARD.encode(&buf[..len]);
            let output = format!("{id}:{encoded}\n");
            stdout.write_all(output.as_bytes()).await?;
            stdout.flush().await?;
        }
    }

    try_join!(send_task(&socket, &clients), recv_task(&socket, &clients))?;

    Ok(())
}

enum Config {
    Listen(u16),
    Send(String),
}

fn parse_config(args: Vec<String>) -> Result<Config, String> {
    let this = args[0].clone();

    let Some(kind) = args.get(1) else {
        return Err(format!(
            "Please call with 2 arguments: {this} listen 8080 or {this} send 192.168.0.1:8080"
        )
        .into());
    };

    match &kind[..] {
        "listen" => {
            let Some(port) = args.get(2) else {
                return Err("Please specify a port number".into());
            };
            let Ok(port) = port.parse() else {
                return Err(format!("Invalid port number {port}").into());
            };
            Ok(Config::Listen(port))
        }
        "send" => {
            let Some(target) = args.get(2) else {
                return Err("Please specify target server".into());
            };
            Ok(Config::Send(target.into()))
        }
        _ => {
            return Err("Invalid argument one. Please use listen or send".into());
        }
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = args().collect();

    let config = parse_config(args)?;

    match config {
        Config::Listen(port) => task::block_on(run_listen(port)).unwrap(),
        Config::Send(target) => task::block_on(run_send(target)).unwrap(),
    };

    Ok(())
}
