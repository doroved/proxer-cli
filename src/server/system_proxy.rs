use std::{process::Command, string::FromUtf8Error};

pub enum ProxyState {
    On,
    Off,
}

#[derive(Clone)]
pub struct SystemProxy {
    pub interface: String,
    pub server: String,
    pub port: u16,
}

impl SystemProxy {
    pub fn init(port: u16) -> Self {
        SystemProxy {
            interface: "Wi-Fi".to_string(),
            server: "127.0.0.1".to_string(),
            port,
        }
    }

    pub fn set(&self) {
        // Define proxy types
        let proxy_types = self.get_proxy_types();

        // Go through each proxy type and set server and port
        for proxy_type in proxy_types.iter() {
            let command = format!("-set{proxy_type}");

            let _ = self
                .execute_command(&[
                    &command,
                    &self.interface,
                    &self.server,
                    &self.port.to_string(),
                ])
                .unwrap_or_else(|_| panic!("Failed to set {proxy_type}"));
        }
    }

    pub fn set_state(&self, state: ProxyState) {
        let proxy_types = self.get_proxy_types();
        let proxy_state = match state {
            ProxyState::On => "on",
            ProxyState::Off => "off",
        };

        for proxy_type in proxy_types.iter() {
            let command = format!("-set{proxy_type}state");

            let _ = self
                .execute_command(&[&command, &self.interface, proxy_state])
                .unwrap_or_else(|_| panic!("Failed to set {proxy_type} state"));
        }
    }

    fn get_proxy_types(&self) -> [&'static str; 2] {
        ["webproxy", "securewebproxy"]
    }

    fn execute_command(&self, args: &[&str]) -> Result<String, FromUtf8Error> {
        let output = Command::new("networksetup")
            .args(args)
            .output()
            .expect("Failed to execute command");

        String::from_utf8(output.stdout)
    }
}
