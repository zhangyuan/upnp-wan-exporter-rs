use anyhow::{anyhow, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::net::UdpSocket;
use tracing::{debug, error, warn};
use xml::reader::{EventReader, XmlEvent};

const UPNP_MULTICAST_ADDR: &str = "239.255.255.250:1900";
const UPNP_SEARCH_MSG: &str = concat!(
    "M-SEARCH * HTTP/1.1\r\n",
    "HOST: 239.255.255.250:1900\r\n",
    "MAN: \"ssdp:discover\"\r\n",
    "ST: urn:schemas-upnp-org:device:InternetGatewayDevice:1\r\n",
    "MX: 3\r\n\r\n"
);

#[derive(Debug, Clone)]
pub struct UpnpDevice {
    pub location: String,
    pub wan_common_service_url: Option<String>,
    pub wan_ip_service_url: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct TrafficStats {
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub packets_sent: u64,
    pub packets_received: u64,
    pub connection_status: String,
}

impl Default for TrafficStats {
    fn default() -> Self {
        Self {
            bytes_sent: 0,
            bytes_received: 0,
            packets_sent: 0,
            packets_received: 0,
            connection_status: "Disconnected".to_string(),
        }
    }
}

pub struct UpnpClient {
    client: Client,
    device: Option<UpnpDevice>,
}

impl Default for UpnpClient {
    fn default() -> Self {
        Self::new()
    }
}

impl UpnpClient {
    pub fn new() -> Self {
        Self {
            client: Client::new(),
            device: None,
        }
    }

    pub async fn discover_device(&mut self) -> Result<()> {
        debug!("Starting UPnP device discovery");

        let socket = UdpSocket::bind("0.0.0.0:0").await?;
        socket.set_broadcast(true)?;

        // Send SSDP discovery message
        socket
            .send_to(UPNP_SEARCH_MSG.as_bytes(), UPNP_MULTICAST_ADDR)
            .await?;

        let mut buf = [0; 1024];

        // Wait for responses with timeout
        match tokio::time::timeout(Duration::from_secs(5), socket.recv_from(&mut buf)).await {
            Ok(Ok((len, _addr))) => {
                let response = String::from_utf8_lossy(&buf[..len]);
                debug!("Received SSDP response: {}", response);

                // Parse location from response
                if let Some(location) = self.extract_location(&response) {
                    debug!("Found UPnP device at: {}", location);
                    self.device = Some(UpnpDevice {
                        location: location.clone(),
                        wan_common_service_url: None,
                        wan_ip_service_url: None,
                    });

                    // Get device description and find WAN service
                    self.setup_service().await?;
                }
            }
            Ok(Err(e)) => {
                error!("Socket error during discovery: {}", e);
                return Err(anyhow!("Socket error: {}", e));
            }
            Err(_) => {
                warn!("No UPnP devices found within timeout");
                return Err(anyhow!("Discovery timeout"));
            }
        }

        Ok(())
    }

    fn extract_location(&self, response: &str) -> Option<String> {
        for line in response.lines() {
            if line.to_lowercase().starts_with("location:") {
                return line
                    .split(':')
                    .skip(1)
                    .collect::<Vec<_>>()
                    .join(":")
                    .trim()
                    .to_string()
                    .into();
            }
        }
        None
    }

    async fn setup_service(&mut self) -> Result<()> {
        let device = self
            .device
            .as_ref()
            .ok_or_else(|| anyhow!("No device found"))?;

        debug!("Fetching device description from: {}", device.location);
        let desc_response = self.client.get(&device.location).send().await?;
        let desc_xml = desc_response.text().await?;

        // Parse XML to find WAN service URLs
        let (wan_common_url, wan_ip_url) = self.parse_service_urls(&desc_xml, &device.location)?;

        if let Some(ref mut dev) = self.device {
            dev.wan_common_service_url = wan_common_url;
            dev.wan_ip_service_url = wan_ip_url;
        }

        Ok(())
    }

    fn parse_service_urls(
        &self,
        xml: &str,
        _base_url: &str,
    ) -> Result<(Option<String>, Option<String>)> {
        let mut reader = EventReader::from_str(xml);
        let mut wan_common_url: Option<String> = None;
        let mut wan_ip_url: Option<String> = None;
        let mut current_service_type = String::new();
        let mut current_control_url = String::new();
        let mut in_service = false;
        let mut in_service_type = false;
        let mut in_control_url = false;

        loop {
            match reader.next() {
                Ok(XmlEvent::StartElement { name, .. }) => match name.local_name.as_str() {
                    "service" => {
                        in_service = true;
                        current_service_type.clear();
                        current_control_url.clear();
                    }
                    "serviceType" if in_service => in_service_type = true,
                    "controlURL" if in_service => in_control_url = true,
                    _ => {}
                },
                Ok(XmlEvent::EndElement { name }) => match name.local_name.as_str() {
                    "service" => {
                        if current_service_type.contains("WANCommonInterfaceConfig") {
                            let full_url = if current_control_url.starts_with("http") {
                                current_control_url.clone()
                            } else {
                                format!("http://192.168.3.1:1900{}", current_control_url)
                            };
                            debug!("Found WANCommonInterfaceConfig service at: {}", full_url);
                            wan_common_url = Some(full_url);
                        } else if current_service_type.contains("WANIPConnection") {
                            let full_url = if current_control_url.starts_with("http") {
                                current_control_url.clone()
                            } else {
                                format!("http://192.168.3.1:1900{}", current_control_url)
                            };
                            debug!("Found WANIPConnection service at: {}", full_url);
                            wan_ip_url = Some(full_url);
                        }
                        in_service = false;
                    }
                    "serviceType" => in_service_type = false,
                    "controlURL" => in_control_url = false,
                    _ => {}
                },
                Ok(XmlEvent::Characters(text)) => {
                    if in_service_type {
                        current_service_type = text;
                    } else if in_control_url {
                        current_control_url = text;
                    }
                }
                Ok(XmlEvent::EndDocument) => break,
                Err(e) => {
                    error!("XML parsing error: {}", e);
                    break;
                }
                _ => {}
            }
        }

        if wan_common_url.is_none() {
            return Err(anyhow!("WANCommonInterfaceConfig service not found"));
        }

        Ok((wan_common_url, wan_ip_url))
    }

    pub async fn get_traffic_stats(&self) -> Result<TrafficStats> {
        let device = self
            .device
            .as_ref()
            .ok_or_else(|| anyhow!("No device configured"))?;
        let wan_common_url = device
            .wan_common_service_url
            .as_ref()
            .ok_or_else(|| anyhow!("No WANCommonInterfaceConfig service URL"))?;
        let _wan_ip_url = device.wan_ip_service_url.as_ref();

        let mut stats = TrafficStats::default();

        // Get bytes sent
        if let Ok(bytes_sent) = self.get_total_bytes_sent(wan_common_url).await {
            stats.bytes_sent = bytes_sent;
        }

        // Get bytes received
        if let Ok(bytes_received) = self.get_total_bytes_received(wan_common_url).await {
            stats.bytes_received = bytes_received;
        }

        // Get packets sent
        if let Ok(packets_sent) = self.get_total_packets_sent(wan_common_url).await {
            stats.packets_sent = packets_sent;
        }

        // Get packets received
        if let Ok(packets_received) = self.get_total_packets_received(wan_common_url).await {
            stats.packets_received = packets_received;
        }

        // Get connection status
        if let Ok(link_status) = self.get_physical_link_status(wan_common_url).await {
            stats.connection_status = link_status;
        }

        Ok(stats)
    }

    async fn get_total_bytes_sent(&self, service_url: &str) -> Result<u64> {
        let soap_body = r#"<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
    <s:Body>
        <u:GetTotalBytesSent xmlns:u="urn:schemas-upnp-org:service:WANCommonInterfaceConfig:1" />
    </s:Body>
</s:Envelope>"#;

        let response = self
            .soap_request(
                service_url,
                "urn:schemas-upnp-org:service:WANCommonInterfaceConfig:1#GetTotalBytesSent",
                soap_body,
            )
            .await?;
        self.parse_u64_response(&response, "NewTotalBytesSent")
    }

    async fn get_total_bytes_received(&self, service_url: &str) -> Result<u64> {
        let soap_body = r#"<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
    <s:Body>
        <u:GetTotalBytesReceived xmlns:u="urn:schemas-upnp-org:service:WANCommonInterfaceConfig:1" />
    </s:Body>
</s:Envelope>"#;

        let response = self
            .soap_request(
                service_url,
                "urn:schemas-upnp-org:service:WANCommonInterfaceConfig:1#GetTotalBytesReceived",
                soap_body,
            )
            .await?;
        self.parse_u64_response(&response, "NewTotalBytesReceived")
    }

    async fn get_total_packets_sent(&self, service_url: &str) -> Result<u64> {
        let soap_body = r#"<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
    <s:Body>
        <u:GetTotalPacketsSent xmlns:u="urn:schemas-upnp-org:service:WANCommonInterfaceConfig:1" />
    </s:Body>
</s:Envelope>"#;

        let response = self
            .soap_request(
                service_url,
                "urn:schemas-upnp-org:service:WANCommonInterfaceConfig:1#GetTotalPacketsSent",
                soap_body,
            )
            .await?;
        self.parse_u64_response(&response, "NewTotalPacketsSent")
    }

    async fn get_total_packets_received(&self, service_url: &str) -> Result<u64> {
        let soap_body = r#"<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
    <s:Body>
        <u:GetTotalPacketsReceived xmlns:u="urn:schemas-upnp-org:service:WANCommonInterfaceConfig:1" />
    </s:Body>
</s:Envelope>"#;

        let response = self
            .soap_request(
                service_url,
                "urn:schemas-upnp-org:service:WANCommonInterfaceConfig:1#GetTotalPacketsReceived",
                soap_body,
            )
            .await?;
        self.parse_u64_response(&response, "NewTotalPacketsReceived")
    }

    async fn get_physical_link_status(&self, service_url: &str) -> Result<String> {
        let soap_body = r#"<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
    <s:Body>
        <u:GetCommonLinkProperties xmlns:u="urn:schemas-upnp-org:service:WANCommonInterfaceConfig:1" />
    </s:Body>
</s:Envelope>"#;

        let response = self
            .soap_request(
                service_url,
                "urn:schemas-upnp-org:service:WANCommonInterfaceConfig:1#GetCommonLinkProperties",
                soap_body,
            )
            .await?;
        self.parse_string_response(&response, "NewPhysicalLinkStatus")
    }

    async fn soap_request(
        &self,
        service_url: &str,
        soap_action: &str,
        soap_body: &str,
    ) -> Result<String> {
        debug!("SOAP request to {}: {}", service_url, soap_action);

        let response = self
            .client
            .post(service_url)
            .header("Content-Type", "text/xml; charset=\"utf-8\"")
            .header("SOAPAction", format!("\"{}\";", soap_action))
            .body(soap_body.to_string())
            .send()
            .await?;

        let response_text = response.text().await?;
        debug!("SOAP response: {}", response_text);

        Ok(response_text)
    }

    fn parse_u64_response(&self, xml: &str, element_name: &str) -> Result<u64> {
        let mut reader = EventReader::from_str(xml);
        let mut in_target_element = false;

        loop {
            match reader.next() {
                Ok(XmlEvent::StartElement { name, .. }) => {
                    if name.local_name == element_name {
                        in_target_element = true;
                    }
                }
                Ok(XmlEvent::Characters(text)) if in_target_element => {
                    return text
                        .parse::<u64>()
                        .map_err(|e| anyhow!("Failed to parse {}: {}", element_name, e));
                }
                Ok(XmlEvent::EndElement { name }) => {
                    if name.local_name == element_name {
                        in_target_element = false;
                    }
                }
                Ok(XmlEvent::EndDocument) => break,
                Err(e) => {
                    error!("XML parsing error: {}", e);
                    break;
                }
                _ => {}
            }
        }

        Err(anyhow!("Element {} not found in response", element_name))
    }

    fn parse_string_response(&self, xml: &str, element_name: &str) -> Result<String> {
        let mut reader = EventReader::from_str(xml);
        let mut in_target_element = false;

        loop {
            match reader.next() {
                Ok(XmlEvent::StartElement { name, .. }) => {
                    if name.local_name == element_name {
                        in_target_element = true;
                    }
                }
                Ok(XmlEvent::Characters(text)) if in_target_element => {
                    return Ok(text);
                }
                Ok(XmlEvent::EndElement { name }) => {
                    if name.local_name == element_name {
                        in_target_element = false;
                    }
                }
                Ok(XmlEvent::EndDocument) => break,
                Err(e) => {
                    error!("XML parsing error: {}", e);
                    break;
                }
                _ => {}
            }
        }

        Err(anyhow!("Element {} not found in response", element_name))
    }
}
