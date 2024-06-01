use std::env::args;
use std::error::Error;
use std::net::SocketAddr;

use async_std::net::{ToSocketAddrs, UdpSocket};
use async_std::sync::Mutex;
use async_std::{io, task};
use base64::Engine;

use futures::try_join;

struct Reciever<'a> {
    socket: &'a UdpSocket,
    target: Mutex<Option<SocketAddr>>,
}

impl<'a> Reciever<'a> {
    fn new(socket: &'a UdpSocket) -> Self {
        Reciever {
            socket,
            target: Mutex::new(None),
        }
    }

    async fn recv(&self, buf: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
        loop {
            let (len, addr) = self.socket.recv_from(buf).await?;
            let mut target = self.target.lock().await;
            if target.is_none() {
                *target = Some(addr);
            }
            if *target == Some(addr) {
                return Ok((len, addr));
            }
        }
    }
}

struct Sender<'a> {
    socket: &'a UdpSocket,
    target: Mutex<Option<SocketAddr>>,
    buffer: Mutex<Vec<Vec<u8>>>,
}

impl<'a> Sender<'a> {
    fn new(socket: &'a UdpSocket) -> Self {
        Sender {
            socket,
            target: Mutex::new(None),
            buffer: Mutex::new(vec![]),
        }
    }

    fn new_with_target(socket: &'a UdpSocket, target: SocketAddr) -> Self {
        Sender {
            socket,
            target: Mutex::new(Some(target)),
            buffer: Mutex::new(vec![]),
        }
    }

    async fn set_target(&self, target: SocketAddr) -> Result<(), Box<dyn Error>> {
        *self.target.lock().await = Some(target);
        self.flush().await
    }

    async fn send(&self, buf: &[u8]) -> Result<(), Box<dyn Error>> {
        self.buffer.lock().await.push(buf.to_vec());
        self.flush().await
    }

    async fn flush(&self) -> Result<(), Box<dyn Error>> {
        if let Some(target) = *self.target.lock().await {
            let mut buffer = self.buffer.lock().await;
            for buf in &*buffer {
                let len = self.socket.send_to(buf, target).await?;
                if len != buf.len() {
                    return Err("Packet boundary was broken".into());
                }
            }
            buffer.clear();
        }
        Ok(())
    }
}

async fn run(config: Config) -> Result<(), Box<dyn Error>> {
    let socket;
    let sender;
    let reciever;

    match config {
        Config::Send(target) => {
            socket = UdpSocket::bind("0.0.0.0:0").await?;
            let [target, ..] = target.to_socket_addrs().await?.collect::<Vec<_>>()[..] else {
                return Err(format!("Failed to resolve target: {target}").into());
            };
            sender = Sender::new_with_target(&socket, target);
            reciever = Reciever::new(&socket);
        }
        Config::Listen(port) => {
            socket = UdpSocket::bind(format!("0.0.0.0:{port}")).await?;
            sender = Sender::new(&socket);
            reciever = Reciever::new(&socket);
        }
    };

    async fn sender_task(sender: &Sender<'_>) -> Result<(), Box<dyn Error>> {
        loop {
            let mut tmp = String::new();
            io::stdin().read_line(&mut tmp).await?;
            tmp.pop();
            for token in tmp.split_whitespace() {
                let s = base64::prelude::BASE64_STANDARD.decode(&token)?;
                sender.send(&s).await?;
            }
        }
    }

    async fn reciever_task(
        sender: &Sender<'_>,
        reciever: &Reciever<'_>,
    ) -> Result<(), Box<dyn Error>> {
        let mut buf = [0u8; 65535];
        loop {
            let (len, addr) = reciever.recv(&mut buf).await.unwrap();
            sender.set_target(addr).await?;

            let x = base64::prelude::BASE64_STANDARD.encode(&buf[..len]);
            println!("{x}");
        }
    }

    let task1 = sender_task(&sender);
    let task2 = reciever_task(&sender, &reciever);

    try_join!(task1, task2)?;

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
            "Please call with 2 arguments: {this} listen 8080 / {this} send 192.168.0.1:8080"
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

    task::block_on(run(config))?;

    Ok(())
}
