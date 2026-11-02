//! Human-readable labels for dimension keys.

/// IANA protocol number → short name.
pub fn proto_name(p: u8) -> String {
    match p {
        1 => "ICMP".into(),
        2 => "IGMP".into(),
        6 => "TCP".into(),
        17 => "UDP".into(),
        41 => "IPv6-in-IPv4".into(),
        47 => "GRE".into(),
        50 => "ESP".into(),
        51 => "AH".into(),
        58 => "ICMPv6".into(),
        89 => "OSPF".into(),
        112 => "VRRP".into(),
        132 => "SCTP".into(),
        n => format!("IP/{n}"),
    }
}

pub fn direction_label(d: &str) -> String {
    match d {
        "down" => "Download".into(),
        "up" => "Upload".into(),
        "internal" => "Internal".into(),
        "transit" => "Transit".into(),
        other => other.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels() {
        assert_eq!(proto_name(6), "TCP");
        assert_eq!(proto_name(200), "IP/200");
        assert_eq!(direction_label("down"), "Download");
    }
}
