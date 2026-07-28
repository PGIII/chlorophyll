use std::net::Ipv4Addr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientConfig {
    pub group: Ipv4Addr,
    pub port: u16,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            group: Ipv4Addr::new(239, 0, 0, 1),
            port: 5000,
        }
    }
}
