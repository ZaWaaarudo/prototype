use async_std::{
    task,
    sync::Arc,
    net::{TcpListener, TcpStream},
    prelude::*,
    io::{BufReader, BufWriter},
};
use futures::channel::mpsc;
use futures::sink::SinkExt;

use rand::Rng;

enum ConnectionEvent {
    Connection(Arc<TcpStream>),
    Disconnection(Arc<TcpStream>),
}

const READ_BUFFER_SIZE: usize = 2 * 1024;
const WRITE_BUFFER_SIZE: usize = 2 * 1024;
const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
const KEY_LEN: u32 = 32;

pub fn listen_blocking(address: &str) {
    task::block_on(listen(address));
}

pub async fn listen(address: &str) {
    let listener: TcpListener = TcpListener::bind(address).await.unwrap();
    let mut accept_queue = listener.incoming();

    let (broker_sender, broker_receiver) = mpsc::unbounded::<ConnectionEvent>();

    let connection_task = task::spawn(connection_broker(broker_receiver));

    while let Some(tcp_stream_result) = accept_queue.next().await {
        match tcp_stream_result {
            Ok(tcp_stream) => {
                task::spawn(handle_connection(broker_sender.clone(), tcp_stream));
            }
            Err(error) => println!("Accept error : {}", error),
        }
    }

    drop(broker_sender);

    connection_task.await;
}

async fn connection_broker(mut connection_receiver: mpsc::UnboundedReceiver<ConnectionEvent>) {
    let mut connections = Vec::new();

    while let Some(event) = connection_receiver.next().await {
        match event {
            ConnectionEvent::Connection(stream) => {
                if !connections.contains(&stream.peer_addr().unwrap()) {
                    connections.push(stream.peer_addr().unwrap());
                }

                println!("{} unique connections", connections.len());
            }
            ConnectionEvent::Disconnection(stream) => println!("{} disconnected", stream.peer_addr().unwrap()),
        }
    }
}

async fn handle_connection(mut broker: mpsc::UnboundedSender<ConnectionEvent>, stream: TcpStream) {
    let stream = Arc::new(stream);

    if let Err(err) = broker.send(ConnectionEvent::Connection(Arc::clone(&stream))).await {
        println!("Connection broker error : {}", err);
        return;
    }

    println!("{} connected", stream.peer_addr().unwrap());

    let mut read_buffer = BufReader::with_capacity(READ_BUFFER_SIZE, &*stream);
    let mut read_buffer_vec = Vec::with_capacity(READ_BUFFER_SIZE);

    let mut write_buffer = BufWriter::with_capacity(WRITE_BUFFER_SIZE, &*stream);

    let key = greet_connection(&mut write_buffer).await;

    let mut packet_rate: usize = 0;
    let mut last_packet_time = std::time::Instant::now();

    loop {
        let bytes_read: usize = match async_std::io::timeout(std::time::Duration::from_secs(60 * 10), read_buffer.read_until(b'\0', &mut read_buffer_vec)).await {
            Ok(bytes) => bytes,
            Err(err) => { // timed out or unexpected dc
                println!("receive error : {}", err);
                break;
            }
        };

        if bytes_read == 0 || bytes_read < 3 || read_buffer_vec[bytes_read-2] != b'\n' {
            break;
        }

        packet_rate += 1;

        if packet_rate > 12 { // Le best serait de check le temps tout le temps pour avoir un counter précis mais celui-là fait le taff et permet d'économiser qlqs calls
            if std::time::Instant::now().duration_since(last_packet_time).as_millis() < 3000 {
                break;
            }

            packet_rate = 0;
            last_packet_time = std::time::Instant::now();
        }

        for packet in read_buffer_vec[0..bytes_read-2].split(|byte| *byte == b'\n') {
            println!("packet : {}", String::from_utf8_lossy(packet));
        }

        read_buffer_vec.clear();
    }

    println!("closing {}", stream.peer_addr().unwrap());

    if let Err(err) = broker.send(ConnectionEvent::Disconnection(stream)).await {
        println!("Connection broker error : {}", err);
    }
}

async fn greet_connection(writer: &mut BufWriter<&TcpStream>) -> String {
    let key = generate_key();

    let _ = writer.write_all(b"HC").await;
    let _ = writer.write_all(key.as_bytes()).await;
    let _ = writer.write_all(b"\0").await;
    let _ = writer.flush().await;

    key
}

fn generate_key() -> String {
    return (0..KEY_LEN).map(|_| CHARSET[rand::thread_rng().gen_range(0, CHARSET.len())] as char).collect();
}