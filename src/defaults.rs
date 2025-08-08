use crate::models::{OutputFormat, Protocol};

pub struct Defaults;

impl Defaults {
    pub const SERVER_HOSTNAME: &'static str = "localhost";
    pub const SERVER_PORT: u16 = 8080;
    pub const SERVER_SSL_VALIDATION: bool = true;
    pub const USER_USERNAME: &'static str = "default_user";
    pub const CACHE_TIME: u64 = 3600;
    pub const CACHE_SIZE: i32 = 104_857_600; // 100 MB
    pub const CACHE_DISABLE: bool = false;
    pub const COMPLETION_DISABLE_API_RELATED: bool = false;
    pub const API_VERSION: &'static str = "v1";
    pub const PROTOCOL: Protocol = Protocol::Https;
    pub const OUTPUT_FORMAT: OutputFormat = OutputFormat::Text;
    pub const OUTPUT_PADDING: i8 = 15;
}
