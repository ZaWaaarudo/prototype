mod network;
mod packets;
mod player;

fn main() {
    network::listen_blocking("localhost:430");
}