//! Local Network Interface & Physical LAN IP Resolver
//! Detects physical Wi-Fi and Ethernet IP addresses for zero-latency LAN party hosting.

use std::net::{IpAddr, Ipv4Addr, UdpSocket};

pub struct NetworkInfo;

impl NetworkInfo {
    /// Detects the machine's primary local LAN IPv4 address.
    /// Uses kernel routing resolution to automatically select the active physical interface.
    pub fn get_local_ip() -> String {
        // Method 1: Kernel routing table query via UDP socket connect
        // This does not transmit any network packets; it queries the OS routing table.
        if let Ok(socket) = UdpSocket::bind("0.0.0.0:0") {
            if socket.connect("8.8.8.8:80").is_ok() {
                if let Ok(local_addr) = socket.local_addr() {
                    if let IpAddr::V4(ipv4) = local_addr.ip() {
                        if !ipv4.is_loopback() && !Self::is_cgnat(&ipv4) {
                            return ipv4.to_string();
                        }
                    }
                }
            }
        }

        // Method 2: Fallback to standard loopback
        "127.0.0.1".to_string()
    }

    /// Checks if an IPv4 address is in the Carrier-Grade NAT (CGNAT) 100.64.0.0/10 range.
    pub fn is_cgnat(ipv4: &Ipv4Addr) -> bool {
        let octets = ipv4.octets();
        octets[0] == 100 && (64..=127).contains(&octets[1])
    }
}
