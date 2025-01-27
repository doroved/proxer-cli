use clap::Parser;

#[derive(Parser, Debug, Clone)]
#[clap(author, version, about, long_about = None)]
pub struct Opt {
    #[clap(
        long,
        value_name = "u16",
        help = "Set port for proxer-cli. By default 5555."
    )]
    pub port: Option<u16>,

    #[clap(
        long,
        value_name = "string",
        help = "Path to the configuration file. Example: '/path/to/config.json'. Default is ~/.proxer-cli/config.json."
    )]
    pub config: Option<String>,
}
