use clap::Parser;

#[derive(Parser, Debug, Clone)]
#[clap(author, version, about, long_about = None)]
pub struct Opt {
    #[clap(
        long,
        value_name = "u16",
        help = "Set port for proxer. By default 5555."
    )]
    pub port: Option<u16>,

    #[clap(
        long,
        value_name = "string",
        help = "Path to the configuration file. Example: '/path/to/config.(jsonc|json)'. Default is ~/.proxer-cli/config.json5."
    )]
    pub config: Option<String>,

    #[clap(
        long,
        value_name = "string",
        help = "Secret token to access the HTTP/S proxerver. Must match the token specified in the proxerver configuration."
    )]
    pub token: Option<String>,
}
