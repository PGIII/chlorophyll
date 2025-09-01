use std::io;
use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};

fn main() -> io::Result<()> {
    // Multicast group and port
    let multicast_addr = Ipv4Addr::new(239, 0, 0, 1); // Example multicast address
    let port = 5000;

    // Bind to any address on the given port
    let socket_addr = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, port);
    let socket = UdpSocket::bind(socket_addr)?;

    // Join the multicast group on the default interface (0.0.0.0)
    socket.join_multicast_v4(&multicast_addr, &Ipv4Addr::UNSPECIFIED)?;

    println!("Listening for multicast on {}:{}", multicast_addr, port);

    let mut buf = [0u8; 1500];
    loop {
        let (len, src) = socket.recv_from(&mut buf)?;
        let msg = String::from_utf8_lossy(&buf[..len]);
        println!("Received from {}: {}", src, msg);
    }
}
