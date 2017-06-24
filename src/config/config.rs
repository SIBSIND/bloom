// Bloom
//
// HTTP REST API caching middleware
// Copyright: 2017, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::net::SocketAddr;
use std::path::PathBuf;

pub struct Config {
    pub listen: ConfigListen,
    pub memcached: ConfigMemcached
}

pub struct ConfigListen {
    pub inet: SocketAddr
}

pub struct ConfigMemcached {
    pub inet: SocketAddr,
    pub max_key_size: u32,
    pub max_key_expiration: u32,
    pub pool_size: u8,
    pub reconnect: u16,
    pub timeout: u16
}
