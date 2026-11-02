//! `ConnKey` ↔ wire `Key`, `TapDir` ↔ wire `Dir`.

use tmon_capture::TapDir;
use tmon_core::naming::ConnKey;
use tmon_dpi_proto::{Dir, Key};

pub fn to_wire(k: &ConnKey) -> Key {
    Key {
        lan_ip: k.lan_ip,
        lan_port: k.lan_port,
        remote_ip: k.remote_ip,
        remote_port: k.remote_port,
        proto: k.proto,
    }
}

pub fn from_wire(k: &Key) -> ConnKey {
    ConnKey {
        lan_ip: k.lan_ip,
        lan_port: k.lan_port,
        remote_ip: k.remote_ip,
        remote_port: k.remote_port,
        proto: k.proto,
    }
}

pub fn dir(d: TapDir) -> Dir {
    match d {
        TapDir::LanToRemote => Dir::LanToRemote,
        TapDir::RemoteToLan => Dir::RemoteToLan,
    }
}
