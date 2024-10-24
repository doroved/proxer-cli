use clap::Parser;

#[derive(Parser, Debug, Clone)]
#[clap(author, version, about, long_about = None)]
pub struct Opt {
    #[clap(
        long,
        value_name = "u16",
        help = "Set port for proxer. By default, a random port is used."
    )]
    pub port: Option<u16>,

    #[clap(
        long,
        help = "Enable DPI spoofing for direct connections. Spoofing is disabled by default."
    )]
    pub dpi: bool,

    #[clap(
        long,
        value_name = "string",
        help = "Path to the configuration file. Example: '/path/to/proxer.(json5|json)'. Default is ~/.proxer/config.json5."
    )]
    pub config: Option<String>,

    #[clap(
        long,
        value_name = "string",
        help = "Secret token to access the HTTP/S proxerver. Must match the token specified in the proxerver configuration."
    )]
    pub token: Option<String>,

    #[clap(
        long,
        help = "Show all errors. By default, only critical errors are shown. This option is useful for debugging."
    )]
    pub log_error_all: bool,
}
