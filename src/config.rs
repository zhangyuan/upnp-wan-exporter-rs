use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    pub server: ServerConfig,
    #[serde(default)]
    pub upnp: UpnpConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ServerConfig {
    pub port: u16,
}

/// Options for locating the UPnP device (router).
///
/// By default the exporter discovers the device with a multicast SSDP search,
/// which only reaches devices on the same link-local network. If the router
/// sits one network upstream (e.g. double NAT), point the exporter at it
/// explicitly with `host` or `location`.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct UpnpConfig {
    /// Full URL of the device description document. When set, SSDP discovery is
    /// skipped entirely. Most reliable option when multicast cannot reach the
    /// router. Example: `http://192.168.1.1:1900/igd.xml`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,

    /// Restrict SSDP discovery to this host using a unicast M-SEARCH instead of
    /// multicast. Useful when the router is upstream and multicast cannot reach
    /// it. Example: `192.168.1.1`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,

    /// SSDP port used together with `host` (defaults to 1900).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssdp_port: Option<u16>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            server: ServerConfig { port: 9091 },
            upnp: UpnpConfig::default(),
        }
    }
}

impl Config {
    pub fn from_file(path: &str) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: Config = toml::from_str(&content)?;
        Ok(config)
    }
}
