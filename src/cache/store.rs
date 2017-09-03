// Bloom
//
// HTTP REST API caching middleware
// Copyright: 2017, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::cmp;
use std::time::Duration;
use r2d2::Pool;
use r2d2::config::Config;
use r2d2_redis::{RedisConnectionManager, Error};
use redis::{self, Value, Connection, Commands, PipelineCommands};

use super::route::ROUTE_PREFIX;
use APP_CONF;

pub struct CacheStoreBuilder;

pub struct CacheStore {
    pool: Pool<RedisConnectionManager>,
}

#[derive(Debug)]
pub enum CacheStoreError {
    Disconnected,
    Failed,
    Invalid,
    Corrupted,
    TooLarge,
}

#[derive(Debug)]
pub enum CachePurgeVariant {
    Bucket,
    Auth,
}

type CacheResult = Result<Option<String>, CacheStoreError>;

impl CacheStoreBuilder {
    pub fn new() -> CacheStore {
        info!("binding to store backend at {}", APP_CONF.redis.inet);

        let addr_auth = match APP_CONF.redis.password {
            Some(ref password) => format!(":{}@", password),
            None => "".to_string(),
        };

        let tcp_addr_raw =
            format!(
            "redis://{}{}:{}/{}",
            &addr_auth,
            APP_CONF.redis.inet.ip(),
            APP_CONF.redis.inet.port(),
            APP_CONF.redis.database,
        );

        debug!("will connect to redis at: {}", tcp_addr_raw);

        match RedisConnectionManager::new(tcp_addr_raw.as_ref()) {
            Ok(manager) => {
                let config = Config::<Connection, Error>::builder()
                    .pool_size(APP_CONF.redis.pool_size)
                    .idle_timeout(Some(
                        Duration::from_secs(APP_CONF.redis.idle_timeout_seconds),
                    ))
                    .connection_timeout(Duration::from_secs(
                        APP_CONF.redis.connection_timeout_seconds,
                    ))
                    .build();

                match Pool::new(config, manager) {
                    Ok(pool) => {
                        info!("bound to store backend");

                        CacheStore { pool: pool }
                    }
                    Err(_) => panic!("could not spawn redis pool"),
                }
            }
            Err(_) => panic!("could not create redis connection manager"),
        }
    }
}

impl CacheStore {
    pub fn get(&self, key: &str) -> CacheResult {
        get_cache_store_client!(self, client {
            match (*client).get::<_, Value>(key) {
                Ok(value) => {
                    match value {
                        Value::Data(bytes) => {
                            // Decode raw bytes to string
                            if let Ok(string) = String::from_utf8(bytes) {
                                Ok(Some(string))
                            } else {
                                Err(CacheStoreError::Corrupted)
                            }
                        },
                        Value::Nil => Ok(None),
                        _ => Err(CacheStoreError::Invalid),
                    }
                },
                _ => Err(CacheStoreError::Failed),
            }
        })
    }

    pub fn set(
        &self,
        key: &str,
        key_mask: &str,
        value: &str,
        ttl: usize,
        key_tags: Vec<String>,
    ) -> CacheResult {
        get_cache_store_client!(self, client {
            // Cap TTL to 'max_key_expiration'
            let ttl_cap = cmp::min(ttl, APP_CONF.redis.max_key_expiration);

            // Ensure value is not larger than 'max_key_size'
            if value.len() > APP_CONF.redis.max_key_size {
                Err(CacheStoreError::TooLarge)
            } else {
                let mut pipeline = redis::pipe();

                pipeline.set_ex(key, value, ttl_cap).ignore();

                for key_tag in key_tags {
                    pipeline.sadd(&key_tag, key_mask).ignore();
                    pipeline.expire(&key_tag, APP_CONF.redis.max_key_expiration);
                }

                // Bucket (MULTI operation for main data + bucket marker)
                gen_cache_store_empty_result!(
                    pipeline.query::<()>(&*client)
                )
            }
        })
    }

    pub fn purge_tag(&self, variant: &CachePurgeVariant, shard: u8, key_tag: &str) -> CacheResult {
        get_cache_store_client!(self, client {
            // Invoke keyspace cleanup script for key tag
            gen_cache_store_empty_result!(
                redis::Script::new(variant.get_script())
                    .arg(ROUTE_PREFIX)
                    .arg(shard)
                    .arg(key_tag)
                    .invoke::<()>(&*client)
            )
        })
    }
}

impl CachePurgeVariant {
    fn get_script(&self) -> &'static str {
        match *self {
            CachePurgeVariant::Bucket |
            CachePurgeVariant::Auth => {
                r#"
                    local targets = {}

                    for _, tag in pairs(redis.call('SMEMBERS', ARGV[3])) do
                        table.insert(targets, ARGV[1] .. ":" .. ARGV[2] .. ":c:" .. tag)
                    end

                    table.insert(targets, ARGV[3])

                    redis.call('DEL', unpack(targets))
                "#
            }
        }
    }
}
